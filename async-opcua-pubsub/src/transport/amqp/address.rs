use std::str::FromStr;

use lapin::uri::AMQPUri;
use opcua_types::StatusCode;

use super::{AmqpAddressSettings, DEFAULT_AMQP_PORT, DEFAULT_ROUTING_KEY};

/// Parses an AMQP address into broker URL + routing key, applying sensible
/// defaults for the port and routing key when the caller omits them.
pub(crate) fn parse_amqp_address(address: &str) -> Result<AmqpAddressSettings, StatusCode> {
    let (scheme, addr) = if address
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("amqp://"))
    {
        ("amqp", &address[7..])
    } else if address
        .get(..8)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("amqps://"))
    {
        ("amqps", &address[8..])
    } else {
        ("amqp", address)
    };

    let default_port = if scheme == "amqps" {
        5671
    } else {
        DEFAULT_AMQP_PORT
    };
    let (authority, routing_key) = addr.split_once('/').unwrap_or((addr, ""));
    let authority = if authority.is_empty() {
        format!("127.0.0.1:{default_port}")
    } else if authority_has_port(authority) {
        authority.to_string()
    } else {
        format!("{authority}:{default_port}")
    };

    let broker_url = format!("{scheme}://{authority}");
    AMQPUri::from_str(&broker_url).map_err(|_| StatusCode::BadConfigurationError)?;

    Ok(AmqpAddressSettings {
        broker_url,
        routing_key: if routing_key.is_empty() {
            DEFAULT_ROUTING_KEY.to_string()
        } else {
            routing_key.to_string()
        },
    })
}

fn authority_has_port(authority: &str) -> bool {
    let host_port = authority.rsplit('@').next().unwrap_or(authority);
    if let Some(rest) = host_port.strip_prefix('[') {
        return rest
            .find(']')
            .is_some_and(|end| rest[end + 1..].starts_with(':'));
    }
    host_port.rsplit_once(':').is_some()
}
