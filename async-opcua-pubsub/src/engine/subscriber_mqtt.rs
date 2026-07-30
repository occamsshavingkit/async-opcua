use std::sync::Arc;

use opcua_core::sync::RwLock;
use opcua_types::{ContextOwned, StatusCode};

use crate::{
    config::DataSetReaderConfig,
    subscriber::{SubscriberApplyOutcome, SubscriberRuntime},
};

pub(super) fn process_reader_payload(
    runtime: &Arc<RwLock<SubscriberRuntime>>,
    reader: &DataSetReaderConfig,
    payload: &[u8],
) -> Result<SubscriberApplyOutcome, StatusCode> {
    let ctx_owned = ContextOwned::default();
    let ctx = ctx_owned.context();
    runtime
        .write()
        .process_datagram_for_reader(reader, payload, &ctx)
}
