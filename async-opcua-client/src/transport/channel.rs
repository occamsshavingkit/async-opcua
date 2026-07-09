use std::{
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use crate::{
    session::{process_unexpected_response, EndpointInfo},
    transport::core::TransportPollResult,
};
use arc_swap::{ArcSwap, ArcSwapOption};
use opcua_core::{
    comms::secure_channel::{Role, SecureChannel},
    sync::RwLock,
    trace_read_lock, trace_write_lock, RequestMessage, ResponseMessage,
};
use opcua_crypto::{CertificateStore, PrivateKey, SecurityPolicy, X509};
use opcua_types::{
    ByteString, CloseSecureChannelRequest, ContextOwned, Error, IntegerId, NodeId, RequestHeader,
    SecurityTokenRequestType, StatusCode,
};
use tokio::sync::{mpsc::Sender, Notify};
use tracing::{debug, debug_span, error, Instrument};

use super::{
    connect::{Connector, Transport},
    state::{Request, RequestSend, SecureChannelState},
};

use crate::{
    retry::SessionRetryPolicy,
    transport::{tcp::TransportConfiguration, OutgoingMessage},
};

// This is an arbitrary limit which should never be reached in practice,
// it's just a safety net to prevent the client from consuming too much
// memory if it gets into an unexpected (bad) state.
const MAX_INFLIGHT_MESSAGES: usize = 1_024;

struct RenewGuard<'a> {
    renewing: &'a AtomicBool,
    notify: &'a Notify,
}

impl Drop for RenewGuard<'_> {
    fn drop(&mut self) {
        self.renewing.store(false, Ordering::Release);
        self.notify.notify_waiters();
    }
}

/// Wrapper around an open secure channel
pub struct AsyncSecureChannel {
    endpoint_info: EndpointInfo,
    session_retry_policy: SessionRetryPolicy,
    pub(crate) secure_channel: Arc<RwLock<SecureChannel>>,
    certificate_store: Arc<RwLock<CertificateStore>>,
    transport_config: TransportConfiguration,
    state: Arc<SecureChannelState>,
    issue_channel_lock: Arc<Notify>,
    channel_is_renewing: AtomicBool,
    channel_lifetime: u32,
    request_timeout: Duration,

    request_send: ArcSwapOption<RequestSend>,
    encoding_context: Arc<RwLock<ContextOwned>>,
}

/// Event loop for a secure channel. This must be polled to make progress.
pub struct SecureChannelEventLoop<T> {
    transport: T,
}

impl<T: Transport + Send + Sync + 'static> SecureChannelEventLoop<T> {
    /// Poll the channel, processing any pending incoming or outgoing messages and returning the
    /// action that was taken.
    pub async fn poll(&mut self) -> TransportPollResult {
        self.transport.poll().await
    }

    /// Get the URL of the connected server.
    /// This was either the URL used to establish the connection, or the URL
    /// reported by the server in ReverseHello.
    pub fn connected_url(&self) -> &str {
        self.transport.connected_url()
    }
}

impl AsyncSecureChannel {
    pub(crate) fn make_request_header(&self, timeout: Duration) -> RequestHeader {
        self.state.make_request_header(timeout)
    }

    /// Get the next request handle on the channel.
    pub fn request_handle(&self) -> IntegerId {
        self.state.request_handle()
    }

    pub(crate) fn update_from_created_session(
        &self,
        nonce: &ByteString,
        certificate: &ByteString,
        auth_token: &NodeId,
    ) -> Result<(), Error> {
        let mut secure_channel = trace_write_lock!(self.secure_channel);
        secure_channel.set_remote_nonce_from_byte_string(nonce)?;
        secure_channel.set_remote_cert_from_byte_string(certificate)?;
        self.set_auth_token(auth_token.clone());
        Ok(())
    }

    pub(crate) fn update_from_activated_session(
        &self,
        server_nonce: &ByteString,
    ) -> Result<(), Error> {
        let mut secure_channel = trace_write_lock!(self.secure_channel);
        secure_channel.set_remote_nonce_from_byte_string(server_nonce)
    }

    pub(crate) fn security_policy(&self) -> SecurityPolicy {
        let secure_channel = trace_read_lock!(self.secure_channel);
        secure_channel.security_policy()
    }

    /// Get the target endpoint of the secure channel.
    pub fn endpoint_info(&self) -> &EndpointInfo {
        &self.endpoint_info
    }

    /// Get the current global encoding context in use by this channel.
    pub fn encoding_context(&self) -> &RwLock<ContextOwned> {
        &self.encoding_context
    }

    /// Set the active authentication token for this channel.
    pub fn set_auth_token(&self, token: NodeId) {
        self.state.set_auth_token(token);
    }

    pub(crate) fn read_own_private_key(&self) -> Option<PrivateKey> {
        let cert_store = trace_read_lock!(self.certificate_store);
        cert_store.read_own_pkey().ok()
    }

    pub(crate) fn read_own_certificate(&self) -> Option<X509> {
        let cert_store = trace_read_lock!(self.certificate_store);
        cert_store.read_own_cert().ok()
    }

    pub(crate) fn remote_certificate(&self) -> Option<X509> {
        let secure_channel = trace_read_lock!(self.secure_channel);
        secure_channel.remote_cert()
    }

    pub(crate) fn certificate_store(&self) -> &RwLock<CertificateStore> {
        &self.certificate_store
    }
}

impl AsyncSecureChannel {
    /// Create a new client secure channel.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        certificate_store: Arc<RwLock<CertificateStore>>,
        endpoint_info: EndpointInfo,
        session_retry_policy: SessionRetryPolicy,
        ignore_clock_skew: bool,
        allow_legacy_crypto: bool,
        auth_token: Arc<ArcSwap<NodeId>>,
        transport_config: TransportConfiguration,
        channel_lifetime: u32,
        request_timeout: Duration,
        encoding_context: Arc<RwLock<ContextOwned>>,
    ) -> Self {
        let mut secure_channel = SecureChannel::new(
            certificate_store.clone(),
            Role::Client,
            encoding_context.clone(),
        );
        secure_channel.set_allow_deprecated(allow_legacy_crypto);
        let secure_channel = Arc::new(RwLock::new(secure_channel));

        Self {
            transport_config,
            issue_channel_lock: Arc::new(Notify::new()),
            channel_is_renewing: AtomicBool::new(false),
            state: Arc::new(SecureChannelState::new(
                ignore_clock_skew,
                secure_channel.clone(),
                auth_token,
            )),
            endpoint_info,
            secure_channel,
            certificate_store,
            session_retry_policy,
            request_send: Default::default(),
            channel_lifetime,
            request_timeout,
            encoding_context,
        }
    }

    fn secure_channel_request_timeout(&self) -> Duration {
        self.request_timeout
            .min(Duration::from_millis(self.channel_lifetime as u64))
    }

    async fn renew_secure_channel(&self, send: Sender<OutgoingMessage>) -> Result<(), Error> {
        if self
            .channel_is_renewing
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            self.issue_channel_lock.notified().await;
            while self.channel_is_renewing.load(Ordering::Acquire) {
                self.issue_channel_lock.notified().await;
            }
            return Ok(());
        }

        let guard = RenewGuard {
            renewing: &self.channel_is_renewing,
            notify: &self.issue_channel_lock,
        };

        let should_renew_security_token = {
            let secure_channel = trace_read_lock!(self.secure_channel);
            secure_channel.should_renew_security_token()
        };

        if should_renew_security_token {
            let request = self.state.begin_issue_or_renew_secure_channel(
                SecurityTokenRequestType::Renew,
                self.channel_lifetime,
                self.secure_channel_request_timeout(),
                send,
            )?;

            let resp = match request.send().await {
                Ok(resp) => resp,
                Err(err) => {
                    self.close_channel().await;
                    return Err(err);
                }
            };

            if !matches!(resp, ResponseMessage::OpenSecureChannel(_)) {
                return Err(process_unexpected_response(resp));
            }
        }

        drop(guard);
        Ok(())
    }

    /// Send a message on the secure channel, and wait for a response.
    pub async fn send(
        &self,
        request: impl Into<RequestMessage>,
        timeout: Duration,
    ) -> Result<ResponseMessage, Error> {
        let sender = self.request_send.load().as_deref().cloned();
        let Some(send) = sender else {
            return Err(Error::new(
                StatusCode::BadNotConnected,
                "Unable to send request, transport is not connected",
            ));
        };

        let should_renew_security_token = {
            let secure_channel = trace_read_lock!(self.secure_channel);
            secure_channel.should_renew_security_token()
        };

        if should_renew_security_token {
            self.renew_secure_channel(send.clone())
                .instrument(debug_span!("Renewing security token"))
                .await?;
        }

        let sender = self.request_send.load().as_deref().cloned();
        let send = sender.ok_or_else(|| {
            Error::new(
                StatusCode::BadNotConnected,
                "Unable to send request, transport is not connected",
            )
        })?;

        Request::new(request, send, timeout)
            .send()
            .in_current_span()
            .await
    }

    /// Attempt to establish a connection using this channel, returning an event loop
    /// for polling the connection.
    pub async fn connect<T: Connector>(
        &self,
        connector: &T,
    ) -> Result<SecureChannelEventLoop<T::Transport>, Error> {
        self.request_send.store(None);
        let mut backoff = self.session_retry_policy.new_backoff();
        loop {
            match self.connect_no_retry(connector).await {
                Ok(event_loop) => {
                    let security_policy = self.secure_channel.read().security_policy();
                    if security_policy.is_deprecated() {
                        tracing::warn!(
                            "Connected using deprecated security policy {security_policy}. \
                             This is allowed by allow_legacy_crypto but should be migrated."
                        );
                    }
                    break Ok(event_loop);
                }
                Err(s) => {
                    let Some(delay) = backoff.next() else {
                        break Err(s);
                    };

                    tokio::time::sleep(delay).await
                }
            }
        }
    }

    /// Connect to the server without attempting to retry if it fails.
    pub async fn connect_no_retry<T: Connector>(
        &self,
        connector: &T,
    ) -> Result<SecureChannelEventLoop<T::Transport>, Error> {
        {
            let mut secure_channel = trace_write_lock!(self.secure_channel);
            secure_channel.clear_security_token();
        }

        let (mut transport, send) = self.create_transport(connector).await?;

        let request = self.state.begin_issue_or_renew_secure_channel(
            SecurityTokenRequestType::Issue,
            self.channel_lifetime,
            Duration::from_secs(30),
            send.clone(),
        )?;

        let request_fut = request
            .send()
            .instrument(debug_span!("Create a secure channel"));
        tokio::pin!(request_fut);

        // Temporarily poll the transport task while we're waiting for a response.
        let resp = loop {
            tokio::select! {
                biased;
                r = &mut request_fut => break r?,
                r = transport.poll() => {
                    if let TransportPollResult::Closed(e) = r {
                        return Err(Error::new(e, "Transport closed unexpectedly"));
                    }
                }
            }
        };

        self.request_send.store(Some(Arc::new(send)));
        if !matches!(resp, ResponseMessage::OpenSecureChannel(_)) {
            return Err(process_unexpected_response(resp));
        }

        Ok(SecureChannelEventLoop { transport })
    }

    async fn create_transport<T: Connector>(
        &self,
        connector: &T,
    ) -> Result<(T::Transport, tokio::sync::mpsc::Sender<OutgoingMessage>), Error> {
        debug!("Connect");
        let security_policy =
            SecurityPolicy::from_str(self.endpoint_info.endpoint.security_policy_uri.as_ref())
                .map_err(|_| {
                    Error::new(
                        StatusCode::BadSecurityPolicyRejected,
                        "Unknown security policy",
                    )
                })?;

        if security_policy == SecurityPolicy::Unknown {
            error!(
                "connect, security policy \"{}\" is unknown",
                self.endpoint_info.endpoint.security_policy_uri.as_ref()
            );
            Err(Error::new(
                StatusCode::BadSecurityPolicyRejected,
                format!(
                    "security policy \"{}\" is unknown",
                    self.endpoint_info.endpoint.security_policy_uri
                ),
            ))
        } else {
            let (cert, key) = {
                let certificate_store = trace_read_lock!(self.certificate_store);
                (
                    certificate_store.read_own_cert().ok(),
                    certificate_store.read_own_pkey().ok(),
                )
            };

            {
                let mut secure_channel = trace_write_lock!(self.secure_channel);
                secure_channel.set_private_key(key);
                secure_channel.set_cert(cert);
                secure_channel.set_security_policy(security_policy);
                secure_channel.set_security_mode(self.endpoint_info.endpoint.security_mode);
                secure_channel.set_remote_cert_from_byte_string(
                    &self.endpoint_info.endpoint.server_certificate,
                )?;
                debug!("Security policy = {:?}", security_policy);
                debug!(
                    "Security mode = {:?}",
                    self.endpoint_info.endpoint.security_mode
                );
            }

            let (send, recv) = tokio::sync::mpsc::channel(MAX_INFLIGHT_MESSAGES);
            let transport = connector
                .connect(self.state.clone(), recv, self.transport_config.clone())
                .await?;

            Ok((transport, send))
        }
    }

    #[cfg(test)]
    pub(crate) fn test_set_channel_is_renewing(&self, value: bool) {
        self.channel_is_renewing.store(value, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn test_notify_renewal_complete(&self) {
        self.issue_channel_lock.notify_waiters();
    }

    /// Close the secure channel.
    ///
    /// This is a fire-and-forget best-effort close: the `CloseSecureChannelRequest`
    /// is sent via `send_no_response()` which does not wait for a reply. Errors
    /// are logged but not propagated to the caller.
    pub async fn close_channel(&self) {
        let msg = CloseSecureChannelRequest {
            request_header: self.state.make_request_header(Duration::from_secs(60)),
        };

        let sender = self.request_send.load().as_deref().cloned();
        let request = sender.map(|s| Request::new(msg, s, Duration::from_secs(60)));

        // Instruct the channel to not attempt to reopen.
        if let Some(request) = request {
            if let Err(e) = request
                .send_no_response()
                .instrument(debug_span!("Sending close channel message"))
                .await
            {
                error!("Failed to send disconnect message, queue full: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{retry::SessionRetryPolicy, IdentityToken};
    use opcua_crypto::SecurityPolicy;
    use opcua_types::{
        ApplicationDescription, ApplicationType, EndpointDescription, MessageSecurityMode,
        UAString, UserTokenPolicy,
    };
    use std::{sync::Arc, time::Duration as StdDuration};

    fn test_endpoint(security_mode: MessageSecurityMode) -> EndpointInfo {
        EndpointInfo {
            endpoint: EndpointDescription {
                endpoint_url: UAString::from("opc.tcp://127.0.0.1:4840"),
                server: ApplicationDescription {
                    application_uri: UAString::from("urn:test:server"),
                    application_type: ApplicationType::Server,
                    ..Default::default()
                },
                security_mode,
                security_policy_uri: SecurityPolicy::None.to_uri().to_string().into(),
                user_identity_tokens: Some(vec![UserTokenPolicy::anonymous()]),
                transport_profile_uri: UAString::null(),
                security_level: 0,
                server_certificate: opcua_types::ByteString::null(),
            },
            user_identity_token: IdentityToken::Anonymous,
            preferred_locales: Vec::new(),
        }
    }

    fn test_channel() -> AsyncSecureChannel {
        AsyncSecureChannel::new(
            Arc::new(RwLock::new(CertificateStore::new(std::path::Path::new(
                "./pki",
            )))),
            test_endpoint(MessageSecurityMode::None),
            SessionRetryPolicy::never(),
            false,
            false,
            Arc::new(ArcSwap::new(Arc::new(opcua_types::NodeId::null()))),
            TransportConfiguration {
                send_buffer_size: 65535,
                recv_buffer_size: 65535,
                max_message_size: 2 * 1024 * 1024,
                max_chunk_count: 8,
                connect_timeout: Duration::from_secs(5),
                tcp_keepalive: Default::default(),
            },
            60_000,
            Duration::from_secs(10),
            Arc::new(RwLock::new(ContextOwned::default())),
        )
    }

    #[tokio::test]
    async fn concurrent_renewal_does_not_deadlock() {
        let channel = test_channel();
        {
            let mut sc = channel.secure_channel.write();
            sc.set_token_lifetime(100);
            sc.set_token_created_at(
                opcua_types::DateTime::now() - chrono::Duration::milliseconds(200),
            );
        }

        let (send, _recv) = tokio::sync::mpsc::channel::<OutgoingMessage>(1);
        channel.request_send.store(Some(Arc::new(send.clone())));

        channel.test_set_channel_is_renewing(true);

        let ch = std::sync::Arc::new(channel);
        let ch2 = ch.clone();

        let waiter = tokio::spawn(async move { ch2.renew_secure_channel(send).await });

        tokio::time::sleep(StdDuration::from_millis(50)).await;

        ch.test_set_channel_is_renewing(false);
        ch.test_notify_renewal_complete();

        tokio::time::timeout(StdDuration::from_secs(2), waiter)
            .await
            .expect("concurrent renewal should not timeout")
            .expect("renew_secure_channel should succeed")
            .expect("renewal should return Ok");
    }

    #[tokio::test]
    async fn send_reloads_request_send_after_renewal_block() {
        let channel = test_channel();
        {
            let mut sc = channel.secure_channel.write();
            sc.set_token_id(1);
            sc.set_token_lifetime(100);
            sc.set_token_created_at(
                opcua_types::DateTime::now() - chrono::Duration::milliseconds(200),
            );
        }

        let (send_a, _recv_a) = tokio::sync::mpsc::channel::<OutgoingMessage>(1);
        channel.request_send.store(Some(Arc::new(send_a)));
        channel.test_set_channel_is_renewing(true);

        let (send_b, mut recv_b) = tokio::sync::mpsc::channel::<OutgoingMessage>(1);

        let ch = std::sync::Arc::new(channel);
        let ch2 = ch.clone();

        let request = opcua_types::GetEndpointsRequest {
            request_header: opcua_types::RequestHeader::new(
                &opcua_types::NodeId::null(),
                &opcua_types::DateTime::now(),
                0,
            ),
            endpoint_url: opcua_types::UAString::null(),
            locale_ids: None,
            profile_uris: None,
        };

        let send_handle =
            tokio::spawn(async move { ch2.send(request, StdDuration::from_secs(10)).await });

        tokio::time::sleep(StdDuration::from_millis(50)).await;
        ch.request_send.store(Some(Arc::new(send_b)));
        ch.test_set_channel_is_renewing(false);
        ch.test_notify_renewal_complete();

        let msg = tokio::time::timeout(StdDuration::from_secs(2), recv_b.recv())
            .await
            .expect("send() must use the reloaded request_send; recv_b never got the message")
            .expect("reloaded sender channel must be open");
        drop(msg);

        send_handle.abort();
    }
}
