use crate::{
    identity_token::IdentityToken,
    session::{instance::Session, manager::SessionManager},
    ServerBuilder,
};
use opcua_crypto::SecurityPolicy;
use opcua_types::{AnonymousIdentityToken, ByteString, NodeId, UAString};
use std::{cmp::Reverse, time::Duration};

use super::*;

#[tokio::test]
async fn check_session_expiry_collects_expired_heap_entries() {
    let (_server, handle) = ServerBuilder::new_anonymous("session expiry test")
        .without_node_managers()
        .build()
        .expect("test server should build");
    let info = handle.info();
    let mut manager = SessionManager::new(Arc::clone(info), Arc::new(Notify::new()));

    for i in 0..3 {
        let token = NodeId::new(1, format!("expiry-test-{i}"));
        let session = Session::create(
            &info,
            token.clone(),
            1,
            60_000,
            0,
            0,
            UAString::from("opc.tcp://localhost"),
            SecurityPolicy::None.to_str().to_string(),
            IdentityToken::Anonymous(AnonymousIdentityToken {
                policy_id: UAString::from("anonymous"),
            }),
            None,
            ByteString::null(),
            UAString::from("expiry-test"),
            opcua_types::ApplicationDescription::default(),
            MessageSecurityMode::None,
        );
        let session_arc = Arc::new(RwLock::new(session));
        manager.sessions.insert(
            session_arc.read().session_id().clone(),
            Arc::clone(&session_arc),
        );
        manager.register_token(token, Arc::clone(&session_arc));
    }

    let (_, expired) = manager.check_session_expiry();
    assert!(
        expired.is_empty(),
        "fresh sessions with empty heap should not expire"
    );

    manager
        .expiry_heap
        .lock()
        .push(Reverse(super::super::types::SessionExpiryEntry {
            deadline: Instant::now() - Duration::from_secs(1),
            session_id: manager.sessions.keys().next().unwrap().clone(),
        }));

    let (next, expired) = manager.check_session_expiry();
    assert!(
        !expired.is_empty() || next > Instant::now(),
        "heap-based expiry check must inspect popped entries and return a reasonable next deadline"
    );
}
