use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr, ToSocketAddrs},
    sync::{
        atomic::{AtomicU16, AtomicU8},
        Arc,
    },
    time::Duration,
};

use futures::{future::Either, never::Never, stream::FuturesUnordered, FutureExt, StreamExt};
use opcua_core::{sync::RwLock, trace_read_lock, trace_write_lock};
use opcua_nodes::DefaultTypeTree;
use tokio::{
    net::{TcpListener, TcpStream},
    pin,
    sync::{mpsc, Notify},
    task::{JoinError, JoinHandle},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use opcua_core::{config::Config, handle::AtomicHandle};
use opcua_crypto::{
    CertificateStore, PrivateKey, RevocationMode, SecurityPolicy, ValidationOptions, X509,
};

#[cfg(feature = "kerberos")]
use opcua_crypto::identity::GssapiIdentityValidator;

#[cfg(feature = "diagnostics")]
use crate::diagnostics::ServerDiagnostics;
use crate::metrics::ServerMetricsSnapshot;
#[cfg(feature = "wss")]
use crate::transport::WebSocketConnector;
use crate::{
    node_manager::{DefaultTypeTreeGetter, ServerContext},
    reverse_connect::{self, ReverseConnectionManager},
    session::controller_command::ControllerCommand,
    session::session_starter::SessionStarter,
    transport::{
        tcp::{TcpConnector, TransportConfig},
        ReverseTcpConnector,
    },
    ServerStatusWrapper,
};
use opcua_types::{DateTime, LocalizedText, ServerState, UAString};

#[cfg(feature = "subscriptions")]
use super::subscriptions::SubscriptionCache;
use super::{
    authenticator::DefaultAuthenticator,
    builder::ServerBuilder,
    config::{EndpointIdentifier, ServerConfig, TcpKeepaliveConfig},
    info::ServerInfo,
    node_manager::{NodeManagers, NodeManagersRef},
    server_handle::ServerHandle,
    session::manager::SessionManager,
    ServerCapabilities,
};

struct ConnectionInfo {
    command_send: tokio::sync::mpsc::Sender<ControllerCommand>,
    ip: IpAddr,
}

struct ConnectionSlots<'a> {
    connections: &'a mut FuturesUnordered<JoinHandle<u32>>,
    connection_map: &'a mut HashMap<u32, ConnectionInfo>,
}

#[cfg_attr(feature = "sharded", derive(Clone))]
struct TcpConnectionDeps {
    max_connections: usize,
    max_connections_per_ip: usize,
    transport_config: TransportConfig,
    info: Arc<ServerInfo>,
    session_manager: Arc<RwLock<SessionManager>>,
    certificate_store: Arc<RwLock<CertificateStore>>,
    node_managers: NodeManagers,
    #[cfg(feature = "subscriptions")]
    subscriptions: Arc<SubscriptionCache>,
}

#[derive(Clone)]
enum AcceptedTransport {
    Tcp,
    #[cfg(feature = "wss")]
    Wss(Arc<rustls::ServerConfig>),
}

fn configure_tcp_stream(stream: &TcpStream, addr: SocketAddr, tcp_keepalive: &TcpKeepaliveConfig) {
    if let Err(e) = stream.set_nodelay(true) {
        warn!("Failed to set TCP_NODELAY for {addr}: {e}");
    }
    if tcp_keepalive.enabled {
        let keepalive = socket2::TcpKeepalive::new()
            .with_time(Duration::from_secs(tcp_keepalive.idle_secs))
            .with_interval(Duration::from_secs(tcp_keepalive.interval_secs))
            .with_retries(tcp_keepalive.retries);
        if let Err(e) = socket2::SockRef::from(stream).set_tcp_keepalive(&keepalive) {
            warn!("Failed to set TCP keep-alive for {addr}: {e}");
        }
    }
}

impl TcpConnectionDeps {
    fn accept<T: Send + 'static>(
        &self,
        slots: &mut ConnectionSlots<'_>,
        socket: TcpStream,
        addr: SocketAddr,
        token: Option<T>,
        connection_counter: u32,
        transport: AcceptedTransport,
    ) -> bool {
        if slots.connection_map.len() >= self.max_connections {
            warn!(
                "Closing connection from {addr}: max_connections ({}) reached",
                self.max_connections
            );
            drop(socket);
            drop(token);
            return false;
        }
        let ip = addr.ip();
        if self.max_connections_per_ip > 0 {
            let connections_from_ip = slots
                .connection_map
                .values()
                .filter(|connection| connection.ip == ip)
                .count();
            if connections_from_ip >= self.max_connections_per_ip {
                warn!("Closing connection from {addr}: max_connections_per_ip reached");
                drop(socket);
                drop(token);
                return false;
            }
        }

        configure_tcp_stream(&socket, addr, &self.transport_config.tcp_keepalive);

        let (send, recv) = tokio::sync::mpsc::channel(5);
        info!("Accept new connection from {addr} ({connection_counter})");
        self.info.metrics.record_connection_accepted();
        let handle = match transport {
            AcceptedTransport::Tcp => {
                let conn = SessionStarter::new(
                    TcpConnector::new(
                        socket,
                        self.transport_config.clone(),
                        self.info.decoding_options(),
                    ),
                    self.info.clone(),
                    self.session_manager.clone(),
                    self.certificate_store.clone(),
                    self.node_managers.clone(),
                    #[cfg(feature = "subscriptions")]
                    self.subscriptions.clone(),
                );
                spawn_connection(conn, recv, token, connection_counter)
            }
            #[cfg(feature = "wss")]
            AcceptedTransport::Wss(tls_config) => {
                let conn = SessionStarter::new(
                    WebSocketConnector::new(
                        socket,
                        tls_config,
                        self.transport_config.clone(),
                        self.info.decoding_options(),
                    ),
                    self.info.clone(),
                    self.session_manager.clone(),
                    self.certificate_store.clone(),
                    self.node_managers.clone(),
                    #[cfg(feature = "subscriptions")]
                    self.subscriptions.clone(),
                );
                spawn_connection(conn, recv, token, connection_counter)
            }
        };
        slots.connections.push(handle);
        slots.connection_map.insert(
            connection_counter,
            ConnectionInfo {
                command_send: send,
                ip,
            },
        );
        true
    }
}

fn spawn_connection<C, T>(
    conn: SessionStarter<C>,
    recv: tokio::sync::mpsc::Receiver<ControllerCommand>,
    token: Option<T>,
    connection_counter: u32,
) -> JoinHandle<u32>
where
    C: crate::transport::Connector + Send + 'static,
    C::Transport: crate::transport::tcp::ConnectionTransport,
    T: Send + 'static,
{
    tokio::spawn(async move {
        let _token = token;
        // Catch panics so the task always yields its counter, otherwise
        // the connection_map slot leaks and permanently consumes
        // max_connections capacity.
        if let Err(payload) = std::panic::AssertUnwindSafe(conn.run(recv, |_| {}))
            .catch_unwind()
            .await
        {
            log_connection_panic(connection_counter, payload);
        }
        connection_counter
    })
}

fn log_connection_panic(connection_counter: u32, payload: Box<dyn std::any::Any + Send>) {
    let message = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown panic payload");
    error!("Connection task {connection_counter} panicked: {message}");
}

enum ConnectionSource<T> {
    Listener(TcpListener),
    Streams(mpsc::Receiver<(TcpStream, SocketAddr, T)>),
    Closed,
}

impl<T> ConnectionSource<T> {
    async fn next(&mut self) -> Option<Result<(TcpStream, SocketAddr, Option<T>), std::io::Error>> {
        match self {
            Self::Listener(listener) => Some(
                listener
                    .accept()
                    .await
                    .map(|(socket, addr)| (socket, addr, None)),
            ),
            Self::Streams(rx) => rx
                .recv()
                .await
                .map(|(socket, addr, token)| Ok((socket, addr, Some(token)))),
            Self::Closed => futures::future::pending().await,
        }
    }

    fn is_closed(&self) -> bool {
        matches!(self, Self::Closed)
    }
}

/// The server struct. This is consumed when run, so you will typically not hold onto this for longer
/// periods of time.
pub struct Server {
    /// Certificate store
    certificate_store: Arc<RwLock<CertificateStore>>,
    /// Session manager
    session_manager: Arc<RwLock<SessionManager>>,
    /// Open connections.
    connections: FuturesUnordered<JoinHandle<u32>>,
    /// Map to metadata about each open connection
    connection_map: HashMap<u32, ConnectionInfo>,
    /// Server configuration, fixed after the server is started
    config: Arc<ServerConfig>,
    /// Context for use by connections to access general server state.
    info: Arc<ServerInfo>,
    /// Subscription cache, global because subscriptions outlive sessions.
    #[cfg(feature = "subscriptions")]
    subscriptions: Arc<SubscriptionCache>,
    /// List of node managers
    node_managers: NodeManagers,
    /// Cancellation token
    token: CancellationToken,
    /// Notify that is woken up if a new session is added to the session manager.
    session_notify: Arc<Notify>,
    /// Wrapper managing the `ServerStatus` server variable.
    status: Arc<ServerStatusWrapper>,
    /// Manager for reverse connections. This does nothing unless users register
    /// reverse connect targets.
    reverse_connect_manager: ReverseConnectionManager,
}

impl Server {
    pub(crate) fn new_from_builder(builder: ServerBuilder) -> Result<(Self, ServerHandle), String> {
        if let Err(e) = builder.config.validate() {
            return Err(format!(
                "Builder configuration is invalid: {}",
                e.join(", ")
            ));
        }

        let mut config = builder.config;

        let application_name = config.application_name.clone();
        let application_uri = UAString::from(&config.application_uri);
        let product_uri = UAString::from(&config.product_uri);
        let servers = vec![config.application_uri.clone()];
        /* let base_endpoint = format!(
            "opc.tcp://{}:{}",
            config.tcp_config.host, config.tcp_config.port
        ); */

        // let diagnostics = Arc::new(RwLock::new(ServerDiagnostics::default()));
        let send_buffer_size = config.limits.send_buffer_size;
        let receive_buffer_size = config.limits.receive_buffer_size;

        let application_description = if config.create_sample_keypair {
            Some(config.application_description())
        } else {
            None
        };

        let (mut certificate_store, global_cert, global_pkey) =
            CertificateStore::new_with_x509_data(
                &config.pki_dir,
                false,
                config.certificate_path.as_deref(),
                config.private_key_path.as_deref(),
                application_description,
            );

        if global_cert.is_none() || global_pkey.is_none() {
            warn!(
                "Server is missing its application instance certificate and/or its private key. Encrypted endpoints will not function correctly."
            );
        }

        // T019: Per-endpoint cert loading
        let mut endpoint_certificates: HashMap<EndpointIdentifier, Option<(X509, PrivateKey)>> =
            HashMap::new();
        let server_pkey = RwLock::new(global_pkey.clone());

        let mut endpoint_futs: Vec<(
            EndpointIdentifier,
            Option<std::path::PathBuf>,
            Option<std::path::PathBuf>,
        )> = Vec::new();

        for endpoint in config.endpoints.values() {
            let endpoint_id = EndpointIdentifier::from(endpoint);
            if endpoint_certificates.contains_key(&endpoint_id) {
                continue;
            }

            let cert_path = endpoint
                .certificate_path
                .as_deref()
                .or(config.certificate_path.as_deref())
                .map(|cp| config.pki_dir.join(cp));
            let key_path = endpoint
                .private_key_path
                .as_deref()
                .or(config.private_key_path.as_deref())
                .map(|kp| config.pki_dir.join(kp));

            endpoint_futs.push((endpoint_id, cert_path, key_path));
        }

        // Parallelize cert + key file I/O across all endpoints.
        let results: Vec<(EndpointIdentifier, Option<(X509, PrivateKey)>)> = std::thread::scope(
            |s| {
                let mut handles = Vec::new();
                for (endpoint_id, cert_path, key_path) in endpoint_futs {
                    handles.push(s.spawn(move || {
                        let cert_entry = match (cert_path.as_deref(), key_path.as_deref()) {
                            (Some(cp), Some(kp)) => {
                                let cert = match CertificateStore::read_cert(cp) {
                                    Ok(cert) => cert,
                                    Err(e) => {
                                        warn!(
                                            "Endpoint '{}': failed to load certificate from {:?}: {e}",
                                            endpoint_id.path, cp
                                        );
                                        return (endpoint_id, None);
                                    }
                                };
                                let key = match CertificateStore::read_pkey(kp) {
                                    Ok(key) => key,
                                    Err(e) => {
                                        warn!(
                                            "Endpoint '{}': failed to load private key from {:?}: {e}",
                                            endpoint_id.path, kp
                                        );
                                        return (endpoint_id, None);
                                    }
                                };
                                (endpoint_id, Some((cert, key)))
                            }
                            _ => (endpoint_id, None),
                        };
                        cert_entry
                    }));
                }
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            },
        );

        for (endpoint_id, cert_entry) in results {
            let cert_entry = cert_entry.or_else(|| {
                global_cert
                    .as_ref()
                    .zip(global_pkey.as_ref())
                    .map(|(c, k)| (c.clone(), k.clone()))
            });
            endpoint_certificates.insert(endpoint_id, cert_entry);
        }

        // T020: Startup validation — ensure all security-policy endpoints have
        // compatible certificates
        for endpoint in config.endpoints.values() {
            let security_policy = endpoint.security_policy();
            let _security_mode = endpoint.message_security_mode();

            if security_policy == SecurityPolicy::None {
                continue;
            }

            let endpoint_id = EndpointIdentifier::from(endpoint);
            let cert_entry = endpoint_certificates
                .get(&endpoint_id)
                .and_then(|entry| entry.as_ref());

            match cert_entry {
                None => {
                    warn!(
                        "Endpoint '{}' uses security policy {} but no compatible certificate is configured. Clients connecting to this endpoint may fail.",
                        endpoint.path,
                        endpoint.security_policy
                    );
                }
                #[allow(unused_variables)]
                Some((cert, _key)) => {
                    let cert_is_ecc = {
                        #[cfg(feature = "ecc")]
                        {
                            cert.public_key()
                                .ok()
                                .and_then(|pk| pk.ecc_curve())
                                .is_some()
                        }
                        #[cfg(not(feature = "ecc"))]
                        {
                            false
                        }
                    };
                    let policy_is_ecc = security_policy.is_ecc();
                    if cert_is_ecc != policy_is_ecc {
                        let cert_type = if cert_is_ecc { "EC" } else { "RSA" };
                        let policy_type = if policy_is_ecc { "EC" } else { "RSA" };
                        error!(
                            "Endpoint '{}' uses security policy {} (requires {} key) but certificate is {}.",
                            endpoint.path,
                            endpoint.security_policy,
                            policy_type,
                            cert_type
                        );
                        return Err(format!(
                            "Endpoint '{}' uses security policy {} (requires {} key) but certificate is {}.",
                            endpoint.path,
                            endpoint.security_policy,
                            policy_type,
                            cert_type
                        ));
                    }
                }
            }
        }

        config.read_x509_thumbprints();

        if config.certificate_validation.trust_client_certs {
            info!(
                "Server has chosen to auto trust client certificates. You do not want to do this in production code."
            );
            certificate_store.set_trust_unknown_certs(true);
        }
        certificate_store.set_check_time(config.certificate_validation.check_time);
        certificate_store.set_validation_options(ValidationOptions {
            revocation_mode: if config.certificate_validation.require_revocation {
                RevocationMode::Required
            } else {
                RevocationMode::Lenient
            },
            ..Default::default()
        });

        #[cfg(feature = "rbac")]
        let role_resolver = {
            let mut role_resolver =
                crate::rbac::resolver::RoleResolver::from_user_tokens(&config.user_tokens);
            for mapping in &config.identity_mapping_rules {
                role_resolver.add_mapping(mapping.role_node_id.clone(), mapping.rule.clone());
            }
            role_resolver
        };
        #[cfg(not(feature = "rbac"))]
        let role_resolver = crate::rbac::resolver::RoleResolver;
        let namespace_defaults =
            crate::rbac::defaults::NamespaceDefaults::from_config(&config.namespace_defaults);
        let config = Arc::new(config);

        let service_level = Arc::new(AtomicU8::new(255));

        #[cfg(feature = "discovery-mdns")]
        let mdns = if config.multicast_discovery.enabled {
            let own_instance = config
                .multicast_discovery
                .mdns_server_name
                .clone()
                .unwrap_or_else(|| config.application_name.clone());
            Some(Arc::new(crate::discovery_mdns::MdnsDiscovery::new(
                own_instance,
            )))
        } else {
            None
        };

        #[cfg(all(feature = "lds", feature = "discovery-mdns"))]
        let registered_mdns = if config.multicast_discovery.enabled {
            match crate::discovery_mdns::MdnsAdvertisementRegistry::new() {
                Ok(registry) => Some(Arc::new(registry)),
                Err(e) => {
                    warn!("mDNS registered-server advertisements unavailable: {e}");
                    None
                }
            }
        } else {
            None
        };

        let type_tree = Arc::new(RwLock::new(DefaultTypeTree::new()));

        let certificate_store = Arc::new(RwLock::new(certificate_store));

        // Validate Kerberos configuration (OPC 10000-6 §6.4)
        #[cfg(feature = "kerberos")]
        if let Some(ref validator) = builder.kerberos_validator {
            if validator.spn().is_empty() {
                return Err("Kerberos SPN is empty — configure with kerberos_spn()".to_string());
            }
            if let Some(keytab_path) = validator.keytab_path() {
                if !keytab_path.exists() {
                    return Err(format!(
                        "Kerberos keytab file not found: {}",
                        keytab_path.display()
                    ));
                }
            }
            GssapiIdentityValidator::probe_library()?;
        }

        let info = ServerInfo {
            authenticator: builder
                .authenticator
                .unwrap_or_else(|| Arc::new(DefaultAuthenticator::new(config.user_tokens.clone()))),
            #[cfg(feature = "kerberos")]
            kerberos_validator: builder.kerberos_validator,
            role_resolver: Arc::new(RwLock::new(role_resolver)),
            namespace_defaults,
            application_uri,
            product_uri,
            application_name: LocalizedText {
                locale: UAString::null(),
                text: UAString::from(application_name),
            },
            start_time: Arc::new(opcua_types::DateTime::now()),
            servers,
            config: config.clone(),
            endpoint_certificates: RwLock::new(endpoint_certificates),
            server_pkey,
            certificate_store: certificate_store.clone(),
            security_checks: RwLock::new(crate::security_checks::SecurityCheckRegistry::new(
                config.security_check_max_entries,
            )),
            operational_limits: config.limits.operational.clone(),
            state: Arc::new(ServerState::Shutdown),
            send_buffer_size,
            receive_buffer_size,
            type_tree: type_tree.clone(),
            type_tree_snapshot: Arc::new(None),
            subscription_id_handle: AtomicHandle::new(1),
            monitored_item_id_handle: AtomicHandle::new(1),
            secure_channel_id_handle: Arc::new(AtomicHandle::new(1)),
            capabilities: ServerCapabilities::default(),
            service_level: service_level.clone(),
            port: AtomicU16::new(0),
            type_tree_getter: builder
                .type_tree_getter
                .unwrap_or_else(|| Arc::new(DefaultTypeTreeGetter)),
            type_loaders: RwLock::new(builder.type_loaders),
            #[cfg(feature = "lds")]
            registered_servers: RwLock::new(Default::default()),
            #[cfg(feature = "discovery-mdns")]
            mdns,
            #[cfg(all(feature = "lds", feature = "discovery-mdns"))]
            registered_mdns,
            #[cfg(feature = "diagnostics")]
            diagnostics: ServerDiagnostics::new(config.diagnostics),
            metrics: Arc::new(crate::metrics::ServerMetrics::new()),
            #[cfg(feature = "fota")]
            fota_cleanup: Default::default(),
            localized_text_variants: Default::default(),
            next_session_id: std::sync::atomic::AtomicU32::new(1),
            session_locale_ids: Default::default(),
            crypto_executor: Some(Arc::new(crate::crypto_executor::CryptoExecutor::new(2, 16))),
        };

        let info = Arc::new(info);
        let node_managers_ref = NodeManagersRef::new_empty();
        #[cfg(feature = "subscriptions")]
        let subscriptions = Arc::new(SubscriptionCache::new_with_node_managers(
            config.limits.subscriptions,
            node_managers_ref.clone(),
        ));
        let status_wrapper = Arc::new(ServerStatusWrapper::new(
            builder.build_info,
            #[cfg(feature = "subscriptions")]
            subscriptions.clone(),
        ));
        let session_notify = Arc::new(Notify::new());
        let session_manager = Arc::new(RwLock::new(SessionManager::new(
            info.clone(),
            session_notify.clone(),
        )));
        let context = ServerContext {
            node_managers: node_managers_ref.clone(),
            session_manager: session_manager.clone(),
            #[cfg(feature = "subscriptions")]
            subscriptions: subscriptions.clone(),
            info: info.clone(),
            authenticator: info.authenticator.clone(),
            type_tree: type_tree.clone(),
            type_tree_getter: info.type_tree_getter.clone(),
            status: status_wrapper.clone(),
        };

        let mut final_node_managers = Vec::new();
        for nm_builder in builder.node_managers {
            final_node_managers.push(nm_builder.build(context.clone()));
        }

        let node_managers = NodeManagers::new(final_node_managers);
        node_managers_ref.init_from_node_managers(node_managers.clone());

        #[cfg(all(
            feature = "generated-address-space",
            feature = "method-call",
            feature = "rbac"
        ))]
        if let Some(core_node_manager) =
            node_managers.get_of_type::<crate::node_manager::memory::CoreNodeManager>()
        {
            crate::rbac::role_management::register_role_management_methods(
                &core_node_manager,
                Arc::clone(&info.role_resolver),
                Arc::clone(core_node_manager.address_space()),
            );
        }

        let (reverse_connect_manager, reverse_connect_handle) =
            reverse_connect::ReverseConnectionManager::new(Duration::from_millis(
                config.reverse_connect_failure_delay_ms,
            ));

        let handle = ServerHandle::new(
            info.clone(),
            certificate_store.clone(),
            service_level,
            #[cfg(feature = "subscriptions")]
            subscriptions.clone(),
            node_managers.clone(),
            session_manager.clone(),
            type_tree.clone(),
            status_wrapper.clone(),
            builder.token.clone(),
            reverse_connect_handle,
        );
        Ok((
            Self {
                certificate_store,
                session_manager,
                connections: FuturesUnordered::new(),
                connection_map: HashMap::new(),
                #[cfg(feature = "subscriptions")]
                subscriptions,
                config,
                info,
                node_managers,
                token: builder.token,
                session_notify,
                status: status_wrapper.clone(),
                reverse_connect_manager,
            },
            handle,
        ))
    }

    /// Get a reference to the SubscriptionCache containing all subscriptions on the server.
    #[cfg(feature = "subscriptions")]
    pub fn subscriptions(&self) -> Arc<SubscriptionCache> {
        self.subscriptions.clone()
    }

    /// Returns a point-in-time copy of this server's metrics.
    pub fn metrics_snapshot(&self) -> ServerMetricsSnapshot {
        self.info.metrics.snapshot()
    }

    #[allow(clippy::await_holding_lock)]
    async fn initialize_node_managers(&self, context: &ServerContext) -> Result<(), String> {
        info!("Initializing node managers");
        {
            if self.node_managers.is_empty() {
                return Err("No node managers defined, server is invalid".to_string());
            }

            // Normally we would strongly attempt to avoid holding a lock over an await point,
            // but during initialization we essentially own the type tree, so this shouldn't deadlock
            // unless a manager for whatever reason attempts to lock the type tree again.
            let mut type_tree = trace_write_lock!(self.info.type_tree);

            for mgr in self.node_managers.iter() {
                mgr.init(&mut type_tree, context.clone()).await;
            }

            self.info.publish_type_tree_snapshot(&type_tree);
        }
        Ok(())
    }

    #[cfg(feature = "discovery-server-registration")]
    async fn run_discovery_server_registration(info: Arc<ServerInfo>) -> Never {
        let registered_server = info.registered_server();
        let Some(discovery_server_url) = info.config.discovery_server_url.as_ref() else {
            loop {
                futures::future::pending::<()>().await;
            }
        };
        crate::discovery::periodic_discovery_server_registration(
            discovery_server_url,
            registered_server,
            info.config.pki_dir.clone(),
            Duration::from_secs(5 * 60),
        )
        .await
    }

    fn server_context(&self) -> ServerContext {
        ServerContext {
            node_managers: self.node_managers.as_weak(),
            session_manager: self.session_manager.clone(),
            #[cfg(feature = "subscriptions")]
            subscriptions: self.subscriptions.clone(),
            info: self.info.clone(),
            authenticator: self.info.authenticator.clone(),
            type_tree: self.info.type_tree.clone(),
            type_tree_getter: self.info.type_tree_getter.clone(),
            status: self.status.clone(),
        }
    }

    async fn prepare_to_run(&self, context: &ServerContext) -> Result<(), String> {
        self.initialize_node_managers(context).await?;

        self.status.set_server_started();
        // SAFETY: Only called during server start-up before start_time is read
        unsafe {
            std::ptr::write(
                Arc::as_ptr(&self.info.start_time) as *mut DateTime,
                DateTime::now(),
            );
        }
        Ok(())
    }

    fn transport_config(&self) -> TransportConfig {
        TransportConfig {
            send_buffer_size: self.info.config.limits.send_buffer_size,
            max_message_size: self.info.config.limits.max_message_size,
            max_chunk_count: self.info.config.limits.max_chunk_count,
            receive_buffer_size: self.info.config.limits.receive_buffer_size,
            hello_timeout: Duration::from_secs(self.info.config.tcp_config.hello_timeout as u64),
            tcp_keepalive: self.info.config.tcp_config.tcp_keepalive,
        }
    }

    fn tcp_connection_deps(&self) -> TcpConnectionDeps {
        TcpConnectionDeps {
            max_connections: self.config.max_connections,
            max_connections_per_ip: self.config.max_connections_per_ip,
            transport_config: self.transport_config(),
            info: self.info.clone(),
            session_manager: self.session_manager.clone(),
            certificate_store: self.certificate_store.clone(),
            node_managers: self.node_managers.clone(),
            #[cfg(feature = "subscriptions")]
            subscriptions: self.subscriptions.clone(),
        }
    }

    async fn run_connection_loop<T: Send + 'static>(
        &mut self,
        #[cfg_attr(not(feature = "subscriptions"), allow(unused_variables))]
        context: &ServerContext,
        mut connection_source: ConnectionSource<T>,
        transport: AcceptedTransport,
    ) -> Result<(), String> {
        let mut connection_counter = 0;

        #[cfg(feature = "discovery-server-registration")]
        let discovery_fut = Self::run_discovery_server_registration(self.info.clone());

        #[cfg(not(feature = "discovery-server-registration"))]
        let discovery_fut = futures::future::pending();

        pin!(discovery_fut);

        #[cfg(feature = "discovery-mdns")]
        let mdns_fut = crate::discovery_mdns::run_mdns_discovery(self.info.clone());

        #[cfg(not(feature = "discovery-mdns"))]
        let mdns_fut = futures::future::pending();

        pin!(mdns_fut);

        #[cfg(feature = "subscriptions")]
        let subscription_fut =
            Self::run_subscription_ticks(self.config.subscription_poll_interval_ms, context);
        #[cfg(not(feature = "subscriptions"))]
        let subscription_fut = futures::future::pending();
        pin!(subscription_fut);

        let session_expiry_fut =
            Self::run_session_expiry(&self.session_manager, &self.session_notify);
        pin!(session_expiry_fut);

        loop {
            if connection_source.is_closed() && self.connections.is_empty() {
                break;
            }

            let conn_fut = if self.connections.is_empty() {
                if self.token.is_cancelled() {
                    break;
                }
                Either::Left(futures::future::pending::<Option<Result<u32, JoinError>>>())
            } else {
                Either::Right(self.connections.next())
            };
            let reverse_connect_fut = if self.connection_map.len() >= self.config.max_connections {
                Either::Left(futures::future::pending())
            } else {
                Either::Right(self.reverse_connect_manager.wait_for_connection())
            };

            tokio::select! {
                conn_res = conn_fut => {
                    match conn_res.unwrap() {
                        Ok(id) => {
                            info!("Connection {} terminated", id);
                            self.connection_map.remove(&id);
                            self.info.metrics.record_connection_closed();
                        },
                        Err(e) => error!("Connection panic! {e}")
                    }
                }
                _ = &mut subscription_fut => {}
                _ = &mut discovery_fut => {}
                _ = &mut mdns_fut => {}
                _ = &mut session_expiry_fut => {}
                rs = connection_source.next() => {
                    match rs {
                        Some(Ok((socket, addr, token))) => {
                            let deps = self.tcp_connection_deps();
                            let mut slots = ConnectionSlots {
                                connections: &mut self.connections,
                                connection_map: &mut self.connection_map,
                            };
                            let accepted = deps.accept(
                                &mut slots,
                                socket,
                                addr,
                                token,
                                connection_counter,
                                transport.clone(),
                            );
                            if accepted {
                                connection_counter += 1;
                            }
                        }
                        Some(Err(e)) => {
                            error!("Failed to accept client connection: {:?}", e);
                        }
                        None => {
                            info!("Stream handoff channel closed");
                            connection_source = ConnectionSource::Closed;
                        }
                    }
                }
                rev_connect = reverse_connect_fut => {
                    debug!("Attempting reverse connection to {:?}", rev_connect.target.address);
                    let conn = SessionStarter::new(
                        ReverseTcpConnector::new(
                            self.transport_config(),
                            self.info.decoding_options(),
                            rev_connect.target.address,
                            self.info.application_uri.to_string(),
                            rev_connect.target.endpoint_url,
                        ),
                        self.info.clone(),
                        self.session_manager.clone(),
                        self.certificate_store.clone(),
                        self.node_managers.clone(),
                        #[cfg(feature = "subscriptions")]
                        self.subscriptions.clone()
                    );

                    // We need to make sure that the reverse connect handle is passed
                    // to the connection task, so that we can signal the result of the connection attempt
                    // back to the reverse connect manager.
                    let (send, recv) = tokio::sync::mpsc::channel(5);
                    let rev_handle = rev_connect.handle;
                    self.info.metrics.record_connection_accepted();
                    let handle = tokio::spawn(async move {
                        let run = conn.run(recv, |status| {
                            rev_handle.set_result(status);
                        });
                        if let Err(payload) = std::panic::AssertUnwindSafe(run)
                            .catch_unwind()
                            .await
                        {
                            log_connection_panic(connection_counter, payload);
                        }
                        connection_counter
                    });
                    self.connections.push(handle);
                    self.connection_map.insert(connection_counter, ConnectionInfo {
                        command_send: send,
                        ip: rev_connect.target.address.ip(),
                    });
                    connection_counter += 1;
                }
                _ = self.token.cancelled() => {
                    for conn in self.connection_map.values() {
                        let _ = conn.command_send.send(ControllerCommand::Close).await;
                    }
                }
            }
        }

        Ok(())
    }

    /// Run the server using a given TCP listener.
    /// Note that the configured TCP endpoint is still used to create the endpoint
    /// descriptions, you must properly set `host` and `port` even when using this.
    ///
    /// This is useful for testing, as you can bind a `TcpListener` to port `0` auto-assign
    /// a port.
    pub async fn run_with(mut self, listener: TcpListener) -> Result<(), String> {
        let context = self.server_context();
        self.prepare_to_run(&context).await?;

        let addr = listener
            .local_addr()
            .map_err(|e| format!("Failed to bind socket: {e:?}"))?;
        info!("Now listening for connections on {addr}");

        self.info
            .port
            .store(addr.port(), std::sync::atomic::Ordering::Relaxed);

        self.log_endpoint_info();
        self.run_connection_loop(
            &context,
            ConnectionSource::<()>::Listener(listener),
            AcceptedTransport::Tcp,
        )
        .await
    }

    /// Run the server using a given TCP listener for `opc.wss` connections.
    ///
    /// The configured TCP endpoint is still used to create endpoint descriptions,
    /// but accepted sockets are upgraded with TLS and WebSocket framing before
    /// the normal OPC UA binary transport handshake.
    #[cfg(feature = "wss")]
    pub async fn run_with_wss(mut self, listener: TcpListener) -> Result<(), String> {
        let Some(tls_config) = self.config.wss_tls.as_ref().map(|config| config.0.clone()) else {
            return Err("Cannot run WSS listener without a WSS rustls ServerConfig".to_string());
        };

        let context = self.server_context();
        self.prepare_to_run(&context).await?;

        let addr = listener
            .local_addr()
            .map_err(|e| format!("Failed to bind socket: {e:?}"))?;
        info!("Now listening for WSS connections on {addr}");

        self.info
            .port
            .store(addr.port(), std::sync::atomic::Ordering::Relaxed);

        self.log_endpoint_info();
        self.run_connection_loop(
            &context,
            ConnectionSource::<()>::Listener(listener),
            AcceptedTransport::Wss(tls_config),
        )
        .await
    }

    /// Run the server using externally accepted TCP streams and caller-owned
    /// per-connection tokens.
    ///
    /// The configured TCP endpoint is still used to create endpoint descriptions,
    /// so callers must set `host` and `port` to match the listener that accepted
    /// the streams. The server exits after the stream channel closes and active
    /// connections finish.
    ///
    /// The token is never inspected by the server. It is moved into the spawned
    /// connection task and dropped when that task exits; if the stream is
    /// rejected by `max_connections`, the token is dropped with the stream.
    pub async fn run_with_streams<T: Send + 'static>(
        mut self,
        rx: mpsc::Receiver<(TcpStream, SocketAddr, T)>,
    ) -> Result<(), String> {
        let context = self.server_context();
        self.prepare_to_run(&context).await?;

        let port = self.config.tcp_config.port;
        self.info
            .port
            .store(port, std::sync::atomic::Ordering::Relaxed);
        info!(
            "Now accepting handed-off TCP connections for {}",
            self.info.base_endpoint()
        );

        self.log_endpoint_info();
        self.run_connection_loop(
            &context,
            ConnectionSource::Streams(rx),
            AcceptedTransport::Tcp,
        )
        .await
    }

    /// Run the server in thread-per-core (sharded) mode: one pinned
    /// `current_thread` runtime per core in `cores`, each binding its own
    /// `SO_REUSEPORT` listener on the configured address, so the kernel spreads
    /// incoming connections across cores and each connection is handled
    /// end-to-end on one core (per-shard I/O). The node managers and the shared
    /// session manager are the same instances the default path uses — only
    /// *where* connections run changes.
    ///
    /// The singleton background tasks (subscription cleanup, session expiry,
    /// discovery, mDNS) run once on the caller's runtime. Reverse connections
    /// are not driven in sharded mode.
    ///
    /// Opt-in; requires the `sharded` feature.
    #[cfg(feature = "sharded")]
    pub async fn run_sharded(self, cores: Vec<usize>) -> Result<(), String> {
        if cores.is_empty() {
            return Err("run_sharded requires at least one core".to_string());
        }
        let context = self.server_context();
        self.prepare_to_run(&context).await?;

        let Some(addr) = self.get_socket_address() else {
            error!("Cannot resolve server address, check server configuration");
            return Err("Cannot resolve server address, check server configuration".to_owned());
        };
        self.info
            .port
            .store(addr.port(), std::sync::atomic::Ordering::Relaxed);
        info!(
            "Now listening for connections on {addr} across {} sharded cores: {cores:?}",
            cores.len()
        );
        self.log_endpoint_info();

        let deps = self.tcp_connection_deps();
        let counter = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let mut handles = Vec::with_capacity(cores.len());
        for core in cores {
            let deps = deps.clone();
            let token = self.token.clone();
            let counter = counter.clone();
            let handle = std::thread::Builder::new()
                .name(format!("opcua-shard-{core}"))
                .spawn(move || {
                    if !core_affinity::set_for_current(core_affinity::CoreId { id: core }) {
                        warn!("Failed to pin shard thread to core {core}");
                    }
                    let rt = match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(rt) => rt,
                        Err(e) => {
                            error!("Failed to build runtime for shard core {core}: {e}");
                            return;
                        }
                    };
                    rt.block_on(async move {
                        let listener = match bind_reuse_port(addr) {
                            Ok(l) => l,
                            Err(e) => {
                                error!("Shard core {core} failed to bind {addr}: {e}");
                                return;
                            }
                        };
                        shard_accept_loop(deps, token, counter, listener).await;
                    });
                })
                .map_err(|e| format!("Failed to spawn shard thread for core {core}: {e}"))?;
            handles.push(handle);
        }

        // Singleton background tasks on the caller's runtime; returns on cancel.
        self.run_background_tasks(&context).await;

        // Cancellation propagates to shards via the shared token.
        for handle in handles {
            let _ = handle.join();
        }
        Ok(())
    }

    /// Drive the singleton background tasks (subscription cleanup, session
    /// expiry, discovery, mDNS) once, until the cancellation token fires. Used
    /// by [`run_sharded`](Self::run_sharded) so these are not replicated per
    /// shard (the subscription cleanup receiver is once-only).
    #[cfg(feature = "sharded")]
    async fn run_background_tasks(
        &self,
        #[cfg_attr(not(feature = "subscriptions"), allow(unused_variables))]
        context: &ServerContext,
    ) {
        #[cfg(feature = "discovery-server-registration")]
        let discovery_fut = Self::run_discovery_server_registration(self.info.clone());
        #[cfg(not(feature = "discovery-server-registration"))]
        let discovery_fut = futures::future::pending::<()>();
        pin!(discovery_fut);

        #[cfg(feature = "discovery-mdns")]
        let mdns_fut = crate::discovery_mdns::run_mdns_discovery(self.info.clone());
        #[cfg(not(feature = "discovery-mdns"))]
        let mdns_fut = futures::future::pending::<()>();
        pin!(mdns_fut);

        #[cfg(feature = "subscriptions")]
        let subscription_fut =
            Self::run_subscription_ticks(self.config.subscription_poll_interval_ms, context);
        #[cfg(not(feature = "subscriptions"))]
        let subscription_fut = futures::future::pending::<()>();
        pin!(subscription_fut);

        let session_expiry_fut =
            Self::run_session_expiry(&self.session_manager, &self.session_notify);
        pin!(session_expiry_fut);

        loop {
            tokio::select! {
                _ = &mut subscription_fut => {}
                _ = &mut discovery_fut => {}
                _ = &mut mdns_fut => {}
                _ = &mut session_expiry_fut => {}
                _ = self.token.cancelled() => break,
            }
        }
    }

    /// Run the server. The provided `token` can be used to stop the server gracefully.
    pub async fn run(self) -> Result<(), String> {
        let addr = self.get_socket_address();

        let Some(addr) = addr else {
            error!("Cannot resolve server address, check server configuration");
            return Err("Cannot resolve server address, check server configuration".to_owned());
        };

        info!("Try to bind address at {addr}");
        let listener = match TcpListener::bind(&addr).await {
            Ok(listener) => listener,
            Err(e) => {
                error!("Failed to bind socket: {:?}", e);
                return Err(format!("Failed to bind socket: {e:?}"));
            }
        };

        self.run_with(listener).await
    }

    #[cfg(feature = "subscriptions")]
    async fn run_subscription_ticks(_interval: u64, context: &ServerContext) -> Never {
        let context = context.clone();
        let cleanup_rx = context.subscriptions.take_cleanup_receiver();

        if let Some(rx) = cleanup_rx {
            let subscriptions = context.subscriptions.clone();
            subscriptions.run_cleanup(&context, rx).await;
        }

        futures::future::pending().await
    }

    async fn run_session_expiry(sessions: &RwLock<SessionManager>, notify: &Notify) -> Never {
        loop {
            let ((expiry, expired), notified) = {
                let session_lck = trace_read_lock!(sessions);
                // Make sure to create the notified future while we still hold the lock.
                (session_lck.check_session_expiry(), notify.notified())
            };
            if !expired.is_empty() {
                let mut session_lck = trace_write_lock!(sessions);
                for id in expired {
                    session_lck.expire_session(&id);
                }
            }
            tokio::select! {
                _ = tokio::time::sleep_until(expiry.into()) => {}
                _ = notified => {}
            }
        }
    }

    /// Log information about the endpoints on this server
    fn log_endpoint_info(&self) {
        info!("OPC UA Server: {}", self.info.application_name);
        info!("Base url: {}", self.info.base_endpoint());
        info!("Supported endpoints:");
        for (id, endpoint) in &self.config.endpoints {
            let users: Vec<String> = endpoint.user_token_ids.iter().cloned().collect();
            let users = users.join(", ");
            info!("Endpoint \"{}\": {}", id, endpoint.path);
            info!("  Security Mode:    {}", endpoint.security_mode);
            info!("  Security Policy:  {}", endpoint.security_policy);
            info!("  Supported user tokens - {}", users);
        }
    }

    /// Returns the server socket address.
    fn get_socket_address(&self) -> Option<SocketAddr> {
        // Resolve this host / port to an address (or not)
        let address = format!(
            "{}:{}",
            self.config.tcp_config.host, self.config.tcp_config.port
        );
        if let Ok(mut addrs_iter) = address.to_socket_addrs() {
            addrs_iter.next()
        } else {
            None
        }
    }
}

/// Bind a `SO_REUSEPORT` TCP listener so multiple shard runtimes can share the
/// same address and let the kernel distribute incoming connections across them.
#[cfg(feature = "sharded")]
fn bind_reuse_port(addr: SocketAddr) -> std::io::Result<TcpListener> {
    let socket = socket2::Socket::new(
        socket2::Domain::for_address(addr),
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )?;
    socket.set_reuse_address(true)?;
    socket.set_reuse_port(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&addr.into())?;
    socket.listen(1024)?;
    TcpListener::from_std(socket.into())
}

/// The accept + reap loop for one shard. Owns its own connection bookkeeping,
/// reuses the shared [`TcpConnectionDeps`] to spawn per-connection tasks on this
/// shard's runtime, and stops when the shared token is cancelled. This is the
/// accept subset of [`Server::run_connection_loop`], minus the singleton
/// background tasks and reverse-connect (which run once, elsewhere).
#[cfg(feature = "sharded")]
async fn shard_accept_loop(
    deps: TcpConnectionDeps,
    token: CancellationToken,
    counter: Arc<std::sync::atomic::AtomicU32>,
    listener: TcpListener,
) {
    let mut connections: FuturesUnordered<JoinHandle<u32>> = FuturesUnordered::new();
    let mut connection_map: HashMap<u32, ConnectionInfo> = HashMap::new();
    let mut source = ConnectionSource::<()>::Listener(listener);

    loop {
        if source.is_closed() && connections.is_empty() {
            break;
        }
        let conn_fut = if connections.is_empty() {
            if token.is_cancelled() {
                break;
            }
            Either::Left(futures::future::pending::<Option<Result<u32, JoinError>>>())
        } else {
            Either::Right(connections.next())
        };

        tokio::select! {
            conn_res = conn_fut => {
                match conn_res.unwrap() {
                    Ok(id) => {
                        info!("Connection {id} terminated");
                        connection_map.remove(&id);
                        deps.info.metrics.record_connection_closed();
                    }
                    Err(e) => error!("Connection panic! {e}"),
                }
            }
            rs = source.next() => {
                match rs {
                    Some(Ok((socket, addr, tok))) => {
                        let id = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let mut slots = ConnectionSlots {
                            connections: &mut connections,
                            connection_map: &mut connection_map,
                        };
                        deps.accept(&mut slots, socket, addr, tok, id, AcceptedTransport::Tcp);
                    }
                    Some(Err(e)) => error!("Failed to accept client connection: {e:?}"),
                    None => source = ConnectionSource::Closed,
                }
            }
            _ = token.cancelled() => {
                for conn in connection_map.values() {
                    let _ = conn.command_send.send(ControllerCommand::Close).await;
                }
            }
        }
    }
}
