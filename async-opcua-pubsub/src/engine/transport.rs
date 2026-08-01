use opcua_types::StatusCode;

use crate::transport::udp::strip_udp_scheme;

fn has_scheme(address: &str, scheme: &str) -> bool {
    address
        .get(..scheme.len())
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme))
}

/// Supported OPC UA PubSub transport mappings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    /// MQTT broker transport.
    Mqtt,
    /// UDP multicast or unicast transport.
    Udp,
    /// AMQP broker transport.
    Amqp,
    /// WebSocket transport.
    WebSocket,
    /// TSN transport. Experimental, requires the `tsn` feature.
    #[cfg(feature = "tsn")]
    Tsn,
}

impl TransportKind {
    /// Classifies a PubSub connection address by URI scheme.
    pub fn from_address(address: &str) -> Result<Self, StatusCode> {
        let address = address.trim();

        if has_scheme(address, "mqtts://") {
            return Err(StatusCode::BadNotSupported);
        }

        if has_scheme(address, "mqtt://") {
            return Ok(Self::Mqtt);
        }

        if strip_udp_scheme(address).is_some() {
            return Ok(Self::Udp);
        }

        if has_scheme(address, "tsn://") {
            #[cfg(feature = "tsn")]
            return Ok(Self::Tsn);

            #[cfg(not(feature = "tsn"))]
            return Err(StatusCode::BadNotSupported);
        }

        if has_scheme(address, "amqp://") || has_scheme(address, "amqps://") {
            return Ok(Self::Amqp);
        }

        if has_scheme(address, "ws://") || has_scheme(address, "wss://") {
            return Ok(Self::WebSocket);
        }

        Err(StatusCode::BadInvalidArgument)
    }
}
