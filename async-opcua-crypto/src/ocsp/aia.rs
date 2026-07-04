use const_oid::db::rfc5280::ID_AD_OCSP;
use tracing::trace;
use x509_cert::ext::pkix::AuthorityInfoAccessSyntax;

use crate::x509::X509;

use super::config::OcspError;

pub fn extract_ocsp_url(cert: &X509) -> Result<String, OcspError> {
    let inner = cert.inner();
    let aia: Option<(bool, AuthorityInfoAccessSyntax)> = inner
        .tbs_certificate
        .get()
        .map_err(|_| OcspError::NoAiaExtension)?;
    let (_critical, aia) = aia.ok_or(OcspError::NoAiaExtension)?;

    for access_desc in &aia.0 {
        if access_desc.access_method == ID_AD_OCSP {
            if let x509_cert::ext::pkix::name::GeneralName::UniformResourceIdentifier(ref uri) =
                access_desc.access_location
            {
                let url = uri.to_string();
                trace!("Found OCSP responder URL in AIA: {url}");
                return Ok(url);
            }
        }
    }

    Err(OcspError::NoOcspUrl)
}
