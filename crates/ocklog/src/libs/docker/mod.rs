pub mod client;
pub mod containers;
pub mod events;
pub mod logging;

pub use client::DockerTransport;
pub use containers::{ContainerEngine, ContainerSummary};
pub use events::EventSubscription;
pub use logging::{DockerLogStream, LogFetchParams};

pub struct DockerEngine {
    transport: DockerTransport,
}

impl DockerEngine {
    pub fn new(transport: DockerTransport) -> Self {
        Self { transport }
    }

    pub fn default_local() -> Self {
        Self::new(DockerTransport::default_socket())
    }

    pub fn is_available(&self) -> bool {
        self.transport.is_available()
    }

    pub fn containers(&self) -> ContainerEngine {
        ContainerEngine::new(self.transport.clone())
    }

    pub async fn subscribe_events(&self) -> eyre::Result<EventSubscription> {
        EventSubscription::connect(&self.transport).await
    }

    pub async fn stream_logs(&self, params: LogFetchParams<'_>) -> eyre::Result<DockerLogStream> {
        DockerLogStream::open(&self.transport, params).await
    }
}
