// OPCUA for Rust
// SPDX-License-Identifier: MPL-2.0
// Copyright (C) 2017-2024 Adam Lock

//! The certificate store holds and retrieves private keys and certificates from disk. It is responsible
//! for checking certificates supplied by the remote end to see if they are valid and trusted or not.

use std::fs::{read_dir, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use opcua_types::{status_code::StatusCode, Error};
use tracing::{debug, error, info, trace, warn};

use x509_cert::{
    crl::CertificateList,
    der::{Decode, Encode, Reader},
};

use crate::{
    validate_certificate_chain, CertificatePurpose, ChainValidationContext, PrivateKey,
    SuppressedFinding, SuppressibleStep, ValidationOptions,
};

use super::{
    security_policy::SecurityPolicy,
    x509::{X509Data, X509},
};

use super::ocsp::{self, CacheKey, OcspError, OcspFetchConfig, OcspFetchPolicy};

/// Default path to the applications own certificate
const OWN_CERTIFICATE_PATH: &str = "own/cert.der";
/// Default path to the applications own private key
const OWN_PRIVATE_KEY_PATH: &str = "private/private.pem";
/// The directory holding trusted certificates
const TRUSTED_CERTS_DIR: &str = "trusted";
/// The directory holding issuer certificates
const ISSUER_CERTS_DIR: &str = "issuer";
/// The directory holding CRLs for trusted CA certificates
const TRUSTED_CRLS_DIR: &str = "trusted_crls";
/// The directory holding CRLs for issuer CA certificates
const ISSUER_CRLS_DIR: &str = "issuer_crls";
/// The directory holding rejected certificates
const REJECTED_CERTS_DIR: &str = "rejected";
/// The PKI subdirectory for the HTTPS CertificateGroup.
const HTTPS_GROUP_DIR: &str = "https";
/// The PKI subdirectory for the user-token CertificateGroup.
const USER_TOKEN_GROUP_DIR: &str = "user_token";

/// The standard CertificateGroups whose TrustLists are exposed by `ServerConfiguration`.
///
/// The application group retains the historical PKI layout directly under the configured PKI
/// root. The HTTPS and user-token groups use dedicated subdirectories so their trust anchors and
/// revocation lists cannot affect application-instance certificate validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CertificateGroup {
    /// `ServerConfiguration.CertificateGroups.DefaultApplicationGroup`.
    DefaultApplication,
    /// `ServerConfiguration.CertificateGroups.DefaultHttpsGroup`.
    DefaultHttps,
    /// `ServerConfiguration.CertificateGroups.DefaultUserTokenGroup`.
    DefaultUserToken,
}

impl CertificateGroup {
    /// All standard CertificateGroups in deterministic order.
    pub const ALL: [Self; 3] = [
        Self::DefaultApplication,
        Self::DefaultHttps,
        Self::DefaultUserToken,
    ];

    const fn directory_name(self) -> Option<&'static str> {
        match self {
            Self::DefaultApplication => None,
            Self::DefaultHttps => Some(HTTPS_GROUP_DIR),
            Self::DefaultUserToken => Some(USER_TOKEN_GROUP_DIR),
        }
    }
}

#[derive(Clone, Copy)]
enum IncomingCertificateKind {
    ApplicationInstance,
    UserIdentity,
}

impl IncomingCertificateKind {
    fn trust_list_status(self) -> StatusCode {
        StatusCode::BadCertificateUntrusted
    }

    fn rejected_store_status(self) -> StatusCode {
        self.trust_list_status()
    }

    fn invalid_structure_status(self) -> StatusCode {
        match self {
            IncomingCertificateKind::ApplicationInstance => StatusCode::BadSecurityChecksFailed,
            IncomingCertificateKind::UserIdentity => StatusCode::BadCertificateInvalid,
        }
    }

    fn policy_check_status(self) -> StatusCode {
        match self {
            IncomingCertificateKind::ApplicationInstance => StatusCode::BadSecurityChecksFailed,
            IncomingCertificateKind::UserIdentity => StatusCode::BadCertificatePolicyCheckFailed,
        }
    }
}

/// The certificate store manages the storage of a server/client's own certificate & private key
/// and the trust / rejection of certificates from the other end.
pub struct CertificateStore {
    /// Path to the applications own certificate
    own_certificate_path: PathBuf,
    /// Path to the applications own private key
    own_private_key_path: PathBuf,
    /// Path to the certificate store on disk
    pub(crate) pki_path: PathBuf,
    /// Timestamps of the cert are normally checked on the cert to ensure it cannot be used before
    /// or after its limits, but this check can be disabled.
    check_time: bool,
    /// This option lets you skip additional certificate validations (e.g. hostname, application
    /// uri and the not before / after values). Certificates are always checked to see if they are
    /// trusted and have a valid key length.
    skip_verify_certs: bool,
    /// Ordinarily an unknown cert will be dropped into the rejected folder, but it can be dropped
    /// into the trusted folder if this flag is set. Certs in the trusted folder must still pass
    /// validity checks.
    #[allow(dead_code)] // retained for the US5 legacy trust-list path
    trust_unknown_certs: bool,
    /// Certificate validation options applied to incoming application instance certificates.
    validation_options: ValidationOptions,
    /// OCSP fetch configuration. When `None` or `policy == Off`, no live OCSP fetching is performed.
    ocsp_fetch_config: Option<OcspFetchConfig>,
    /// OCSP response cache, shared across all validations.
    ocsp_cache: parking_lot::Mutex<ocsp::cache::OcspCache>,
}

impl CertificateStore {
    /// Sets up the certificate store to the specified PKI directory.
    /// It is a bad idea to have more than one running instance pointing to the same path
    /// location on disk.
    pub fn new(pki_path: &Path) -> CertificateStore {
        CertificateStore {
            own_certificate_path: PathBuf::from(OWN_CERTIFICATE_PATH),
            own_private_key_path: PathBuf::from(OWN_PRIVATE_KEY_PATH),
            pki_path: pki_path.to_path_buf(),
            check_time: true,
            skip_verify_certs: false,
            trust_unknown_certs: false,
            validation_options: ValidationOptions::default(),
            ocsp_fetch_config: None,
            ocsp_cache: parking_lot::Mutex::new(ocsp::cache::OcspCache::new()),
        }
    }

    /// Create a new certificate store with application certificate from the given
    /// `cert_path`.
    pub fn new_with_x509_data<X>(
        pki_path: &Path,
        overwrite: bool,
        cert_path: Option<&Path>,
        pkey_path: Option<&Path>,
        x509_data: Option<X>,
    ) -> (CertificateStore, Option<X509>, Option<PrivateKey>)
    where
        X: Into<X509Data>,
    {
        let mut certificate_store = CertificateStore::new(pki_path);
        if let (Some(cert_path), Some(pkey_path)) = (cert_path, pkey_path) {
            certificate_store.own_certificate_path = cert_path.to_path_buf();
            certificate_store.own_private_key_path = pkey_path.to_path_buf();
        }
        let (cert, pkey) = if certificate_store.ensure_pki_path().is_err() {
            error!(
                "Folder for storing certificates cannot be examined so server has no application instance certificate or private key."
            );
            (None, None)
        } else {
            let cert = certificate_store.read_own_cert();
            let pkey = certificate_store.read_own_pkey();
            match (cert, pkey, x509_data) {
                (Ok(cert), Ok(pkey), _) => (Some(cert), Some(pkey)),
                (_, _, Some(x509_data)) => {
                    info!("Creating sample application instance certificate and private key");
                    let x509_data = x509_data.into();
                    let result = certificate_store
                        .create_and_store_application_instance_cert(&x509_data, overwrite);
                    match result {
                        Ok((cert, pkey)) => (Some(cert), Some(pkey)),
                        Err(err) => {
                            error!("Certificate creation failed, error = {}", err);
                            (None, None)
                        }
                    }
                }
                (Err(e1), Err(e2), _) => {
                    error!("Failed to get cert and private key: {e1}, {e2}");
                    (None, None)
                }
                (Err(e), _, _) | (_, Err(e), _) => {
                    error!("Failed to get cert or private key: {e}");
                    (None, None)
                }
            }
        };
        (certificate_store, cert, pkey)
    }

    /// Set `skip_verify_certs` to not verify incoming certificates.
    pub fn set_skip_verify_certs(&mut self, skip_verify_certs: bool) {
        self.skip_verify_certs = skip_verify_certs;
    }

    /// Set `trust_unknown_certs` to automatically trust valid but
    /// untrusted certificates.
    pub fn set_trust_unknown_certs(&mut self, trust_unknown_certs: bool) {
        self.trust_unknown_certs = trust_unknown_certs;
    }

    /// Check expiration time of incoming certificates.
    pub fn set_check_time(&mut self, check_time: bool) {
        self.check_time = check_time;
    }

    /// Set certificate validation options for incoming application instance certificates.
    pub fn set_validation_options(&mut self, options: ValidationOptions) {
        self.validation_options = options;
    }

    /// Configure live OCSP fetching. `None` or `Off` policy preserves
    /// backward-compatible behavior (stapled/supplied OCSP only).
    pub fn set_ocsp_fetch_config(&mut self, config: Option<OcspFetchConfig>) {
        self.ocsp_fetch_config = config;
    }

    /// Reads a private key from a path on disk.
    pub fn read_pkey(path: &Path) -> Result<PrivateKey, String> {
        if let Ok(pkey) = PrivateKey::read_pem_file(path) {
            return Ok(pkey);
        }

        Err(format!("Cannot read pkey from path {path:?}"))
    }

    /// Reads the store's own certificate
    pub fn read_own_cert(&self) -> Result<X509, String> {
        CertificateStore::read_cert(&self.own_certificate_path()).map_err(|e| {
            format!(
                "Cannot read cert from path {:?}: {e}",
                self.own_certificate_path()
            )
        })
    }

    /// Read own private key from file.
    pub fn read_own_pkey(&self) -> Result<PrivateKey, String> {
        CertificateStore::read_pkey(&self.own_private_key_path()).map_err(|e| {
            format!(
                "Cannot read pkey from path {:?}: {e}",
                self.own_private_key_path()
            )
        })
    }

    /// Create a certificate and key pair to the specified locations
    pub fn create_certificate_and_key(
        args: &X509Data,
        overwrite: bool,
        cert_path: &Path,
        pkey_path: &Path,
    ) -> Result<(X509, PrivateKey), String> {
        let (cert, pkey) = X509::cert_and_pkey(args)?;

        // Write the public cert
        let _ = CertificateStore::store_cert(&cert, cert_path, overwrite)?;

        // Write the private key
        use rsa::pkcs8;
        use x509_cert::der::pem::PemLabel;
        let doc = pkey
            .to_der()
            .map_err(|e| format!("Failed to convert private key to DER: {e:?}"))?;
        let pem = doc
            .to_pem(rsa::pkcs8::PrivateKeyInfo::PEM_LABEL, pkcs8::LineEnding::CR)
            .map_err(|e| format!("Failed to convert private key to PEM: {e:?}"))?;
        let _ = CertificateStore::write_private_key_to_file(pem.as_bytes(), pkey_path, overwrite)?;
        Ok((cert, pkey))
    }

    /// This function will use the supplied arguments to create an Application Instance Certificate
    /// consisting of a X509v3 certificate and public/private key pair. The cert (including pubkey)
    /// and private key will be written to disk under the pki path.
    pub fn create_and_store_application_instance_cert(
        &self,
        args: &X509Data,
        overwrite: bool,
    ) -> Result<(X509, PrivateKey), String> {
        CertificateStore::create_certificate_and_key(
            args,
            overwrite,
            &self.own_certificate_path(),
            &self.own_private_key_path(),
        )
    }

    /// Validates the cert as trusted and valid. If the cert is unknown, it will be written to
    /// the rejected folder so that the administrator can manually move it to the trusted folder.
    ///
    /// # Errors
    ///
    /// A non `Good` status code indicates a failure in the cert or in some action required in
    /// order to validate it.
    ///
    pub fn validate_or_reject_application_instance_cert(
        &self,
        cert: &X509,
        security_policy: SecurityPolicy,
        hostname: Option<&str>,
        application_uri: Option<&str>,
    ) -> Result<(), Error> {
        self.validate_application_instance_cert(cert, security_policy, hostname, application_uri)
    }

    /// Validates an X.509 user identity certificate before thumbprint-based user mapping.
    ///
    /// User identity certificates use the same trust-chain, validity, revocation, security-policy,
    /// and usage pipeline as incoming application certificates, but a configured user thumbprint is
    /// not a trust anchor. Suppressed non-critical findings are returned to the caller so they can
    /// be audited as required by OPC UA Part 4.
    pub fn validate_user_identity_cert(
        &self,
        cert: &X509,
        security_policy: SecurityPolicy,
    ) -> Result<Vec<SuppressedFinding>, Error> {
        self.validate_incoming_cert(
            cert,
            security_policy,
            CertificatePurpose::ClientApplication,
            false,
            IncomingCertificateKind::UserIdentity,
        )
    }

    /// Ensures that the cert provided is the same as the one specified by a path. This is a
    /// security check to stop someone from renaming a cert on disk to match another cert and
    /// somehow bypassing or subverting a check. The disk cert must exactly match the memory cert
    /// or the test is assumed to fail.
    #[allow(dead_code)] // retained for the US5 legacy trust-list path
    fn ensure_cert_and_file_are_the_same(cert: &X509, cert_path: &Path) -> bool {
        if !cert_path.exists() {
            trace!("Cannot find cert on disk");
            false
        } else {
            match CertificateStore::read_cert(cert_path) {
                Ok(file_der) => {
                    // Compare the buffers
                    trace!("Comparing cert on disk to memory");
                    let der;
                    {
                        let r = cert.to_der();
                        match r {
                            Err(_) => return false,
                            Ok(val) => der = val,
                        }
                    }

                    let target_der;
                    {
                        let r = file_der.to_der();
                        match r {
                            Err(_) => return false,
                            Ok(val) => target_der = val,
                        }
                    }

                    der == target_der
                }
                Err(err) => {
                    trace!("Cannot read cert from disk {:?} - {}", cert_path, err);
                    // No cert2 to compare to
                    false
                }
            }
        }
    }

    /// Validates the certificate according to the strictness set in the CertificateStore itself.
    /// Validation might include checking the issue time, expiration time, revocation, trust chain
    /// etc. In the first instance this function will only check if the cert is recognized
    /// and is already contained in the trusted or rejected folder.
    ///
    /// # Errors
    ///
    /// A non `Good` status code indicates a failure in the cert or in some action required in
    /// order to validate it.
    ///
    pub fn validate_application_instance_cert(
        &self,
        cert: &X509,
        security_policy: SecurityPolicy,
        hostname: Option<&str>,
        application_uri: Option<&str>,
    ) -> Result<(), Error> {
        // Server application certificates carry host names; client validation does not.
        let purpose = if hostname.is_some() {
            CertificatePurpose::ServerApplication
        } else {
            CertificatePurpose::ClientApplication
        };
        let cert_file_name = CertificateStore::cert_file_name(cert);
        let findings = self.validate_incoming_cert(
            cert,
            security_policy,
            purpose,
            true,
            IncomingCertificateKind::ApplicationInstance,
        )?;
        for finding in findings {
            warn!(
                "Certificate {cert_file_name}: suppressed certificate-validation finding [{:?}] {} - {}",
                finding.step, finding.status, finding.message
            );
        }

        if self.skip_verify_certs {
            debug!(
                "Skipping additional verifications for certificate {}",
                cert_file_name
            );
            return Ok(());
        }

        // Compare the hostname of the cert against the cert supplied
        if let Some(hostname) = hostname {
            cert.is_hostname_valid(hostname)?;
        }

        // Compare the application / product uri to the supplied application description
        if let Some(application_uri) = application_uri {
            cert.is_application_uri_valid(application_uri)?;
        }

        Ok(())
    }

    fn validate_incoming_cert(
        &self,
        cert: &X509,
        security_policy: SecurityPolicy,
        purpose: CertificatePurpose,
        allow_trust_unknown_certs: bool,
        kind: IncomingCertificateKind,
    ) -> Result<Vec<SuppressedFinding>, Error> {
        let cert_file_name = CertificateStore::cert_file_name(cert);
        debug!("Validating cert with name on disk {}", cert_file_name);

        // Reject unsupported / unavailable security policies before any policy-crypto call.
        security_policy.ensure_supported()?;

        // Look for the cert in the rejected folder. If it's rejected there is no purpose going
        // any further
        {
            let mut cert_path = self.rejected_certs_dir();
            if !cert_path.exists() {
                error!(
                    "Path for rejected certificates {} does not exist",
                    cert_path.display()
                );
                return Err(Error::new(
                    StatusCode::BadUnexpectedError,
                    format!(
                        "Path for rejected certificates {} does not exist",
                        cert_path.display()
                    ),
                ));
            }
            cert_path.push(&cert_file_name);
            if cert_path.exists() {
                warn!(
                    "Certificate {} is untrusted because it resides in the rejected directory",
                    cert_file_name
                );
                return Err(Error::new(
                    kind.rejected_store_status(),
                    format!(
                        "Certificate {} is untrusted because it resides in the rejected directory",
                        cert_file_name
                    ),
                ));
            }
        }

        #[cfg(feature = "ecc")]
        cert.ensure_curve_matches_policy(security_policy)?;

        // Check that the certificate is the right length for the security policy
        match cert.key_length() {
            Err(_) => {
                error!("Cannot read key length from certificate {}", cert_file_name);
                return Err(Error::new(
                    kind.invalid_structure_status(),
                    format!("Cannot read key length from certificate {}", cert_file_name),
                ));
            }
            Ok(key_length) => {
                if !security_policy.is_valid_keylength(key_length) {
                    warn!(
                        "Certificate {} has an invalid key length {} for the policy {}",
                        cert_file_name, key_length, security_policy
                    );
                    return Err(Error::new(
                        kind.policy_check_status(),
                        format!(
                            "Certificate {} has an invalid key length {} for the policy {}",
                            cert_file_name, key_length, security_policy
                        ),
                    ));
                }
            }
        }

        let mut options = self.validation_options.clone();
        if !self.check_time || self.skip_verify_certs {
            options.suppressed_steps.insert(SuppressibleStep::Validity);
        }

        self.ensure_trusted_certs_dir_available(cert, &cert_file_name, kind)?;

        let mut trusted = self.read_trusted_certs_for_validation(cert, &cert_file_name, kind)?;
        // Honor trust_unknown_certs: auto-trust an unknown presented certificate by persisting it and
        // making it its own trust anchor. The chain engine still verifies its signature (a self-signed
        // cert self-verifies; a non-self-signed anchor is trusted as presented) and its validity period.
        if allow_trust_unknown_certs && self.trust_unknown_certs {
            let already_trusted = trusted.iter().any(|t| t.thumbprint() == cert.thumbprint());
            if !already_trusted {
                warn!(
                    "Certificate {} is unknown but trust_unknown_certs is set, so it will be trusted",
                    cert_file_name
                );
                let _ = self.store_trusted_cert(cert);
                trusted.push(cert.clone());
            }
        }
        let issuers = self.read_issuer_certs();
        let mut crls = self.read_trusted_crls();
        crls.extend(self.read_issuer_crls());
        let now = chrono::Utc::now();

        let mut fetched_ocsp = Vec::new();
        if let Some(ref config) = self.ocsp_fetch_config {
            if config.policy != OcspFetchPolicy::Off {
                let result = self.fetch_ocsp_for_cert(cert, config, &trusted, &issuers, &now);
                match (&config.policy, result) {
                    (OcspFetchPolicy::Strict, Err(e)) => {
                        warn!(
                            "Certificate {}: OCSP fetch failed (strict mode): {e}",
                            cert_file_name
                        );
                        let _ = self.store_rejected_cert(cert);
                        return Err(Error::new(
                            StatusCode::BadCertificateRevoked,
                            format!("Certificate {} OCSP check failed: {e}", cert_file_name),
                        ));
                    }
                    (_, Ok(Some(ocsp_der))) => {
                        fetched_ocsp.push(ocsp_der);
                    }
                    _ => {}
                }
            }
        }

        let context = ChainValidationContext {
            trusted_certs: &trusted,
            issuer_certs: &issuers,
            crls: &crls,
            ocsp_responses: &fetched_ocsp,
            security_policy,
            purpose,
            options: &options,
            now: &now,
        };
        match validate_certificate_chain(cert, &context) {
            Err(e) => {
                let _ = self.store_rejected_cert(cert);
                Err(e)
            }
            Ok(findings) => Ok(findings),
        }
    }

    fn fetch_ocsp_for_cert(
        &self,
        cert: &X509,
        config: &OcspFetchConfig,
        trusted: &[X509],
        issuers: &[X509],
        now: &chrono::DateTime<chrono::Utc>,
    ) -> Result<Option<Vec<u8>>, OcspError> {
        let issuer = find_issuer_for_cert(cert, trusted, issuers).ok_or(OcspError::NoIssuerCert)?;
        let url = ocsp::aia::extract_ocsp_url(cert)?;

        let cache_key = self.build_cache_key(cert, issuer);

        {
            let mut cache = self.ocsp_cache.lock();
            if let Some(cached_der) = cache.get(&cache_key) {
                return Ok(Some(cached_der));
            }
        }

        let request_der = ocsp::codec::build_ocsp_request(cert, issuer)?;
        let response_der = ocsp::fetch::fetch_ocsp_response(&url, &request_der, config)?;

        let issuer_pk = issuer
            .public_key()
            .map_err(|_| OcspError::InvalidResponse("issuer has no public key".into()))?;

        let _verdict = ocsp::validate::validate_ocsp_response(
            &response_der,
            &cert.serial_number(),
            &issuer_pk,
            now,
        )?;

        let next_update = compute_next_update(&response_der, now);
        {
            let mut cache = self.ocsp_cache.lock();
            cache.insert(cache_key, response_der.clone(), next_update);
        }

        Ok(Some(response_der))
    }

    fn build_cache_key(&self, cert: &X509, issuer: &X509) -> CacheKey {
        use sha1::Digest;

        let issuer_name_der = issuer
            .inner()
            .tbs_certificate
            .subject
            .to_der()
            .unwrap_or_default();
        let issuer_name_hash = sha1::Sha1::digest(&issuer_name_der).to_vec();

        let issuer_key_hash = sha1::Sha1::digest(
            issuer
                .inner()
                .tbs_certificate
                .subject_public_key_info
                .subject_public_key
                .raw_bytes(),
        )
        .to_vec();

        (issuer_name_hash, issuer_key_hash, cert.serial_number())
    }

    fn ensure_trusted_certs_dir_available(
        &self,
        cert: &X509,
        cert_file_name: &str,
        kind: IncomingCertificateKind,
    ) -> Result<(), Error> {
        let trusted_dir = self.trusted_certs_dir();
        if let Err(err) = read_dir(&trusted_dir) {
            let thumbprint = cert.thumbprint().as_hex_string();
            error!(
                "Certificate {} cannot be trusted because trusted certificate directory {} is unavailable: {}",
                cert_file_name,
                trusted_dir.display(),
                err
            );
            return Err(Error::new(
                kind.trust_list_status(),
                format!(
                    "Certificate {} ({}) cannot be trusted because trusted certificate directory {} is unavailable: {}",
                    cert_file_name,
                    thumbprint,
                    trusted_dir.display(),
                    err
                ),
            ));
        }

        Ok(())
    }

    fn read_trusted_certs_for_validation(
        &self,
        cert: &X509,
        cert_file_name: &str,
        kind: IncomingCertificateKind,
    ) -> Result<Vec<X509>, Error> {
        let trusted_dir = self.trusted_certs_dir();
        CertificateStore::read_cert_dir_strict(&trusted_dir).map_err(|err| {
            let thumbprint = cert.thumbprint().as_hex_string();
            error!(
                "Certificate {} cannot be trusted because trusted certificate storage {} is invalid: {}",
                cert_file_name,
                trusted_dir.display(),
                err
            );
            Error::new(
                kind.trust_list_status(),
                format!(
                    "Certificate {} ({}) cannot be trusted because trusted certificate storage {} is invalid: {}",
                    cert_file_name,
                    thumbprint,
                    trusted_dir.display(),
                    err
                ),
            )
        })
    }

    /// Returns a certificate file name from the cert's issuer and thumbprint fields.
    /// File name is either "prefix - \[thumbprint\].der" or "thumbprint.der" depending on
    /// the cert's common name being empty or not
    pub fn cert_file_name(cert: &X509) -> String {
        let prefix = if let Ok(common_name) = cert.common_name() {
            common_name.trim().to_string().replace('/', "")
        } else {
            String::new()
        };
        let thumbprint = cert.thumbprint().as_hex_string();

        if !prefix.is_empty() {
            format!("{prefix} [{thumbprint}].der")
        } else {
            format!("{thumbprint}.der")
        }
    }

    /// Creates the PKI directory structure
    ///
    /// # Errors
    ///
    /// A string description of any failure
    ///
    pub fn ensure_pki_path(&self) -> Result<(), String> {
        let mut rejected_path = self.pki_path.clone();
        rejected_path.push(REJECTED_CERTS_DIR);
        CertificateStore::ensure_dir(&rejected_path)?;

        let trust_list_subdirs = [
            TRUSTED_CERTS_DIR,
            ISSUER_CERTS_DIR,
            TRUSTED_CRLS_DIR,
            ISSUER_CRLS_DIR,
        ];
        for certificate_group in CertificateGroup::ALL {
            let mut group_path = self.certificate_group_dir(certificate_group);
            for subdir in &trust_list_subdirs {
                group_path.push(subdir);
                CertificateStore::ensure_dir(&group_path)?;
                group_path.pop();
            }
        }
        Ok(())
    }

    /// Ensure the directory exists, creating it if necessary
    ///
    /// # Errors
    ///
    /// A string description of any failure
    ///
    fn ensure_dir(path: &Path) -> Result<(), String> {
        if path.exists() {
            if !path.is_dir() {
                Err(format!("{} is not a directory ", path.display()))
            } else {
                Ok(())
            }
        } else {
            std::fs::create_dir_all(path)
                .map_err(|_| format!("Cannot make directories for {}", path.display()))
        }
    }

    /// Get path to application instance certificate
    pub fn own_certificate_path(&self) -> PathBuf {
        let mut path = PathBuf::from(&self.pki_path);
        path.push(&self.own_certificate_path);
        path
    }

    /// Get path to application instance private key
    pub fn own_private_key_path(&self) -> PathBuf {
        let mut path = PathBuf::from(&self.pki_path);
        path.push(&self.own_private_key_path);
        path
    }

    /// Get the path to the rejected certs dir
    pub fn rejected_certs_dir(&self) -> PathBuf {
        let mut path = PathBuf::from(&self.pki_path);
        path.push(REJECTED_CERTS_DIR);
        path
    }

    fn certificate_group_dir(&self, certificate_group: CertificateGroup) -> PathBuf {
        let mut path = self.pki_path.clone();
        if let Some(directory_name) = certificate_group.directory_name() {
            path.push(directory_name);
        }
        path
    }

    /// Get the path to the trusted certs dir
    pub fn trusted_certs_dir(&self) -> PathBuf {
        self.trusted_certs_dir_for_group(CertificateGroup::DefaultApplication)
    }

    /// Get the path to the trusted certificates for `certificate_group`.
    pub fn trusted_certs_dir_for_group(&self, certificate_group: CertificateGroup) -> PathBuf {
        let mut path = self.certificate_group_dir(certificate_group);
        path.push(TRUSTED_CERTS_DIR);
        path
    }

    /// Get the path to the issuer certs dir
    pub fn issuer_certs_dir(&self) -> PathBuf {
        self.issuer_certs_dir_for_group(CertificateGroup::DefaultApplication)
    }

    /// Get the path to the issuer certificates for `certificate_group`.
    pub fn issuer_certs_dir_for_group(&self, certificate_group: CertificateGroup) -> PathBuf {
        let mut path = self.certificate_group_dir(certificate_group);
        path.push(ISSUER_CERTS_DIR);
        path
    }

    /// Get the path to the trusted CRLs dir
    pub fn trusted_crls_dir(&self) -> PathBuf {
        self.trusted_crls_dir_for_group(CertificateGroup::DefaultApplication)
    }

    /// Get the path to trusted-certificate CRLs for `certificate_group`.
    pub fn trusted_crls_dir_for_group(&self, certificate_group: CertificateGroup) -> PathBuf {
        let mut path = self.certificate_group_dir(certificate_group);
        path.push(TRUSTED_CRLS_DIR);
        path
    }

    /// Get the path to the issuer CRLs dir
    pub fn issuer_crls_dir(&self) -> PathBuf {
        self.issuer_crls_dir_for_group(CertificateGroup::DefaultApplication)
    }

    /// Get the path to issuer-certificate CRLs for `certificate_group`.
    pub fn issuer_crls_dir_for_group(&self, certificate_group: CertificateGroup) -> PathBuf {
        let mut path = self.certificate_group_dir(certificate_group);
        path.push(ISSUER_CRLS_DIR);
        path
    }

    /// Read all trusted certificates from the store.
    pub fn read_trusted_certs(&self) -> Vec<X509> {
        self.read_trusted_certs_for_group(CertificateGroup::DefaultApplication)
    }

    /// Read all trusted certificates for `certificate_group`.
    pub fn read_trusted_certs_for_group(&self, certificate_group: CertificateGroup) -> Vec<X509> {
        CertificateStore::read_cert_dir(&self.trusted_certs_dir_for_group(certificate_group))
    }

    /// Read all issuer certificates from the store.
    pub fn read_issuer_certs(&self) -> Vec<X509> {
        self.read_issuer_certs_for_group(CertificateGroup::DefaultApplication)
    }

    /// Read all issuer certificates for `certificate_group`.
    pub fn read_issuer_certs_for_group(&self, certificate_group: CertificateGroup) -> Vec<X509> {
        CertificateStore::read_cert_dir(&self.issuer_certs_dir_for_group(certificate_group))
    }

    /// Read all rejected certificates from the store (OPC UA Part 12 §7.10.12 GetRejectedList).
    pub fn read_rejected_certs(&self) -> Vec<X509> {
        CertificateStore::read_cert_dir(&self.rejected_certs_dir())
    }

    /// Read all trusted CRLs from the store.
    pub fn read_trusted_crls(&self) -> Vec<CertificateList> {
        self.read_trusted_crls_for_group(CertificateGroup::DefaultApplication)
    }

    fn read_trusted_crls_for_group(
        &self,
        certificate_group: CertificateGroup,
    ) -> Vec<CertificateList> {
        CertificateStore::read_crl_dir(&self.trusted_crls_dir_for_group(certificate_group))
    }

    /// Read all issuer CRLs from the store.
    pub fn read_issuer_crls(&self) -> Vec<CertificateList> {
        self.read_issuer_crls_for_group(CertificateGroup::DefaultApplication)
    }

    fn read_issuer_crls_for_group(
        &self,
        certificate_group: CertificateGroup,
    ) -> Vec<CertificateList> {
        CertificateStore::read_crl_dir(&self.issuer_crls_dir_for_group(certificate_group))
    }

    /// Read all trusted CRLs from the store as raw DER bytes (avoids exposing the `x509_cert`
    /// crate's `CertificateList` type across the crate boundary).
    pub fn read_trusted_crls_der(&self) -> Vec<Vec<u8>> {
        self.read_trusted_crls_der_for_group(CertificateGroup::DefaultApplication)
    }

    /// Read all trusted CRLs for `certificate_group` as raw DER bytes.
    pub fn read_trusted_crls_der_for_group(
        &self,
        certificate_group: CertificateGroup,
    ) -> Vec<Vec<u8>> {
        Self::crls_to_der(self.read_trusted_crls_for_group(certificate_group))
    }

    /// Read all issuer CRLs from the store as raw DER bytes.
    pub fn read_issuer_crls_der(&self) -> Vec<Vec<u8>> {
        self.read_issuer_crls_der_for_group(CertificateGroup::DefaultApplication)
    }

    /// Read all issuer CRLs for `certificate_group` as raw DER bytes.
    pub fn read_issuer_crls_der_for_group(
        &self,
        certificate_group: CertificateGroup,
    ) -> Vec<Vec<u8>> {
        Self::crls_to_der(self.read_issuer_crls_for_group(certificate_group))
    }

    fn crls_to_der(crls: Vec<CertificateList>) -> Vec<Vec<u8>> {
        crls.iter().filter_map(|crl| crl.to_der().ok()).collect()
    }

    /// Write a cert to the rejected directory. If the write succeeds, the function
    /// returns a path to the written file.
    ///
    /// # Errors
    ///
    /// A string description of any failure
    ///
    pub fn store_rejected_cert(&self, cert: &X509) -> Result<PathBuf, String> {
        // Store the cert in the rejected folder where untrusted certs go
        let cert_file_name = CertificateStore::cert_file_name(cert);
        let mut cert_path = self.rejected_certs_dir();
        cert_path.push(&cert_file_name);
        let _ = CertificateStore::store_cert(cert, &cert_path, true)?;
        Ok(cert_path)
    }

    /// Writes a cert to the trusted directory. If the write succeeds, the function
    /// returns a path to the written file.
    ///
    /// # Errors
    ///
    /// A string description of any failure
    ///
    pub fn store_trusted_cert(&self, cert: &X509) -> Result<PathBuf, String> {
        self.store_trusted_cert_for_group(CertificateGroup::DefaultApplication, cert)
    }

    /// Writes a trusted certificate to `certificate_group`'s TrustList.
    ///
    /// # Errors
    ///
    /// Returns a description if the certificate cannot be persisted.
    pub fn store_trusted_cert_for_group(
        &self,
        certificate_group: CertificateGroup,
        cert: &X509,
    ) -> Result<PathBuf, String> {
        self.store_cert_in_dir(cert, &self.trusted_certs_dir_for_group(certificate_group))
    }

    /// Writes a cert to the issuer directory. If the write succeeds, the function
    /// returns a path to the written file.
    ///
    /// # Errors
    ///
    /// A string description of any failure
    ///
    pub fn store_issuer_cert(&self, cert: &X509) -> Result<PathBuf, String> {
        self.store_issuer_cert_for_group(CertificateGroup::DefaultApplication, cert)
    }

    /// Writes an issuer certificate to `certificate_group`'s TrustList.
    ///
    /// # Errors
    ///
    /// Returns a description if the certificate cannot be persisted.
    pub fn store_issuer_cert_for_group(
        &self,
        certificate_group: CertificateGroup,
        cert: &X509,
    ) -> Result<PathBuf, String> {
        self.store_cert_in_dir(cert, &self.issuer_certs_dir_for_group(certificate_group))
    }

    fn store_cert_in_dir(&self, cert: &X509, dir: &Path) -> Result<PathBuf, String> {
        let mut cert_path = dir.to_path_buf();
        cert_path.push(CertificateStore::cert_file_name(cert));
        let _ = CertificateStore::store_cert(cert, &cert_path, true)?;
        Ok(cert_path)
    }

    /// Removes a certificate matching `thumbprint` from the trusted directory, along with any
    /// CRLs whose issuer matches the removed certificate's subject (OPC UA Part 12 §7.8.2.7:
    /// "If the Certificate is a CA Certificate that has CRLs then all CRLs for that CA are
    /// removed as well"). Returns whether a matching certificate was found and removed.
    ///
    /// # Errors
    ///
    /// A string description of any failure
    ///
    pub fn remove_trusted_cert(&self, thumbprint: &crate::Thumbprint) -> Result<bool, String> {
        self.remove_trusted_cert_for_group(CertificateGroup::DefaultApplication, thumbprint)
    }

    /// Removes a trusted certificate and its associated CRLs from `certificate_group`.
    ///
    /// # Errors
    ///
    /// Returns a description if a matching certificate cannot be removed.
    pub fn remove_trusted_cert_for_group(
        &self,
        certificate_group: CertificateGroup,
        thumbprint: &crate::Thumbprint,
    ) -> Result<bool, String> {
        self.remove_cert_and_crls(
            &self.trusted_certs_dir_for_group(certificate_group),
            &self.trusted_crls_dir_for_group(certificate_group),
            thumbprint,
        )
    }

    /// Removes a certificate matching `thumbprint` from the issuer directory, along with any
    /// CRLs whose issuer matches the removed certificate's subject.
    ///
    /// # Errors
    ///
    /// A string description of any failure
    ///
    pub fn remove_issuer_cert(&self, thumbprint: &crate::Thumbprint) -> Result<bool, String> {
        self.remove_issuer_cert_for_group(CertificateGroup::DefaultApplication, thumbprint)
    }

    /// Removes an issuer certificate and its associated CRLs from `certificate_group`.
    ///
    /// # Errors
    ///
    /// Returns a description if a matching certificate cannot be removed.
    pub fn remove_issuer_cert_for_group(
        &self,
        certificate_group: CertificateGroup,
        thumbprint: &crate::Thumbprint,
    ) -> Result<bool, String> {
        self.remove_cert_and_crls(
            &self.issuer_certs_dir_for_group(certificate_group),
            &self.issuer_crls_dir_for_group(certificate_group),
            thumbprint,
        )
    }

    fn remove_cert_and_crls(
        &self,
        certs_dir: &Path,
        crls_dir: &Path,
        thumbprint: &crate::Thumbprint,
    ) -> Result<bool, String> {
        let Some((cert, cert_path)) = Self::find_cert_by_thumbprint(certs_dir, thumbprint) else {
            return Ok(false);
        };

        std::fs::remove_file(&cert_path).map_err(|e| {
            format!(
                "Could not remove certificate file {}: {e}",
                cert_path.display()
            )
        })?;

        let subject_name = cert.subject_name();
        for (crl, crl_path) in Self::crls_with_paths(crls_dir) {
            let issuer_name = crl.tbs_cert_list.issuer.to_string().replace(';', "/");
            if issuer_name == subject_name {
                let _ = std::fs::remove_file(&crl_path);
            }
        }

        Ok(true)
    }

    fn find_cert_by_thumbprint(
        dir: &Path,
        thumbprint: &crate::Thumbprint,
    ) -> Option<(X509, PathBuf)> {
        let entries = read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if !CertificateStore::is_der_or_pem_file(&path) {
                continue;
            }
            if let Ok(cert) = CertificateStore::read_cert(&path) {
                if cert.thumbprint() == *thumbprint {
                    return Some((cert, path));
                }
            }
        }
        None
    }

    fn crls_with_paths(dir: &Path) -> Vec<(CertificateList, PathBuf)> {
        let Ok(entries) = read_dir(dir) else {
            return Vec::new();
        };
        let mut crls = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !CertificateStore::is_der_or_pem_file(&path) {
                continue;
            }
            if let Ok(crl) = CertificateStore::read_crl(&path) {
                crls.push((crl, path));
            }
        }
        crls
    }

    /// Validates `der` as a well-formed CRL and writes it to the trusted CRLs directory, named
    /// by a unique, non-content-derived file name. If the write succeeds, returns a path to the
    /// written file. Takes raw DER bytes (rather than a parsed `CertificateList`) so callers
    /// outside this crate don't need `x509_cert` as a direct dependency.
    ///
    /// # Errors
    ///
    /// A string description of any failure
    ///
    pub fn store_trusted_crl(&self, der: &[u8]) -> Result<PathBuf, String> {
        self.store_trusted_crl_for_group(CertificateGroup::DefaultApplication, der)
    }

    /// Writes a trusted CRL to `certificate_group`'s TrustList.
    ///
    /// # Errors
    ///
    /// Returns a description if `der` is invalid or cannot be persisted.
    pub fn store_trusted_crl_for_group(
        &self,
        certificate_group: CertificateGroup,
        der: &[u8],
    ) -> Result<PathBuf, String> {
        Self::store_crl_der(der, &self.trusted_crls_dir_for_group(certificate_group))
    }

    /// Validates `der` as a well-formed CRL and writes it to the issuer CRLs directory, named by
    /// a unique, non-content-derived file name. If the write succeeds, returns a path to the
    /// written file.
    ///
    /// # Errors
    ///
    /// A string description of any failure
    ///
    pub fn store_issuer_crl(&self, der: &[u8]) -> Result<PathBuf, String> {
        self.store_issuer_crl_for_group(CertificateGroup::DefaultApplication, der)
    }

    /// Writes an issuer CRL to `certificate_group`'s TrustList.
    ///
    /// # Errors
    ///
    /// Returns a description if `der` is invalid or cannot be persisted.
    pub fn store_issuer_crl_for_group(
        &self,
        certificate_group: CertificateGroup,
        der: &[u8],
    ) -> Result<PathBuf, String> {
        Self::store_crl_der(der, &self.issuer_crls_dir_for_group(certificate_group))
    }

    fn store_crl_der(der: &[u8], dir: &Path) -> Result<PathBuf, String> {
        CertificateList::from_der(der).map_err(|e| format!("Not a valid CRL: {e:?}"))?;
        // Named uniquely rather than content-addressed: this is a filesystem detail with no
        // security property to uphold, and a cryptographic hash here would be pure overhead (and
        // trips static analysis for "insecure hashing" if a weak hash such as SHA-1 is used).
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let file_name = format!("{nanos:x}-{}-{id:x}", std::process::id());
        let mut path = PathBuf::from(dir);
        path.push(format!("{file_name}.der"));
        CertificateStore::write_to_file(der, &path, true)?;
        Ok(path)
    }

    /// Replaces the entire trusted-certificates list with `certs_der` (OPC UA Part 12 §7.8.2.5
    /// `CloseAndUpdate`: a set bit in the uploaded TrustList's mask means that whole list is
    /// replaced, not merged). Existing files are removed first; if any new certificate fails to
    /// parse, the directory is left cleared and an error is returned (the caller -- `CloseAndUpdate`
    /// -- is expected to have already validated every certificate before calling this).
    ///
    /// # Errors
    ///
    /// A string description of any failure
    ///
    pub fn replace_trusted_certs(&self, certs_der: &[Vec<u8>]) -> Result<(), String> {
        self.replace_trusted_certs_for_group(CertificateGroup::DefaultApplication, certs_der)
    }

    /// Replaces the trusted-certificate list for `certificate_group`.
    ///
    /// # Errors
    ///
    /// Returns a description if a supplied certificate is invalid or cannot be persisted.
    pub fn replace_trusted_certs_for_group(
        &self,
        certificate_group: CertificateGroup,
        certs_der: &[Vec<u8>],
    ) -> Result<(), String> {
        Self::replace_list(
            &self.trusted_certs_dir_for_group(certificate_group),
            certs_der,
            |der| {
                let cert =
                    X509::from_der(der).map_err(|e| format!("Not a valid certificate: {e}"))?;
                self.store_trusted_cert_for_group(certificate_group, &cert)
                    .map(|_| ())
            },
        )
    }

    /// Replaces the entire issuer-certificates list with `certs_der`. See
    /// [`CertificateStore::replace_trusted_certs`].
    ///
    /// # Errors
    ///
    /// A string description of any failure
    ///
    pub fn replace_issuer_certs(&self, certs_der: &[Vec<u8>]) -> Result<(), String> {
        self.replace_issuer_certs_for_group(CertificateGroup::DefaultApplication, certs_der)
    }

    /// Replaces the issuer-certificate list for `certificate_group`.
    ///
    /// # Errors
    ///
    /// Returns a description if a supplied certificate is invalid or cannot be persisted.
    pub fn replace_issuer_certs_for_group(
        &self,
        certificate_group: CertificateGroup,
        certs_der: &[Vec<u8>],
    ) -> Result<(), String> {
        Self::replace_list(
            &self.issuer_certs_dir_for_group(certificate_group),
            certs_der,
            |der| {
                let cert =
                    X509::from_der(der).map_err(|e| format!("Not a valid certificate: {e}"))?;
                self.store_issuer_cert_for_group(certificate_group, &cert)
                    .map(|_| ())
            },
        )
    }

    /// Replaces the entire trusted-CRLs list with `crls_der`. See
    /// [`CertificateStore::replace_trusted_certs`].
    ///
    /// # Errors
    ///
    /// A string description of any failure
    ///
    pub fn replace_trusted_crls(&self, crls_der: &[Vec<u8>]) -> Result<(), String> {
        self.replace_trusted_crls_for_group(CertificateGroup::DefaultApplication, crls_der)
    }

    /// Replaces the trusted-CRL list for `certificate_group`.
    ///
    /// # Errors
    ///
    /// Returns a description if a supplied CRL is invalid or cannot be persisted.
    pub fn replace_trusted_crls_for_group(
        &self,
        certificate_group: CertificateGroup,
        crls_der: &[Vec<u8>],
    ) -> Result<(), String> {
        Self::replace_list(
            &self.trusted_crls_dir_for_group(certificate_group),
            crls_der,
            |der| {
                self.store_trusted_crl_for_group(certificate_group, der)
                    .map(|_| ())
            },
        )
    }

    /// Replaces the entire issuer-CRLs list with `crls_der`. See
    /// [`CertificateStore::replace_trusted_certs`].
    ///
    /// # Errors
    ///
    /// A string description of any failure
    ///
    pub fn replace_issuer_crls(&self, crls_der: &[Vec<u8>]) -> Result<(), String> {
        self.replace_issuer_crls_for_group(CertificateGroup::DefaultApplication, crls_der)
    }

    /// Replaces the issuer-CRL list for `certificate_group`.
    ///
    /// # Errors
    ///
    /// Returns a description if a supplied CRL is invalid or cannot be persisted.
    pub fn replace_issuer_crls_for_group(
        &self,
        certificate_group: CertificateGroup,
        crls_der: &[Vec<u8>],
    ) -> Result<(), String> {
        Self::replace_list(
            &self.issuer_crls_dir_for_group(certificate_group),
            crls_der,
            |der| {
                self.store_issuer_crl_for_group(certificate_group, der)
                    .map(|_| ())
            },
        )
    }

    /// Clears `dir` and re-populates it by calling `store_one` for each item in `items_der`,
    /// sharing the "clear then repopulate" shape common to every `replace_*` method above.
    fn replace_list(
        dir: &Path,
        items_der: &[Vec<u8>],
        mut store_one: impl FnMut(&[u8]) -> Result<(), String>,
    ) -> Result<(), String> {
        Self::clear_dir(dir);
        for der in items_der {
            store_one(der)?;
        }
        Ok(())
    }

    fn clear_dir(dir: &Path) {
        let Ok(entries) = read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if CertificateStore::is_der_or_pem_file(&path) {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    /// Writes a cert to the specified directory
    ///
    /// # Errors
    ///
    /// A string description of any failure
    ///
    fn store_cert(cert: &X509, path: &Path, overwrite: bool) -> Result<usize, String> {
        let der = cert
            .to_der()
            .map_err(|e| format!("Could not encode X509 cert as DER: {e:?}"))?;
        info!("Writing X509 cert to {}", path.display());
        CertificateStore::write_to_file(&der, path, overwrite)
    }

    /// Reads an X509 certificate in .def or .pem format from disk
    ///
    /// # Errors
    ///
    /// A string description of any failure
    ///
    pub fn read_cert(path: &Path) -> Result<X509, String> {
        let file = File::open(path);
        if file.is_err() {
            return Err(format!("Could not open cert file {}", path.display()));
        }

        let mut file: File =
            file.map_err(|_| format!("Could not open cert file {}", path.display()))?;
        let mut cert = Vec::new();
        let bytes_read = file.read_to_end(&mut cert);
        if bytes_read.is_err() {
            return Err(format!(
                "Could not read bytes from cert file {}",
                path.display()
            ));
        }

        let cert = match path.extension() {
            Some(v) if v == "der" => X509::from_der(&cert),
            Some(v) if v == "pem" => X509::from_pem(&cert),
            _ => return Err("Only .der and .pem certificates are supported".to_string()),
        };

        match cert {
            Err(_) => Err(format!(
                "Could not read cert from cert file {}",
                path.display()
            )),
            Ok(val) => Ok(val),
        }
    }

    fn read_cert_dir(path: &Path) -> Vec<X509> {
        let entries = match read_dir(path) {
            Ok(entries) => entries,
            Err(err) => {
                trace!(
                    "Cannot read certificate directory {}: {}",
                    path.display(),
                    err
                );
                return Vec::new();
            }
        };

        let mut certs = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    trace!(
                        "Cannot read certificate directory entry from {}: {}",
                        path.display(),
                        err
                    );
                    continue;
                }
            };
            let cert_path = entry.path();
            if !CertificateStore::is_der_or_pem_file(&cert_path) {
                continue;
            }

            match CertificateStore::read_cert(&cert_path) {
                Ok(cert) => certs.push(cert),
                Err(err) => {
                    trace!("Cannot read certificate {}: {}", cert_path.display(), err);
                }
            }
        }
        certs
    }

    fn read_cert_dir_strict(path: &Path) -> Result<Vec<X509>, String> {
        let entries = read_dir(path).map_err(|err| {
            format!(
                "Cannot read certificate directory {}: {}",
                path.display(),
                err
            )
        })?;

        let mut certs = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|err| {
                format!(
                    "Cannot read certificate directory entry from {}: {}",
                    path.display(),
                    err
                )
            })?;
            let cert_path = entry.path();
            if !CertificateStore::is_der_or_pem_file(&cert_path) {
                continue;
            }

            let cert = CertificateStore::read_cert(&cert_path).map_err(|err| {
                format!("Cannot read certificate {}: {}", cert_path.display(), err)
            })?;
            certs.push(cert);
        }

        Ok(certs)
    }

    fn read_crl_dir(path: &Path) -> Vec<CertificateList> {
        let entries = match read_dir(path) {
            Ok(entries) => entries,
            Err(err) => {
                trace!("Cannot read CRL directory {}: {}", path.display(), err);
                return Vec::new();
            }
        };

        let mut crls = Vec::new();
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    trace!(
                        "Cannot read CRL directory entry from {}: {}",
                        path.display(),
                        err
                    );
                    continue;
                }
            };
            let crl_path = entry.path();
            if !CertificateStore::is_der_or_pem_file(&crl_path) {
                continue;
            }

            match CertificateStore::read_crl(&crl_path) {
                Ok(crl) => crls.push(crl),
                Err(err) => {
                    trace!("Cannot read CRL {}: {}", crl_path.display(), err);
                }
            }
        }
        crls
    }

    fn read_crl(path: &Path) -> Result<CertificateList, String> {
        let crl = std::fs::read(path)
            .map_err(|_| format!("Could not read bytes from CRL file {}", path.display()))?;

        match path.extension().and_then(|extension| extension.to_str()) {
            Some("der") => CertificateList::from_der(&crl),
            Some("pem") => CertificateStore::read_pem_crl(&crl),
            _ => return Err("Only .der and .pem CRLs are supported".to_string()),
        }
        .map_err(|_| format!("Could not read CRL from CRL file {}", path.display()))
    }

    fn read_pem_crl(crl: &[u8]) -> Result<CertificateList, x509_cert::der::Error> {
        let mut reader = x509_cert::der::PemReader::new(crl)?;
        let crl = CertificateList::decode(&mut reader)?;
        reader.finish(crl)
    }

    fn is_der_or_pem_file(path: &Path) -> bool {
        path.is_file()
            && matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("der" | "pem")
            )
    }

    /// Writes bytes to file and returns the size written, or an error reason for failure.
    ///
    /// # Errors
    ///
    /// A string description of any failure
    ///
    fn write_to_file(bytes: &[u8], file_path: &Path, overwrite: bool) -> Result<usize, String> {
        if !overwrite && file_path.exists() {
            Err(format!(
                "File {} already exists and will not be overwritten. Enable overwrite to disable this safeguard.",
                file_path.display()
            ))
        } else {
            if let Some(parent) = file_path.parent() {
                CertificateStore::ensure_dir(parent)?;
            }
            match File::create(file_path) {
                Ok(mut file) => file
                    .write(bytes)
                    .map_err(|_| format!("Could not write bytes to file {}", file_path.display())),
                Err(_) => Err(format!("Could not create file {}", file_path.display())),
            }
        }
    }

    fn write_private_key_to_file(
        bytes: &[u8],
        file_path: &Path,
        overwrite: bool,
    ) -> Result<usize, String> {
        if !overwrite && file_path.exists() {
            Err(format!(
                "File {} already exists and will not be overwritten. Enable overwrite to disable this safeguard.",
                file_path.display()
            ))
        } else {
            if let Some(parent) = file_path.parent() {
                CertificateStore::ensure_dir(parent)?;
            }
            let mut options = OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            options.mode(0o600);
            match options.open(file_path) {
                Ok(mut file) => file
                    .write(bytes)
                    .map_err(|_| format!("Could not write bytes to file {}", file_path.display())),
                Err(_) => Err(format!("Could not create file {}", file_path.display())),
            }
        }
    }
}

fn find_issuer_for_cert<'a>(
    cert: &X509,
    trusted: &'a [X509],
    issuers: &'a [X509],
) -> Option<&'a X509> {
    issuers
        .iter()
        .chain(trusted.iter())
        .find(|candidate| candidate.subject_name() == cert.issuer_name())
}

fn compute_next_update(response_der: &[u8], now: &chrono::DateTime<chrono::Utc>) -> SystemTime {
    use x509_ocsp::{BasicOcspResponse, OcspResponse, OcspResponseStatus};

    let now_sys: SystemTime = (*now).into();

    let response = match OcspResponse::from_der(response_der) {
        Ok(r) => r,
        Err(_) => return fallback_next_update(now_sys),
    };

    if response.response_status != OcspResponseStatus::Successful {
        return fallback_next_update(now_sys);
    }

    let Some(response_bytes) = response.response_bytes else {
        return fallback_next_update(now_sys);
    };

    let basic = match BasicOcspResponse::from_der(response_bytes.response.as_bytes()) {
        Ok(b) => b,
        Err(_) => return fallback_next_update(now_sys),
    };

    basic
        .tbs_response_data
        .responses
        .iter()
        .filter_map(|single| single.next_update.as_ref())
        .map(|next_update| next_update.0.to_system_time())
        .filter(|next| *next > now_sys)
        .max()
        .unwrap_or_else(|| fallback_next_update(now_sys))
}

fn fallback_next_update(now_sys: SystemTime) -> SystemTime {
    now_sys + Duration::from_secs(300)
}
