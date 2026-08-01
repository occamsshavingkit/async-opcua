//! Regression tests for secret-free WebSocket warning fields.

use std::fmt;

use opcua_core::sync::RwLock;
use opcua_pubsub::{MessageEncoding, WebSocketPublisher};
use opcua_server::address_space::AddressSpace;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    subscriber::{Interest, NoSubscriber},
    Dispatch, Event, Level, Metadata, Subscriber,
};

const CONNECT_WARNING: &str = "failed to connect WebSocket publisher";
const INVALID_DESTINATION_WARNING: &str = "invalid WebSocket PubSub destination";
const MAX_HANDSHAKE_REQUEST_BYTES: usize = 8 * 1024;
const PUBLISHER_TARGET: &str = "opcua_pubsub::transport::websocket";

struct WarnEventCapture {
    sender: mpsc::UnboundedSender<Vec<(String, String)>>,
}

#[derive(Default)]
struct EventFieldVisitor {
    fields: Vec<(String, String)>,
}

impl EventFieldVisitor {
    fn value(&self, name: &str) -> Option<&str> {
        self.fields
            .iter()
            .find_map(|(field, value)| (field == name).then_some(value.as_str()))
    }
}

impl Visit for EventFieldVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.fields
            .push((field.name().to_owned(), format!("{value:?}")));
    }

    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .push((field.name().to_owned(), value.to_owned()));
    }
}

impl Subscriber for WarnEventCapture {
    fn register_callsite(&self, _metadata: &'static Metadata<'static>) -> Interest {
        Interest::sometimes()
    }

    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() == &Level::WARN && metadata.target() == PUBLISHER_TARGET
    }

    fn new_span(&self, _attributes: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut visitor = EventFieldVisitor::default();
        event.record(&mut visitor);
        let _ = self.sender.send(visitor.fields);
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

async fn receive_warning(
    receiver: &mut mpsc::UnboundedReceiver<Vec<(String, String)>>,
) -> EventFieldVisitor {
    let fields = tokio::time::timeout(std::time::Duration::from_secs(2), receiver.recv())
        .await
        .expect("WebSocket warning was not captured before the deadline")
        .expect("WebSocket warning channel closed unexpectedly");
    EventFieldVisitor { fields }
}

fn assert_fields_exclude_destination_data(
    visitor: &EventFieldVisitor,
    destination: &str,
    secret_markers: &[&str],
) {
    assert!(
        visitor.fields.iter().all(|(_, value)| {
            secret_markers.iter().all(|secret| !value.contains(secret))
                && !value.contains('@')
                && !value.contains(destination)
        }),
        "captured WebSocket warning exposed destination data: {:?}",
        visitor.fields
    );
}

#[tokio::test(flavor = "current_thread")]
async fn connection_warning_excludes_destination_secrets() {
    // Given: a scoped subscriber and a local endpoint that rejects the WebSocket handshake.
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let _callsite_cache_sentinel = Dispatch::new(NoSubscriber::new());
    let _subscriber_guard = tracing::subscriber::set_default(WarnEventCapture { sender });
    let user_marker = "websocket-user-marker";
    let password_marker = "websocket-password-marker";
    let token_marker = "websocket-query-token-marker";
    let fragment_marker = "websocket-fragment-marker";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("local WebSocket test listener should bind");
    let endpoint = listener
        .local_addr()
        .expect("local WebSocket test listener should expose its address");
    let reject_connection = tokio::spawn(async move {
        let (mut stream, _) = listener
            .accept()
            .await
            .expect("local WebSocket test listener should accept the publisher connection");

        let request_target = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            let mut request = Vec::with_capacity(1024);
            let mut buffer = [0_u8; 1024];

            loop {
                let remaining = MAX_HANDSHAKE_REQUEST_BYTES
                    .checked_sub(request.len())
                    .expect("WebSocket handshake request should remain within the fixture limit");
                assert!(
                    remaining > 0,
                    "WebSocket handshake request exceeded the fixture limit"
                );
                let chunk_size = remaining.min(buffer.len());
                let bytes_read = stream
                    .read(&mut buffer[..chunk_size])
                    .await
                    .expect("local WebSocket test listener should read the handshake");
                assert_ne!(
                    bytes_read, 0,
                    "WebSocket handshake ended before the HTTP headers completed"
                );
                request.extend_from_slice(&buffer[..bytes_read]);

                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }

            let request_line_end = request
                .windows(2)
                .position(|window| window == b"\r\n")
                .expect("WebSocket handshake should contain an HTTP request line");
            std::str::from_utf8(&request[..request_line_end])
                .expect("WebSocket handshake request line should be UTF-8")
                .split_ascii_whitespace()
                .nth(1)
                .expect("WebSocket handshake request line should contain a request target")
                .to_owned()
        })
        .await
        .expect("WebSocket handshake request should arrive before the fixture deadline");
        let response_head = format!(
            "HTTP/1.1 400 Bad Request\r\nX-Rejected-Target: {request_target}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            request_target.len()
        );
        stream
            .write_all(response_head.as_bytes())
            .await
            .expect("local WebSocket test listener should reject the handshake");
        stream
            .write_all(request_target.as_bytes())
            .await
            .expect("local WebSocket test listener should reflect the request target");
    });
    let destination = format!(
        "ws://{user_marker}:{password_marker}@{endpoint}/opcua?token={token_marker}#{fragment_marker}"
    );
    let publisher = WebSocketPublisher::new(std::sync::Arc::new(RwLock::new(AddressSpace::new())));

    // When: the real publisher reports the rejected connection.
    publisher.publish_immediate(vec![1, 2, 3], &destination, &MessageEncoding::Uadp);
    tokio::time::timeout(std::time::Duration::from_secs(2), reject_connection)
        .await
        .expect("WebSocket publisher should connect to the local test listener")
        .expect("local WebSocket test listener should complete successfully");
    let visitor = receive_warning(&mut receiver).await;

    // Then: every field is secret-free and the endpoint remains useful to operators.
    assert_fields_exclude_destination_data(
        &visitor,
        &destination,
        &[user_marker, password_marker, token_marker, fragment_marker],
    );
    assert!(
        visitor
            .value("message")
            .is_some_and(|message| message.contains(CONNECT_WARNING)),
        "captured WebSocket warning did not contain the expected message"
    );
    assert_eq!(
        visitor.value("websocket_endpoint"),
        Some(format!("ws://127.0.0.1:{}", endpoint.port()).as_str()),
        "captured WebSocket warning omitted the sanitized endpoint"
    );
    assert!(
        visitor
            .value("error")
            .is_some_and(|error| !error.is_empty()),
        "captured WebSocket warning omitted the connection error"
    );
    assert_eq!(
        visitor.value("error"),
        Some("HTTP error: 400 Bad Request"),
        "captured WebSocket warning used the untrusted Debug representation"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn malformed_destination_warning_excludes_raw_address() {
    // Given: a scoped subscriber and a malformed destination containing secret markers.
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let _callsite_cache_sentinel = Dispatch::new(NoSubscriber::new());
    let _subscriber_guard = tracing::subscriber::set_default(WarnEventCapture { sender });
    let user_marker = "malformed-user-marker";
    let password_marker = "malformed-password-marker";
    let token_marker = "malformed-query-token-marker";
    let fragment_marker = "malformed-fragment-marker";
    let destination = format!(
        "ws://{user_marker}:{password_marker}@/opcua?token={token_marker}#{fragment_marker}"
    );
    let publisher = WebSocketPublisher::new(std::sync::Arc::new(RwLock::new(AddressSpace::new())));

    // When: the real publisher rejects the malformed destination.
    publisher.publish_immediate(vec![1, 2, 3], &destination, &MessageEncoding::Uadp);
    let visitor = receive_warning(&mut receiver).await;

    // Then: the warning identifies the failure without echoing untrusted destination data.
    assert_fields_exclude_destination_data(
        &visitor,
        &destination,
        &[user_marker, password_marker, token_marker, fragment_marker],
    );
    assert!(
        visitor
            .value("message")
            .is_some_and(|message| message.contains(INVALID_DESTINATION_WARNING)),
        "captured WebSocket warning did not identify an invalid destination"
    );
}
