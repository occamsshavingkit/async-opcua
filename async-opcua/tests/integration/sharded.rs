//! Feature 072 US2: sharded (thread-per-core) run mode — end-to-end correctness.
//!
//! Verifies that `Server::run_sharded` serves a real client across multiple
//! pinned SO_REUSEPORT shards, and that a second connection (likely landing on
//! a different shard) sees the same address space — i.e. the node managers and
//! session manager are genuinely shared across shards, so the graft is
//! wire-correct, not merely faster.

use std::time::Duration;

use opcua::{
    server::address_space::{AccessLevel, VariableBuilder},
    types::{
        AttributeId, DataTypeId, DataValue, ObjectId, ReferenceTypeId, StatusCode,
        TimestampsToReturn, VariableTypeId, Variant, WriteValue,
    },
};
use opcua_types::NumericRange;

use super::utils::{read_value_id, setup_sharded};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sharded_read_write_shared_address_space() {
    // Two shards behind SO_REUSEPORT; connection 1 comes from setup.
    let (mut tester, nm, session1) = setup_sharded(vec![0, 1]).await;

    // Add a writable Int32 to the shared address space.
    let id = nm.inner().next_node_id();
    nm.inner().add_node(
        nm.address_space(),
        tester.handle.type_tree(),
        VariableBuilder::new(&id, "ShardVar", "ShardVar")
            .data_type(DataTypeId::Int32)
            .value(1i32)
            .access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .user_access_level(AccessLevel::CURRENT_READ | AccessLevel::CURRENT_WRITE)
            .build()
            .into(),
        &ObjectId::ObjectsFolder.into(),
        &ReferenceTypeId::Organizes.into(),
        Some(&VariableTypeId::BaseDataVariableType.into()),
        Vec::new(),
    );

    // Client 1 writes a sentinel value, then reads it back.
    let write = WriteValue {
        value: DataValue {
            value: Some(Variant::Int32(4242)),
            status: Some(StatusCode::Good),
            ..Default::default()
        },
        node_id: id.clone(),
        attribute_id: AttributeId::Value as u32,
        index_range: NumericRange::None,
    };
    assert_eq!(
        session1.write(&[write]).await.unwrap(),
        vec![StatusCode::Good]
    );
    let r1 = session1
        .read(
            &[read_value_id(AttributeId::Value, &id)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(r1[0].value, Some(Variant::Int32(4242)));

    // A second client — likely a different shard — must observe the same value,
    // proving the address space and session manager are shared across shards.
    let (session2, lp2) = tester.connect_default().await.unwrap();
    lp2.spawn();
    tokio::time::timeout(Duration::from_secs(5), session2.wait_for_connection())
        .await
        .unwrap();
    let r2 = session2
        .read(
            &[read_value_id(AttributeId::Value, &id)],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .unwrap();
    assert_eq!(r2[0].value, Some(Variant::Int32(4242)));
}
