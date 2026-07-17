// OPCUA for Rust
// SPDX-License-Identifier: MPL-2.0
// Copyright (C) 2017-2026 Adam Lock

//! Certificate Signing Request (CSR) GDS exchange client implementation.
//! Provides mechanisms to initiate and finalize dynamic certificate signing requests.

use crate::Session;
use opcua_types::{ByteString, CallMethodRequest, NodeId, StatusCode, Variant};
use tracing::error;

/// Client helper for dynamic certificate signing request exchange with a GDS Directory.
///
/// Constructed from real, discovered NodeIds -- see [`crate::gds::GdsClient::discover`]. Every
/// real GDS deployment assigns its own namespace index to the GDS companion types, so there is no
/// valid fixed default to construct this with.
pub struct GdsCsrClient {
    /// NodeId of the GDS Directory object (`CertificateDirectoryType` instance, OPC-10000-12
    /// §7.9.2). `StartSigningRequest`/`FinishRequest` are methods on this same object -- there is
    /// no separate "CertificateManager" object; "CertificateManager" is the deployment role a
    /// server hosting this object plays, per §7.9.2's own wording.
    pub directory_object_id: NodeId,
    /// NodeId of the `StartSigningRequest` method (§7.9.3).
    pub start_signing_request_id: NodeId,
    /// NodeId of the `FinishRequest` method (§7.9.5). Field name kept as
    /// `finish_signing_request_id` to match this client's existing `finish_signing_request`
    /// method name -- the real OPC UA method name is `FinishRequest`, shared by both the
    /// CSR-signing and new-key-pair-request flows.
    pub finish_signing_request_id: NodeId,
}

impl GdsCsrClient {
    /// Creates a `GdsCsrClient` from real, already-resolved NodeIds.
    pub fn new(
        directory_object_id: NodeId,
        start_signing_request_id: NodeId,
        finish_signing_request_id: NodeId,
    ) -> Self {
        Self {
            directory_object_id,
            start_signing_request_id,
            finish_signing_request_id,
        }
    }

    /// Submits a CSR to the GDS Directory to start the signing process (§7.9.3:
    /// `(ApplicationId, CertificateGroupId, CertificateTypeId, CertificateRequest) -> RequestId`,
    /// four input arguments -- verified against the real GDS companion NodeSet2.xml).
    /// Returns the GDS-allocated `NodeId` representing the request ID.
    pub async fn start_signing_request(
        &self,
        session: &Session,
        application_id: NodeId,
        certificate_group_id: NodeId,
        certificate_type_id: NodeId,
        csr_der: &[u8],
    ) -> Result<NodeId, StatusCode> {
        let request = CallMethodRequest {
            object_id: self.directory_object_id.clone(),
            method_id: self.start_signing_request_id.clone(),
            input_arguments: Some(vec![
                Variant::from(application_id),
                Variant::from(certificate_group_id),
                Variant::from(certificate_type_id),
                Variant::from(ByteString::from(csr_der)),
            ]),
        };

        match session.call_one(request).await {
            Ok(result) => {
                if result.status_code.is_good() {
                    if let Some(args) = result.output_arguments {
                        if !args.is_empty() {
                            if let Variant::NodeId(node_id) = &args[0] {
                                return Ok(*node_id.clone());
                            }
                        }
                    }
                    Err(StatusCode::BadUnexpectedError)
                } else {
                    Err(result.status_code)
                }
            }
            Err(err) => {
                error!("Failed to start signing request: {}", err);
                Err(err.status())
            }
        }
    }

    /// Polls or calls `FinishRequest` to fetch the signed certificate (and optional private key).
    /// Returns a tuple containing the signed DER certificate bytes, and optionally the PEM private key if regenerated.
    pub async fn finish_signing_request(
        &self,
        session: &Session,
        application_id: NodeId,
        request_id: NodeId,
    ) -> Result<(Vec<u8>, Option<Vec<u8>>), StatusCode> {
        let request = CallMethodRequest {
            object_id: self.directory_object_id.clone(),
            method_id: self.finish_signing_request_id.clone(),
            input_arguments: Some(vec![
                Variant::from(application_id),
                Variant::from(request_id),
            ]),
        };

        match session.call_one(request).await {
            Ok(result) => {
                if result.status_code.is_good() {
                    if let Some(args) = result.output_arguments {
                        if args.len() >= 2 {
                            let signed_cert = match &args[0] {
                                Variant::ByteString(bs) => bs.as_ref().to_vec(),
                                _ => return Err(StatusCode::BadUnexpectedError),
                            };
                            let private_key = match &args[1] {
                                Variant::ByteString(bs) if !bs.is_null() => {
                                    Some(bs.as_ref().to_vec())
                                }
                                _ => None,
                            };
                            return Ok((signed_cert, private_key));
                        }
                    }
                    Err(StatusCode::BadUnexpectedError)
                } else {
                    Err(result.status_code)
                }
            }
            Err(err) => {
                error!("Failed to finish signing request: {}", err);
                Err(err.status())
            }
        }
    }
}
