use std::fmt;

use opcua_core::sync::RwLock;
use opcua_server::address_space::AddressSpace;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    subscriber::{Interest, NoSubscriber},
    Dispatch, Event, Level, Metadata, Subscriber,
};

use crate::{PubSubConnectionConfig, PubSubPublisher};

use super::super::{parse_amqp_address, AmqpPublisher};

const CONNECT_WARNING: &str = "failed to connect AMQP publisher";
const PUBLISHER_TARGET: &str = "opcua_pubsub::transport::amqp::publisher";

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
        if visitor
            .value("message")
            .is_some_and(|message| message.contains(CONNECT_WARNING))
        {
            let _ = self.sender.send(visitor.fields);
        }
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

#[tokio::test(flavor = "current_thread")]
async fn connection_warning_redacts_broker_credentials() {
    // Given: a thread-local subscriber and a controlled rejecting broker with userinfo.
    let (sender, mut receiver) = mpsc::unbounded_channel();
    // Keep a second dispatch live so tracing-core cannot cache a sibling thread's
    // NoSubscriber as Interest::never for this static warning callsite.
    let _callsite_cache_sentinel = Dispatch::new(NoSubscriber::new());
    let _subscriber_guard = tracing::subscriber::set_default(WarnEventCapture { sender });
    let user_marker = "amqp-user-marker";
    let secret_marker = "amqp-secret-marker";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("local AMQP test listener should bind");
    let broker_address = listener
        .local_addr()
        .expect("local AMQP test listener should expose its address");
    let reject_connection = tokio::spawn(async move {
        let (mut stream, _) = listener
            .accept()
            .await
            .expect("local AMQP test listener should accept the publisher connection");
        stream
            .write_all(b"AMQP\0\0\x09\x02")
            .await
            .expect("local AMQP test listener should send an unsupported protocol header");
    });
    let address = format!("amqp://{user_marker}:{secret_marker}@{broker_address}/redaction");
    let full_broker_url = parse_amqp_address(&address)
        .expect("credential-bearing test address should parse")
        .broker_url;
    let publisher = AmqpPublisher::new(std::sync::Arc::new(RwLock::new(AddressSpace::new())));
    let cancel_token = CancellationToken::new();
    let coordinator = publisher
        .start_publishing(
            PubSubConnectionConfig {
                connection_id: "amqp-log-redaction".to_owned(),
                name: "AMQP log redaction".to_owned(),
                address: address.clone(),
                writer_groups: Vec::new(),
                reader_groups: Vec::new(),
            },
            cancel_token.clone(),
        )
        .expect("AMQP publisher should start");

    // When: the spawned transport reports its failed connection attempt.
    tokio::time::timeout(std::time::Duration::from_secs(2), reject_connection)
        .await
        .expect("AMQP publisher should connect to the local test listener")
        .expect("local AMQP test listener should complete successfully");
    let captured = tokio::time::timeout(std::time::Duration::from_secs(2), receiver.recv()).await;
    cancel_token.cancel();
    tokio::time::timeout(std::time::Duration::from_secs(2), coordinator)
        .await
        .expect("AMQP publisher should stop before the deadline")
        .expect("AMQP publisher should stop successfully");
    let fields = captured
        .expect("AMQP connection warning was not captured before the deadline")
        .expect("AMQP connection warning channel closed unexpectedly");
    let visitor = EventFieldVisitor { fields };

    // Then: every captured field is credential-free while useful endpoint context remains.
    assert!(
        visitor
            .value("message")
            .is_some_and(|message| message.contains(CONNECT_WARNING)),
        "captured AMQP warning did not contain the expected message"
    );
    let endpoint = visitor
        .value("broker_endpoint")
        .expect("captured AMQP warning omitted the sanitized endpoint field");
    assert!(
        endpoint.contains("amqp://")
            && endpoint.contains("127.0.0.1")
            && endpoint.contains(&format!(":{}", broker_address.port())),
        "sanitized AMQP endpoint omitted scheme, host, or port"
    );
    assert!(
        visitor
            .value("error")
            .is_some_and(|error| !error.is_empty()),
        "captured AMQP warning omitted the connection error field"
    );
    assert!(
        visitor.fields.iter().all(|(_, value)| {
            !value.contains(user_marker)
                && !value.contains(secret_marker)
                && !value.contains('@')
                && !value.contains(&full_broker_url)
                && !value.contains(&address)
        }),
        "captured AMQP warning exposed credential-bearing broker data"
    );
}
