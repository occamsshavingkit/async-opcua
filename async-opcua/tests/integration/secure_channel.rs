//! SecureChannel concurrency regression tests for async lock remediation.

use std::{sync::atomic::Ordering, time::Duration};

use opcua::{
    client::IdentityToken,
    crypto::SecurityPolicy,
    types::{MessageSecurityMode, NodeId, ReadValueId, StatusCode, TimestampsToReturn, VariableId},
};
use tokio::time::{timeout, Instant};

use crate::utils::{default_client, test_server, Tester, TEST_COUNTER};

#[tokio::test]
async fn rsa_secure_channel_connection_storm_completes_without_runtime_starvation() {
    let tester = Tester::new(test_server(), false).await;
    let endpoint = tester.endpoint();
    let started_at = Instant::now();
    let mut joins = Vec::new();

    for _ in 0..24 {
        let endpoint = endpoint.clone();
        joins.push(tokio::spawn(async move {
            let test_id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
            let mut client = default_client(test_id, false).client().unwrap();
            let (session, event_loop) = client
                .connect_to_matching_endpoint(
                    (
                        endpoint.as_str(),
                        SecurityPolicy::Basic256Sha256.to_str(),
                        MessageSecurityMode::SignAndEncrypt,
                    ),
                    IdentityToken::Anonymous,
                )
                .await
                .unwrap();
            let event_loop = event_loop.spawn();

            timeout(Duration::from_secs(20), session.wait_for_connection())
                .await
                .expect("storm connection should activate before timeout");

            let values = session
                .read(
                    &[ReadValueId::from(<VariableId as Into<NodeId>>::into(
                        VariableId::Server_ServiceLevel,
                    ))],
                    TimestampsToReturn::Both,
                    0.0,
                )
                .await
                .unwrap();
            assert_eq!(values.len(), 1);

            session.disconnect().await.unwrap();
            event_loop.await.unwrap()
        }));
    }

    for join in joins {
        assert_eq!(join.await.unwrap(), StatusCode::Good);
    }

    assert!(
        started_at.elapsed() < Duration::from_secs(20),
        "secure channel connection storm took {:?}",
        started_at.elapsed()
    );
}
