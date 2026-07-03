use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use tracing::Instrument;

use opcua_core::sync::RwLock;
use opcua_crypto::CertificateStore;
use opcua_types::StatusCode;

use crate::{
    info::ServerInfo,
    node_manager::NodeManagers,
    session::{
        controller::SessionController, controller_command::ControllerCommand,
        manager::SessionManager,
    },
    transport::{tcp::ConnectionTransport, Connector},
};

#[cfg(feature = "subscriptions")]
use crate::SubscriptionCache;

pub(crate) struct SessionStarter<T> {
    connector: T,
    info: Arc<ServerInfo>,
    session_manager: Arc<RwLock<SessionManager>>,
    certificate_store: Arc<RwLock<CertificateStore>>,
    node_managers: NodeManagers,
    #[cfg(feature = "subscriptions")]
    subscriptions: Arc<SubscriptionCache>,
}

impl<T> SessionStarter<T>
where
    T: Connector,
    T::Transport: ConnectionTransport,
{
    pub(crate) fn new(
        connector: T,
        info: Arc<ServerInfo>,
        session_manager: Arc<RwLock<SessionManager>>,
        certificate_store: Arc<RwLock<CertificateStore>>,
        node_managers: NodeManagers,
        #[cfg(feature = "subscriptions")] subscriptions: Arc<SubscriptionCache>,
    ) -> Self {
        Self {
            connector,
            info,
            session_manager,
            certificate_store,
            node_managers,
            #[cfg(feature = "subscriptions")]
            subscriptions,
        }
    }

    pub(crate) async fn run(
        self,
        mut command: tokio::sync::mpsc::Receiver<ControllerCommand>,
        on_connect: impl FnOnce(StatusCode) + Send,
    ) {
        let token = CancellationToken::new();
        let span = tracing::info_span!("Establish TCP channel");
        let fut = self
            .connector
            .connect(self.info.clone(), token.clone())
            .instrument(span.clone());
        tokio::pin!(fut);
        let transport = tokio::select! {
            cmd = command.recv() => {
                match cmd {
                    Some(ControllerCommand::Close) | None => {
                        token.cancel();
                        let _ = fut.await;
                        return;
                    }
                }
            }
            r = &mut fut => {
                match r {
                    Ok(t) => t,
                    Err(e) => {
                        on_connect(e);
                        span.in_scope(|| {
                            tracing::error!("Connection failed while waiting for channel to be established: {e}");
                        });
                        return;
                    }
                }
            }
        };

        let controller = SessionController::new(
            transport,
            self.session_manager,
            self.certificate_store,
            self.info,
            self.node_managers,
            #[cfg(feature = "subscriptions")]
            self.subscriptions,
        );
        controller.run(command).await
    }
}
