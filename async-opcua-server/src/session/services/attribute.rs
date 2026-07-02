use std::sync::Arc;

#[cfg(feature = "history")]
use opcua_core::trace_write_lock;
use tracing::{debug_span, Instrument};

use crate::{
    node_manager::{
        consume_results, DynNodeManager, NodeManagers, ReadNode, RequestContext,
    },
    session::{
        controller::Response,
        message_handler::Request,
        services::{invoke_service_concurrently_mut, ServiceCb},
    },
};
#[cfg(feature = "history")]
use crate::node_manager::{
    HistoryNode, HistoryReadDetails, HistoryUpdateDetails, HistoryUpdateNode,
};
use opcua_types::{
    ReadRequest, ReadResponse, ResponseHeader, StatusCode, TimestampsToReturn,
};
#[cfg(feature = "history")]
use opcua_types::{
    ByteString, DeleteAtTimeDetails, DiagnosticInfo, ExtensionObject, HistoryReadRequest,
    HistoryReadResponse, HistoryReadResult, HistoryUpdateRequest, HistoryUpdateResponse,
    HistoryUpdateResult, NodeId, ObjectId,
};
#[cfg_attr(not(feature = "subscriptions"), allow(dead_code))]
pub(crate) async fn read(node_managers: NodeManagers, request: Request<ReadRequest>) -> Response {
    let context = request.context();
    let nodes_to_read = take_service_items!(
        request,
        request.request.nodes_to_read,
        request.info.operational_limits.max_nodes_per_read
    );
    if request.request.max_age < 0.0 {
        return service_fault!(request, StatusCode::BadMaxAgeInvalid);
    }
    if request.request.timestamps_to_return == TimestampsToReturn::Invalid {
        return service_fault!(request, StatusCode::BadTimestampsToReturnInvalid);
    }

    let mut results: Vec<_> = nodes_to_read
        .into_iter()
        .map(|n| ReadNode::new(n, request.request.request_header.return_diagnostics))
        .collect();

    let max_age = request.request.max_age;
    let timestamps_to_return = request.request.timestamps_to_return;

    struct ReadServiceCb {
        max_age: f64,
        timestamps_to_return: TimestampsToReturn,
    }
    impl ServiceCb<ReadNode> for ReadServiceCb {
        async fn call(
            &self,
            batch: &mut [&mut ReadNode],
            node_manager: &Arc<DynNodeManager>,
            context: RequestContext,
        ) {
            if let Err(e) = node_manager
                .read(&context, self.max_age, self.timestamps_to_return, batch)
                .instrument(debug_span!("Read", node_manager = %node_manager.name()))
                .await
            {
                for node in batch {
                    node.set_error(e);
                }
            }
        }
    }
    invoke_service_concurrently_mut(
        context,
        &mut results,
        &node_managers,
        ReadServiceCb {
            max_age,
            timestamps_to_return,
        },
        |node, node_manager| {
            node_manager.owns_node(&node.node().node_id)
                && node.status() == StatusCode::BadNodeIdUnknown
        },
    )
    .await;

    let (results, diagnostic_infos) =
        consume_results(results, request.request.request_header.return_diagnostics);

    Response {
        message: ReadResponse {
            response_header: ResponseHeader::new_good(request.request_handle),
            results,
            diagnostic_infos,
        }
        .into(),
        request_id: request.request_id,
    }
}

#[cfg(feature = "history")]
pub(crate) async fn history_read(
    node_managers: NodeManagers,
    request: Request<HistoryReadRequest>,
) -> Response {
    let context = request.context();
    if matches!(
        request.request.timestamps_to_return,
        TimestampsToReturn::Invalid | TimestampsToReturn::Neither
    ) {
        return service_fault!(request, StatusCode::BadTimestampsToReturnInvalid);
    }
    let Some(items) = request.request.nodes_to_read else {
        return service_fault!(request, StatusCode::BadNothingToDo);
    };
    if items.is_empty() {
        return service_fault!(request, StatusCode::BadNothingToDo);
    }
    let details =
        match HistoryReadDetails::from_extension_object(request.request.history_read_details) {
            Ok(r) => r,
            Err(e) => return service_fault!(request, e),
        };

    let is_events = matches!(details, HistoryReadDetails::Events(_));

    if is_events {
        if items.len()
            > request
                .info
                .operational_limits
                .max_nodes_per_history_read_events
        {
            return service_fault!(request, StatusCode::BadTooManyOperations);
        }
    } else if items.len()
        > request
            .info
            .operational_limits
            .max_nodes_per_history_read_data
    {
        return service_fault!(request, StatusCode::BadTooManyOperations);
    }
    let return_diagnostics = request.request.request_header.return_diagnostics;
    let mut nodes = crate::services::history_read::prepare_history_nodes(
        &request.session,
        &node_managers,
        &context,
        items,
        return_diagnostics,
        is_events,
        request.request.release_continuation_points,
    )
    .await;

    // If we are releasing continuation points we should not return any data.
    if request.request.release_continuation_points {
        let pairs: Vec<(HistoryReadResult, Option<DiagnosticInfo>)> = nodes
            .into_iter()
            .map(|mut node| {
                let diagnostic_info = node.take_diagnostic_info();
                let result = HistoryReadResult {
                    status_code: node.status(),
                    continuation_point: ByteString::null(),
                    history_data: ExtensionObject::null(),
                };
                (result, diagnostic_info)
            })
            .collect();
        let (results, diagnostic_infos) = consume_results(pairs, return_diagnostics);

        return Response {
            message: HistoryReadResponse {
                response_header: ResponseHeader::new_good(request.request_handle),
                results,
                diagnostic_infos,
            }
            .into(),
            request_id: request.request_id,
        };
    }

    struct HistoryReadServiceCb {
        details: HistoryReadDetails,
        timestamps_to_return: TimestampsToReturn,
    }

    impl ServiceCb<HistoryNode> for HistoryReadServiceCb {
        async fn call(
            &self,
            batch: &mut [&mut HistoryNode],
            node_manager: &Arc<DynNodeManager>,
            context: RequestContext,
        ) {
            let result = match &self.details {
                HistoryReadDetails::RawModified(d) => node_manager
                    .history_read_raw_modified(&context, d, &mut *batch, self.timestamps_to_return)
                    .instrument(
                        debug_span!("HistoryReadRawModified", node_manager = %node_manager.name()),
                    )
                    .await,
                HistoryReadDetails::AtTime(d) => {
                    node_manager
                        .history_read_at_time(&context, d, &mut *batch, self.timestamps_to_return)
                        .instrument(
                            debug_span!("HistoryReadAtTime", node_manager = %node_manager.name()),
                        )
                        .await
                }
                HistoryReadDetails::Processed(d) => node_manager
                    .history_read_processed(&context, d, &mut *batch, self.timestamps_to_return)
                    .instrument(
                        debug_span!("HistoryReadProcessed", node_manager = %node_manager.name()),
                    )
                    .await,
                HistoryReadDetails::Events(d) => {
                    node_manager
                        .history_read_events(&context, d, &mut *batch, self.timestamps_to_return)
                        .instrument(
                            debug_span!("HistoryReadEvents", node_manager = %node_manager.name()),
                        )
                        .await
                }
                HistoryReadDetails::Annotations(d) => node_manager
                    .history_read_annotations(&context, d, &mut *batch, self.timestamps_to_return)
                    .instrument(
                        debug_span!("HistoryReadAnnotations", node_manager = %node_manager.name()),
                    )
                    .await,
            };

            if let Err(e) = result {
                for node in batch {
                    node.set_status(e);
                }
            }
        }
    }

    let is_events = matches!(details, HistoryReadDetails::Events(_));
    invoke_service_concurrently_mut(
        context,
        &mut nodes,
        &node_managers,
        HistoryReadServiceCb {
            details,
            timestamps_to_return: request.request.timestamps_to_return,
        },
        |node, node_manager| {
            if node.status() != StatusCode::BadNodeIdUnknown {
                return false;
            }
            if node.node_id() == &ObjectId::Server && is_events {
                node_manager.owns_server_events()
            } else {
                node_manager.owns_node(node.node_id())
            }
        },
    )
    .await;

    let pairs: Vec<(HistoryReadResult, Option<DiagnosticInfo>)> = {
        let mut session = trace_write_lock!(request.session);
        nodes
            .into_iter()
            .map(|mut node| {
                let diagnostic_info = node.take_diagnostic_info();
                let result = node.into_result(&mut session);
                (result, diagnostic_info)
            })
            .collect()
    };
    let (results, diagnostic_infos) = consume_results(pairs, return_diagnostics);

    Response {
        message: HistoryReadResponse {
            response_header: ResponseHeader::new_good(&request.request.request_header),
            results,
            diagnostic_infos,
        }
        .into(),
        request_id: request.request_id,
    }
}

#[cfg(feature = "history")]
pub(crate) async fn history_update(
    node_managers: NodeManagers,
    request: Request<HistoryUpdateRequest>,
) -> Response {
    let context = request.context();
    let items = take_service_items!(
        request,
        request.request.history_update_details,
        request.info.operational_limits.max_nodes_per_history_update
    );
    let return_diagnostics = request.request.request_header.return_diagnostics;

    let mut nodes: Vec<_> = items
        .into_iter()
        .map(|obj| {
            let details = match HistoryUpdateDetails::from_extension_object(obj) {
                Ok(h) => h,
                Err(e) => {
                    // need some empty history update node here, it won't be passed to node managers.
                    let mut node = HistoryUpdateNode::new(
                        HistoryUpdateDetails::DeleteAtTime(DeleteAtTimeDetails {
                            node_id: NodeId::null(),
                            req_times: None,
                        }),
                        return_diagnostics,
                    );
                    node.set_status(e);
                    return node;
                }
            };
            HistoryUpdateNode::new(details, return_diagnostics)
        })
        .collect();

    struct HistoryWriteServiceCb;

    impl ServiceCb<HistoryUpdateNode> for HistoryWriteServiceCb {
        async fn call(
            &self,
            items: &mut [&mut HistoryUpdateNode],
            node_manager: &Arc<DynNodeManager>,
            context: RequestContext,
        ) {
            if let Err(e) = node_manager
                .history_update(&context, &mut *items)
                .instrument(debug_span!("HistoryUpdate", node_manager = %node_manager.name()))
                .await
            {
                for node in items {
                    node.set_status(e);
                }
            }
        }
    }

    invoke_service_concurrently_mut(
        context,
        &mut nodes,
        &node_managers,
        HistoryWriteServiceCb,
        |node, node_manager| {
            // If the node is a server event, we need to check if the manager owns server events.
            if node.details().node_id() == &ObjectId::Server
                && matches!(
                    node.details(),
                    HistoryUpdateDetails::UpdateEvent(_) | HistoryUpdateDetails::DeleteEvent(_)
                )
            {
                node_manager.owns_server_events()
            } else {
                node_manager.owns_node(node.details().node_id())
                    && node.status() == StatusCode::BadNodeIdUnknown
            }
        },
    )
    .await;

    let pairs: Vec<(HistoryUpdateResult, Option<DiagnosticInfo>)> = nodes
        .into_iter()
        .map(|mut node| {
            let diagnostic_info = node.take_diagnostic_info();
            let result = node.into_result();
            (result, diagnostic_info)
        })
        .collect();
    let (results, diagnostic_infos) = consume_results(pairs, return_diagnostics);

    Response {
        message: HistoryUpdateResponse {
            response_header: ResponseHeader::new_good(&request.request.request_header),
            results,
            diagnostic_infos,
        }
        .into(),
        request_id: request.request_id,
    }
}
