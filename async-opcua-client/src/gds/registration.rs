// OPCUA for Rust
// SPDX-License-Identifier: MPL-2.0
// Copyright (C) 2017-2026 Adam Lock

//! Global Discovery Server (GDS) registration client implementation.
//! Provides mechanisms to register client or server applications with a GDS directory.

use crate::Session;
use opcua_types::{ApplicationDescription, CallMethodRequest, NodeId, StatusCode, Variant};
use tracing::{error, info};

/// Client helper for interacting with the GDS registration and directory services.
///
/// Constructed from real, discovered NodeIds -- see [`crate::gds::GdsClient::discover`]. Every
/// real GDS deployment assigns its own namespace index to the GDS companion types, so there is no
/// valid fixed default to construct this with.
pub struct GdsRegistrationClient {
    /// NodeId of the GDS Directory object (`CertificateDirectoryType` instance, OPC-10000-12
    /// §7.9.2).
    pub directory_object_id: NodeId,
    /// NodeId of the `RegisterApplication` method.
    pub register_method_id: NodeId,
}

impl GdsRegistrationClient {
    /// Creates a `GdsRegistrationClient` from real, already-resolved NodeIds.
    pub fn new(directory_object_id: NodeId, register_method_id: NodeId) -> Self {
        Self {
            directory_object_id,
            register_method_id,
        }
    }

    /// Register an application with the GDS directory.
    /// Returns the assigned unique `NodeId` of the registered application.
    pub async fn register_application(
        &self,
        session: &Session,
        application_description: ApplicationDescription,
    ) -> Result<NodeId, StatusCode> {
        let request = CallMethodRequest {
            object_id: self.directory_object_id.clone(),
            method_id: self.register_method_id.clone(),
            input_arguments: Some(vec![Variant::from(application_description)]),
        };

        match session.call_one(request).await {
            Ok(result) => {
                if result.status_code.is_good() {
                    if let Some(args) = result.output_arguments {
                        if !args.is_empty() {
                            if let Variant::NodeId(node_id) = &args[0] {
                                info!(
                                    "Application successfully registered with GDS, assigned ID: {}",
                                    node_id
                                );
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
                error!("Failed to register application with GDS: {}", err);
                Err(err.status())
            }
        }
    }
}
