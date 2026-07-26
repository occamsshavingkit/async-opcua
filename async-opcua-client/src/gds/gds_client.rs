// OPCUA for Rust
// SPDX-License-Identifier: MPL-2.0
// Copyright (C) 2017-2026 Adam Lock

//! Unified Global Discovery Server client helpers.

use super::{csr::GdsCsrClient, registration::GdsRegistrationClient};
use crate::Session;
use opcua_core::sync::RwLock;
use opcua_crypto::{gds_reload, CertificateStore};
use opcua_types::{
    ApplicationRecordDataType, BrowsePath, BrowsePathResult, Error, GdsApplicationRecordTypeLoader,
    NodeId, ObjectId, QualifiedName, ReferenceTypeId, RelativePath, RelativePathElement,
    StatusCode,
};
use std::{fs, sync::Arc};

/// The GDS companion namespace URI (OPC-10000-12), declared in the companion NodeSet's own
/// `<NamespaceUris>`.
const GDS_NAMESPACE_URI: &str = "http://opcfoundation.org/UA/GDS/";

/// Facade over the GDS registration and certificate signing helpers.
pub struct GdsClient {
    registration: GdsRegistrationClient,
    csr: GdsCsrClient,
}

impl GdsClient {
    /// Discovers the real GDS Directory object and its `RegisterApplication`/
    /// `StartSigningRequest`/`FinishRequest` method NodeIds against `session`, via the target
    /// server's own namespace array and `TranslateBrowsePathsToNodeIds` (OPC-10000-4 §5.8.4) --
    /// the standard mechanism for resolving a well-known node whose namespace index isn't known
    /// in advance. Every real GDS deployment assigns its own index to the GDS companion
    /// namespace, so there is no fixed NodeId this could ever use instead.
    ///
    /// Fails closed (a specific [`Error`], never a panic) if the server doesn't expose the GDS
    /// companion namespace, or if any expected node is missing.
    ///
    /// Discovery is one-shot: it runs once here and the resolved NodeIds are held by the
    /// returned `GdsClient` for its lifetime. There is no internal re-discovery path -- call this
    /// again (e.g. after reconnecting to a different server) if you need a fresh resolution.
    pub async fn discover(session: &Session) -> Result<Self, Error> {
        let gds_ns = session.get_namespace_index(GDS_NAMESPACE_URI).await?;
        session.add_type_loader(Arc::new(GdsApplicationRecordTypeLoader));

        let browse_paths = vec![
            directory_browse_path(gds_ns, &["Directory"]),
            directory_browse_path(gds_ns, &["Directory", "RegisterApplication"]),
            directory_browse_path(gds_ns, &["Directory", "StartSigningRequest"]),
            directory_browse_path(gds_ns, &["Directory", "FinishRequest"]),
        ];
        let results = session
            .translate_browse_paths_to_node_ids(&browse_paths)
            .await?;

        let directory_object_id = resolve_target(&results, 0, "Directory")?;
        let register_method_id = resolve_target(&results, 1, "RegisterApplication")?;
        let start_signing_request_id = resolve_target(&results, 2, "StartSigningRequest")?;
        let finish_request_id = resolve_target(&results, 3, "FinishRequest")?;

        Ok(Self {
            registration: GdsRegistrationClient::new(
                directory_object_id.clone(),
                register_method_id,
            ),
            csr: GdsCsrClient::new(
                directory_object_id,
                start_signing_request_id,
                finish_request_id,
            ),
        })
    }

    /// Creates a GDS client from explicit helper clients (e.g. NodeIds already known or
    /// discovered out-of-band). Prefer [`GdsClient::discover`] when connected to a live session.
    pub fn from_parts(registration: GdsRegistrationClient, csr: GdsCsrClient) -> Self {
        Self { registration, csr }
    }

    /// Returns the wrapped registration helper.
    pub fn registration(&self) -> &GdsRegistrationClient {
        &self.registration
    }

    /// Returns the wrapped CSR helper.
    pub fn csr(&self) -> &GdsCsrClient {
        &self.csr
    }

    /// Registers an application with the GDS directory service.
    pub async fn register_application(
        &self,
        session: &Session,
        application_record: ApplicationRecordDataType,
    ) -> Result<NodeId, StatusCode> {
        self.registration
            .register_application(session, application_record)
            .await
    }

    /// Submits a DER-encoded CSR to the GDS and returns the signing request id.
    pub async fn request_signing_csr(
        &self,
        session: &Session,
        application_id: NodeId,
        certificate_group_id: NodeId,
        certificate_type_id: NodeId,
        csr_der: &[u8],
    ) -> Result<NodeId, StatusCode> {
        self.csr
            .start_signing_request(
                session,
                application_id,
                certificate_group_id,
                certificate_type_id,
                csr_der,
            )
            .await
    }

    /// Polls the GDS for a completed signing request.
    pub async fn poll_signing_request(
        &self,
        session: &Session,
        application_id: NodeId,
        request_id: NodeId,
    ) -> Result<(Vec<u8>, Option<Vec<u8>>), StatusCode> {
        self.csr
            .finish_signing_request(session, application_id, request_id)
            .await
    }

    /// Persists renewed certificate material and verifies the store can reload it.
    pub fn apply_renewed_certificate(
        &self,
        certificate_store: &RwLock<CertificateStore>,
        certificate_der: &[u8],
        private_key_pem: Option<&[u8]>,
    ) -> Result<(), StatusCode> {
        let store = certificate_store.read();

        match private_key_pem {
            Some(private_key_pem) => {
                gds_reload::save_new_credentials(&store, certificate_der, private_key_pem)
            }
            None => write_certificate(&store, certificate_der),
        }
        .map_err(|_| StatusCode::BadSecurityChecksFailed)?;

        gds_reload::reload_store_from_disk(&store)
            .map(|_| ())
            .map_err(|_| StatusCode::BadSecurityChecksFailed)
    }
}

/// Builds a `BrowsePath` from the standard `ObjectsFolder`, following `hops` by `BrowseName` in
/// the GDS companion namespace -- the first hop via `Organizes` (matching how the real Directory
/// object is organized under `ObjectsFolder`), every subsequent hop via `HasComponent` (matching
/// how the real Directory object's methods are wired to it).
fn directory_browse_path(gds_ns: u16, hops: &[&str]) -> BrowsePath {
    let elements = hops
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let reference_type_id = if i == 0 {
                ReferenceTypeId::Organizes
            } else {
                ReferenceTypeId::HasComponent
            };
            RelativePathElement {
                reference_type_id: NodeId::from(reference_type_id),
                is_inverse: false,
                include_subtypes: true,
                target_name: QualifiedName::new(gds_ns, *name),
            }
        })
        .collect();

    BrowsePath {
        starting_node: NodeId::from(ObjectId::ObjectsFolder),
        relative_path: RelativePath {
            elements: Some(elements),
        },
    }
}

/// Extracts the resolved `NodeId` for the `index`th browse path in `results`, failing closed with
/// a specific error naming `label` if that node wasn't found.
fn resolve_target(
    results: &[BrowsePathResult],
    index: usize,
    label: &str,
) -> Result<NodeId, Error> {
    let result = results.get(index).ok_or_else(|| {
        Error::new(
            StatusCode::BadUnexpectedError,
            format!("GDS discovery: missing TranslateBrowsePathsToNodeIds result for {label}"),
        )
    })?;
    if !result.status_code.is_good() {
        return Err(Error::new(
            result.status_code,
            format!(
                "GDS discovery: could not resolve {label} ({})",
                result.status_code
            ),
        ));
    }
    let target = result
        .targets
        .as_ref()
        .and_then(|targets| targets.first())
        .ok_or_else(|| {
            Error::new(
                StatusCode::BadNotFound,
                format!("GDS discovery: no targets resolved for {label}"),
            )
        })?;
    Ok(target.target_id.node_id.clone())
}

fn write_certificate(store: &CertificateStore, certificate_der: &[u8]) -> Result<(), String> {
    let cert_path = store.own_certificate_path();
    if let Some(parent) = cert_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create certificate parent dir: {e}"))?;
    }
    fs::write(&cert_path, certificate_der).map_err(|e| format!("failed to write certificate: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    struct TempPki {
        path: PathBuf,
    }

    impl TempPki {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("async-opcua-client-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create temporary pki dir");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempPki {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn client_with_dummy_node_ids() -> GdsClient {
        GdsClient::from_parts(
            GdsRegistrationClient::new(NodeId::new(1, "Directory"), NodeId::new(1, "Register")),
            GdsCsrClient::new(
                NodeId::new(1, "Directory"),
                NodeId::new(1, "StartSigningRequest"),
                NodeId::new(1, "FinishRequest"),
            ),
        )
    }

    #[test]
    fn apply_renewed_certificate_rejects_invalid_security_material() {
        let temp_pki = TempPki::new("gds-client-invalid-material");
        let store = RwLock::new(CertificateStore::new(temp_pki.path()));
        let client = client_with_dummy_node_ids();

        let result = client.apply_renewed_certificate(&store, b"not-a-certificate", None);

        assert_eq!(
            result,
            Err(opcua_types::StatusCode::BadSecurityChecksFailed)
        );
    }
}
