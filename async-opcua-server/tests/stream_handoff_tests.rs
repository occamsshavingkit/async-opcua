//! Stream handoff tests.

#[cfg(feature = "wss")]
#[tokio::test]
async fn run_with_wss_streams_requires_tls_configuration() {
    let (server, _handle) = ServerBuilder::new_anonymous("WSS Missing TLS Test")
        .build()
        .expect("server should build");
    let (_tx, rx) = tokio::sync::mpsc::channel::<(TcpStream, std::net::SocketAddr, ())>(1);
    let result = server
        .run_with_wss_streams(rx, "127.0.0.1:4843".parse().unwrap())
        .await;
    assert_eq!(
        result,
        Err("Cannot run WSS listener without a WSS rustls ServerConfig".to_string())
    );
}

#[cfg(feature = "wss")]
mod wss {
    use super::*;
    use base64::Engine;
    use opcua_core::{comms::wss::WsByteStream, config::Config};
    use opcua_crypto::CertificateStore;
    use std::sync::{atomic::Ordering, Arc};
    use tokio_tungstenite::tungstenite::client::ClientRequestBuilder;

    #[tokio::test]
    async fn run_with_wss_streams_acknowledges_hello_and_drops_token() {
        let dir = tempfile::tempdir().unwrap();
        let cert_der = dir.path().join("tls_cert.der");
        let cert_pem = dir.path().join("tls_cert.pem");
        let key_pem = dir.path().join("tls_key.pem");
        let builder = ServerBuilder::new_anonymous("WSS Stream Handoff Test")
            .host("localhost")
            // Deliberately differs from the caller's ephemeral WSS listener port.
            .port(4840);
        let desc = builder.config().application_description();
        CertificateStore::create_certificate_and_key(&desc.into(), true, &cert_der, &key_pem)
            .unwrap();
        let der = std::fs::read(&cert_der).unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&der);
        let mut pem = String::from("-----BEGIN CERTIFICATE-----\n");
        for chunk in b64.as_bytes().chunks(64) {
            pem.push_str(std::str::from_utf8(chunk).unwrap());
            pem.push('\n');
        }
        pem.push_str("-----END CERTIFICATE-----\n");
        std::fs::write(&cert_pem, pem).unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let endpoint_url = format!("opc.wss://localhost:{}/", addr.port());
        let (server, handle) = builder
            .discovery_urls(vec![endpoint_url.clone()])
            .websocket_tls(&cert_pem, &key_pem)
            .expect("load WSS TLS cert/key")
            .build()
            .unwrap();
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        let server_task = tokio::spawn(server.run_with_wss_streams(rx, addr));

        let socket = TcpStream::connect(addr).await.unwrap();
        let (accepted, peer) = listener.accept().await.unwrap();
        let (token, mut token_drop_rx) = drop_token();
        tx.send((accepted, peer, token)).await.unwrap();

        let mut roots = rustls::RootCertStore::empty();
        roots
            .add(rustls::pki_types::CertificateDer::from(der))
            .unwrap();
        let tls_config = rustls::ClientConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        let mut client = tokio::time::timeout(Duration::from_secs(5), async {
            let tls = tokio_rustls::TlsConnector::from(Arc::new(tls_config))
                .connect("localhost".try_into().unwrap(), socket)
                .await
                .expect("TLS upgrade should succeed");
            let request = ClientRequestBuilder::new(
                format!("wss://localhost:{}/", addr.port()).parse().unwrap(),
            )
            .with_sub_protocol("opcua+uacp");
            let (ws, _) = tokio_tungstenite::client_async(request, tls)
                .await
                .expect("WebSocket upgrade should succeed");
            WsByteStream::new(ws)
        })
        .await
        .expect("WSS upgrade should finish");

        let hello = HelloMessage::new(&endpoint_url, 65_535, 65_535, 0, 0);
        client.write_all(&hello.encode_to_vec()).await.unwrap();
        client.flush().await.unwrap();
        let mut framed = FramedRead::new(client, TcpCodec::new(DecodingOptions::default()));
        let message = tokio::time::timeout(Duration::from_secs(2), framed.next())
            .await
            .expect("server should respond to HEL");
        assert!(matches!(message, Some(Ok(Message::Acknowledge(_)))));
        assert_eq!(handle.info().port.load(Ordering::Relaxed), addr.port());
        assert!(handle
            .info()
            .base_endpoint()
            .ends_with(&format!(":{}", addr.port())));
        assert!(matches!(
            token_drop_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));

        drop(framed);
        expect_token_drop(token_drop_rx, "closed WSS connection").await;
        drop(tx);
        tokio::time::timeout(Duration::from_secs(5), server_task)
            .await
            .expect("server should exit after channel and connections close")
            .expect("server task should not panic")
            .expect("server should run with handed-off WSS streams");
    }
}

use std::time::Duration;

use futures::StreamExt;
use opcua_core::comms::{
    tcp_codec::{Message, TcpCodec},
    tcp_types::HelloMessage,
};
use opcua_server::ServerBuilder;
use opcua_types::{DecodingOptions, SimpleBinaryEncodable};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    sync::oneshot,
};
use tokio_util::codec::FramedRead;

struct DropToken(Option<oneshot::Sender<()>>);

impl Drop for DropToken {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            let _ = tx.send(());
        }
    }
}

fn drop_token() -> (DropToken, oneshot::Receiver<()>) {
    let (tx, rx) = oneshot::channel();
    (DropToken(Some(tx)), rx)
}

async fn expect_token_drop(rx: oneshot::Receiver<()>, context: &str) {
    tokio::time::timeout(Duration::from_secs(2), rx)
        .await
        .unwrap_or_else(|_| panic!("{context} token should drop"))
        .unwrap_or_else(|_| panic!("{context} token sender should not be canceled before drop"));
}

#[tokio::test]
async fn run_with_streams_accepts_externally_accepted_tcp_stream() {
    let handoff_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("handoff listener should bind");
    let addr = handoff_listener
        .local_addr()
        .expect("handoff listener should have address");
    let endpoint_url = format!("opc.tcp://127.0.0.1:{}/", addr.port());

    let builder = ServerBuilder::new_anonymous("Stream Handoff Test")
        .application_uri("urn:stream-handoff-test")
        .product_uri("urn:stream-handoff-test")
        .host("127.0.0.1")
        .port(addr.port())
        .discovery_urls(vec![endpoint_url.clone()]);
    let (server, handle) = builder
        .build()
        .expect("stream handoff test server should build");
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let server_task = tokio::spawn(async move {
        server
            .run_with_streams(rx)
            .await
            .expect("server should run with handed-off streams");
    });

    let mut client = TcpStream::connect(addr)
        .await
        .expect("client should connect to external listener");
    let (socket, peer_addr) = handoff_listener
        .accept()
        .await
        .expect("external listener should accept client");
    tx.send((socket, peer_addr, ()))
        .await
        .expect("accepted stream should hand off to server");

    let hello = HelloMessage::new(&endpoint_url, 65_535, 65_535, 0, 0);
    client
        .write_all(&SimpleBinaryEncodable::encode_to_vec(&hello))
        .await
        .expect("client should send HEL");

    let (read, _) = tokio::io::split(client);
    let mut framed = FramedRead::new(read, TcpCodec::new(DecodingOptions::default()));
    let message = tokio::time::timeout(Duration::from_secs(2), framed.next())
        .await
        .expect("server should respond to HEL");

    assert!(matches!(message, Some(Ok(Message::Acknowledge(_)))));

    handle.cancel();
    server_task.abort();
}

#[tokio::test]
async fn run_with_streams_drops_owned_token_when_connection_closes() {
    let handoff_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("handoff listener should bind");
    let addr = handoff_listener
        .local_addr()
        .expect("handoff listener should have address");
    let endpoint_url = format!("opc.tcp://127.0.0.1:{}/", addr.port());

    let builder = ServerBuilder::new_anonymous("Stream Handoff Token Drop Test")
        .application_uri("urn:stream-handoff-token-drop-test")
        .product_uri("urn:stream-handoff-token-drop-test")
        .host("127.0.0.1")
        .port(addr.port())
        .discovery_urls(vec![endpoint_url.clone()]);
    let (server, handle) = builder
        .build()
        .expect("stream handoff test server should build");
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let server_task = tokio::spawn(async move {
        server
            .run_with_streams(rx)
            .await
            .expect("server should run with handed-off streams");
    });

    let mut client = TcpStream::connect(addr)
        .await
        .expect("client should connect to external listener");
    let (socket, peer_addr) = handoff_listener
        .accept()
        .await
        .expect("external listener should accept client");
    let (token, token_drop_rx) = drop_token();
    tx.send((socket, peer_addr, token))
        .await
        .expect("accepted stream should hand off to server");

    let hello = HelloMessage::new(&endpoint_url, 65_535, 65_535, 0, 0);
    client
        .write_all(&SimpleBinaryEncodable::encode_to_vec(&hello))
        .await
        .expect("client should send HEL");

    let (read, write) = tokio::io::split(client);
    let mut framed = FramedRead::new(read, TcpCodec::new(DecodingOptions::default()));
    let message = tokio::time::timeout(Duration::from_secs(2), framed.next())
        .await
        .expect("server should respond to HEL");

    assert!(matches!(message, Some(Ok(Message::Acknowledge(_)))));

    drop(framed);
    drop(write);
    expect_token_drop(token_drop_rx, "closed connection").await;

    handle.cancel();
    server_task.abort();
}

#[tokio::test]
async fn run_with_streams_drops_owned_token_when_max_connections_rejects() {
    let handoff_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("handoff listener should bind");
    let addr = handoff_listener
        .local_addr()
        .expect("handoff listener should have address");
    let endpoint_url = format!("opc.tcp://127.0.0.1:{}/", addr.port());

    let builder = ServerBuilder::new_anonymous("Stream Handoff Max Connection Test")
        .application_uri("urn:stream-handoff-max-connection-test")
        .product_uri("urn:stream-handoff-max-connection-test")
        .host("127.0.0.1")
        .port(addr.port())
        .discovery_urls(vec![endpoint_url.clone()])
        .max_connections(1);
    let (server, handle) = builder
        .build()
        .expect("stream handoff test server should build");
    let (tx, rx) = tokio::sync::mpsc::channel(2);
    let server_task = tokio::spawn(async move {
        server
            .run_with_streams(rx)
            .await
            .expect("server should run with handed-off streams");
    });

    let mut first_client = TcpStream::connect(addr)
        .await
        .expect("first client should connect to external listener");
    let (first_socket, first_peer_addr) = handoff_listener
        .accept()
        .await
        .expect("external listener should accept first client");
    let (first_token, _first_token_drop_rx) = drop_token();
    tx.send((first_socket, first_peer_addr, first_token))
        .await
        .expect("first stream should hand off to server");

    let hello = HelloMessage::new(&endpoint_url, 65_535, 65_535, 0, 0);
    first_client
        .write_all(&SimpleBinaryEncodable::encode_to_vec(&hello))
        .await
        .expect("first client should send HEL");
    let (first_read, _first_write) = tokio::io::split(first_client);
    let mut first_framed = FramedRead::new(first_read, TcpCodec::new(DecodingOptions::default()));
    let message = tokio::time::timeout(Duration::from_secs(2), first_framed.next())
        .await
        .expect("server should respond to first HEL");
    assert!(matches!(message, Some(Ok(Message::Acknowledge(_)))));

    let _second_client = TcpStream::connect(addr)
        .await
        .expect("second client should connect to external listener");
    let (second_socket, second_peer_addr) = handoff_listener
        .accept()
        .await
        .expect("external listener should accept second client");
    let (second_token, second_token_drop_rx) = drop_token();
    tx.send((second_socket, second_peer_addr, second_token))
        .await
        .expect("second stream should hand off to server");

    expect_token_drop(second_token_drop_rx, "max-connections rejected").await;

    handle.cancel();
    server_task.abort();
}
