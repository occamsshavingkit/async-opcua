use std::time::Duration;

use opcua::{
    client::IdentityToken,
    crypto::{create_signature_data, legacy_encrypt_secret, SecurityPolicy, X509},
    types::{
        ActivateSessionRequest, ByteString, DateTime, ExtensionObject, MessageSecurityMode, NodeId,
        ReadValueId, RequestHeader, SignatureData, StatusCode, TimestampsToReturn, UAString,
        UserNameIdentityToken, UserTokenType, VariableId,
    },
};
use opcua_client::{services::CreateSession, transport::TransportPollResult, UARequest};
use opcua_core::{config::Config, ResponseMessage};

use crate::utils::{client_user_token, default_server, Tester, CLIENT_USERPASS_ID};

#[tokio::test]
async fn rsa_kem_user_token_success() {
    let _ = env_logger::try_init();

    let server = default_server();
    let mut tester = Tester::new(server, false).await;

    let (session, handle) = tester
        .connect(
            SecurityPolicy::Aes128Sha256RsaOaep,
            MessageSecurityMode::SignAndEncrypt,
            client_user_token(),
        )
        .await
        .expect("connect with UserName token over RSA-KEM endpoint should succeed");

    let _h = handle.spawn();

    tokio::time::timeout(Duration::from_secs(20), session.wait_for_connection())
        .await
        .expect("session should activate within 20s");

    let read_values = session
        .read(
            &[ReadValueId::from(<VariableId as Into<NodeId>>::into(
                VariableId::Server_ServiceLevel,
            ))],
            TimestampsToReturn::Both,
            0.0,
        )
        .await
        .expect("Read of Server_ServiceLevel should succeed");

    assert_eq!(read_values.len(), 1);
    assert_eq!(read_values[0].status(), StatusCode::Good);

    let _ = session.disconnect().await;
}

/// Helper: open a channel + CreateSession, returning the channel with poller and
/// the raw CreateSessionResponse.
async fn setup_rsa_kem_channel(
    tester: &mut Tester,
) -> (
    opcua_client::AsyncSecureChannel,
    tokio::task::JoinHandle<()>,
    opcua::types::EndpointDescription,
    opcua::types::UserTokenPolicy,
    opcua::types::CreateSessionResponse,
) {
    let endpoints = tester
        .client
        .get_endpoints(tester.endpoint(), &[], &[])
        .await
        .expect("GetEndpoints should succeed");

    let target_ep = endpoints
        .iter()
        .find(|ep| {
            ep.security_policy_uri.as_ref() == SecurityPolicy::Aes128Sha256RsaOaep.to_uri()
                && ep.security_mode == MessageSecurityMode::SignAndEncrypt
        })
        .expect("Need RSA-KEM SignAndEncrypt endpoint")
        .clone();

    let user_token_policy = target_ep
        .user_identity_tokens
        .as_ref()
        .and_then(|tokens| {
            tokens
                .iter()
                .find(|p| p.token_type == UserTokenType::UserName)
        })
        .expect("Need UserName token policy")
        .clone();

    let (channel, mut channel_loop) = tester
        .client
        .open_secure_channel_to_endpoint_directly(target_ep.clone(), IdentityToken::Anonymous)
        .await
        .expect("OpenSecureChannel should succeed");

    let channel_poller = tokio::spawn(async move {
        loop {
            let poll = channel_loop.poll().await;
            if matches!(poll, TransportPollResult::Closed(_)) {
                break;
            }
        }
    });

    let create_resp = CreateSession::new_manual(
        tester.client.certificate_store(),
        &target_ep,
        1,
        Duration::from_secs(10),
        NodeId::null(),
        channel.request_handle(),
    )
    .endpoint_url(tester.endpoint())
    .client_description(tester.client.config().application_description().clone())
    .session_name("rsa-kem-test")
    .client_cert_from_store(tester.client.certificate_store())
    .send(&channel)
    .await
    .expect("CreateSession should succeed");

    (
        channel,
        channel_poller,
        target_ep,
        user_token_policy,
        create_resp,
    )
}

/// OPC 10000-6 §6.7.3 and OPC 10000-4 §7.36 (ActivateSession):
/// A corrupted RSA-KEM ciphertext in the UserName identity token must be
/// rejected.
#[tokio::test]
async fn rsa_kem_corrupted_token_rejected() {
    let _ = env_logger::try_init();

    let server = default_server();
    let mut tester = Tester::new(server, false).await;

    let (channel, channel_poller, _target_ep, user_token_policy, create_resp) =
        setup_rsa_kem_channel(&mut tester).await;

    let server_nonce = create_resp.server_nonce;
    let server_cert_bytes = create_resp.server_certificate;

    let server_x509 =
        X509::from_byte_string(&server_cert_bytes).expect("server cert should parse to X509");

    let password = format!("{CLIENT_USERPASS_ID}_password");
    let secret = legacy_encrypt_secret(
        SecurityPolicy::Aes128Sha256RsaOaep,
        MessageSecurityMode::SignAndEncrypt,
        &user_token_policy,
        server_nonce.as_ref(),
        &Some(server_x509),
        password.as_bytes(),
    )
    .expect("encrypting password should succeed");

    let mut corrupted = secret.secret.as_ref().to_vec();
    assert!(!corrupted.is_empty(), "encrypted secret must not be empty");
    corrupted[0] ^= 0xFF;

    let corrupted_token = UserNameIdentityToken {
        policy_id: secret.policy,
        user_name: UAString::from(CLIENT_USERPASS_ID),
        password: ByteString::from(corrupted),
        encryption_algorithm: secret.encryption_algorithm,
    };

    let corrupt_identity = ExtensionObject::from_message(corrupted_token);

    let client_store = tester.client.certificate_store();
    let client_pkey = {
        let store = client_store.read();
        store
            .read_own_pkey()
            .expect("client private key should be available")
    };

    let client_signature = create_signature_data(
        &client_pkey,
        SecurityPolicy::Aes128Sha256RsaOaep,
        &server_cert_bytes,
        &server_nonce,
    )
    .expect("client signature should be computable");

    let request_header = RequestHeader {
        authentication_token: create_resp.authentication_token.clone(),
        timestamp: DateTime::now(),
        request_handle: channel.request_handle(),
        timeout_hint: 10000,
        ..Default::default()
    };

    let activate_request = ActivateSessionRequest {
        request_header,
        client_signature,
        client_software_certificates: None,
        locale_ids: None,
        user_identity_token: corrupt_identity,
        user_token_signature: SignatureData::null(),
    };

    let result = channel
        .send(activate_request, Duration::from_secs(10))
        .await;

    channel.close_channel().await;
    let _ = tokio::time::timeout(Duration::from_secs(5), channel_poller).await;

    match result {
        Ok(ResponseMessage::ActivateSession(resp)) => {
            assert_eq!(
                resp.response_header.service_result,
                StatusCode::BadIdentityTokenRejected,
                "corrupted RSA-KEM ciphertext must result in BadIdentityTokenRejected"
            );
        }
        Ok(ResponseMessage::ServiceFault(fault)) => {
            let code = fault.response_header.service_result;
            assert!(
                code == StatusCode::BadIdentityTokenRejected
                    || code == StatusCode::BadUserAccessDenied,
                "corrupted RSA-KEM ciphertext must be rejected, got ServiceFault({})",
                code
            );
        }
        Ok(other) => panic!(
            "expected ActivateSession response, got {}",
            other.type_name()
        ),
        Err(e) => {
            let status = e.status();
            assert!(
                status == StatusCode::BadIdentityTokenRejected
                    || status == StatusCode::BadUserAccessDenied,
                "corrupted RSA-KEM ciphertext must be rejected, got {}",
                status
            );
        }
    }
}
