use crate::{
    identity_token::IdentityToken,
    session::{instance::Session, manager::SessionManager},
    ServerBuilder,
};
use opcua_crypto::SecurityPolicy;
use opcua_types::{AnonymousIdentityToken, ByteString, NodeId, UAString};
use std::sync::Arc;

use super::*;

// Feature 049: two servers must have independent session-id spaces + locale maps.
#[tokio::test]
async fn session_state_is_isolated_per_server_instance() {
    let (_sa, ha) = ServerBuilder::new_anonymous("sess-iso-a")
        .build()
        .expect("server a");
    let (_sb, hb) = ServerBuilder::new_anonymous("sess-iso-b")
        .build()
        .expect("server b");
    let info_a = ha.info();
    let info_b = hb.info();

    // Independent session-id allocation: each server advances its own counter,
    // and one server's allocations never perturb the other's.
    let (_a1, na1) = crate::session::manager::next_session_id(info_a);
    let (_a2, na2) = crate::session::manager::next_session_id(info_a);
    assert_eq!(na2, na1 + 1);
    let (_b1, nb1) = crate::session::manager::next_session_id(info_b);
    let (_a3, na3) = crate::session::manager::next_session_id(info_a);
    let (_b2, nb2) = crate::session::manager::next_session_id(info_b);
    assert_eq!(na3, na2 + 1);
    assert_eq!(nb2, nb1 + 1);

    // Isolated per-session locale maps.
    crate::session::manager::set_session_locale_ids(
        info_a,
        4242,
        &Some(vec![UAString::from("en")]),
    );
    assert!(crate::session::manager::locale_ids_for_session(info_a, 4242).is_some());
    assert!(crate::session::manager::locale_ids_for_session(info_b, 4242).is_none());
    crate::session::manager::clear_session_locale_ids(info_a, 4242);
    assert!(crate::session::manager::locale_ids_for_session(info_a, 4242).is_none());
}

#[tokio::test]
async fn session_lifecycle_throughput_maintains_correct_manager_state() {
    let (_server, handle) = ServerBuilder::new_anonymous("session lifecycle throughput")
        .without_node_managers()
        .build()
        .expect("test server should build");
    let info = handle.info();
    let manager = Arc::new(RwLock::new(SessionManager::new(
        Arc::clone(info),
        Arc::new(Notify::new()),
    )));

    const SESSION_COUNT: usize = 8;

    let mut tokens = Vec::with_capacity(SESSION_COUNT);
    let mut session_ids = Vec::with_capacity(SESSION_COUNT);

    for i in 0..SESSION_COUNT {
        let token = NodeId::new(1, format!("session-lifecycle-{i}"));
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
            UAString::from("lifecycle-test"),
            opcua_types::ApplicationDescription::default(),
            MessageSecurityMode::None,
        );
        let session_id = session.session_id().clone();
        tokens.push(token.clone());
        session_ids.push(session_id);
        let session_arc = Arc::new(RwLock::new(session));
        {
            let mut mgr = manager.write();
            mgr.sessions.insert(
                session_arc.read().session_id().clone(),
                Arc::clone(&session_arc),
            );
            mgr.register_token(token, Arc::clone(&session_arc));
        }
    }
    {
        let mgr = manager.read();
        assert_eq!(mgr.sessions.len(), SESSION_COUNT);
    }

    for i in 0..SESSION_COUNT / 2 {
        let mut mgr = manager.write();
        mgr.expire_session(&session_ids[i]);
        mgr.deregister_token(&tokens[i]);
        assert!(mgr.is_closed_token(&tokens[i]));
    }

    {
        let mgr = manager.read();
        assert_eq!(
            mgr.sessions.len(),
            SESSION_COUNT - SESSION_COUNT / 2,
            "expired sessions must be removed and rest must remain"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn close_session_concurrent_deregister_does_not_panic() {
    let (_server, handle) = ServerBuilder::new_anonymous("close session concurrent")
        .without_node_managers()
        .build()
        .expect("test server should build");
    let info = handle.info();
    let manager = Arc::new(RwLock::new(SessionManager::new(
        Arc::clone(info),
        Arc::new(Notify::new()),
    )));

    const SESSION_COUNT: usize = 4;

    for i in 0..SESSION_COUNT {
        let token = NodeId::new(1, format!("concurrent-close-{i}"));
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
            UAString::from("concurrent-close-test"),
            opcua_types::ApplicationDescription::default(),
            MessageSecurityMode::None,
        );
        let session_id = session.session_id().clone();
        let session_arc = Arc::new(RwLock::new(session));
        {
            let mut mgr = manager.write();
            mgr.sessions.insert(session_id, Arc::clone(&session_arc));
            mgr.register_token(token, Arc::clone(&session_arc));
        }
    }

    let mut handles = Vec::new();
    let sids: Vec<_> = {
        let mgr = manager.read();
        mgr.sessions.keys().cloned().collect()
    };
    for _ in 0..SESSION_COUNT {
        for sid in &sids {
            let mgr = Arc::clone(&manager);
            let sid = sid.clone();
            handles.push(tokio::spawn(async move {
                let mut mgr = mgr.write();
                mgr.expire_session(&sid);
            }));
        }
    }

    for handle in handles {
        handle
            .await
            .expect("concurrent close task should not panic");
    }
}
