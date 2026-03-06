use bollard::container::{
    Config, CreateContainerOptions, StartContainerOptions, StopContainerOptions,
};
use bollard::Docker;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Mutex};
use tokio::time::sleep;
use tracing::{info, warn};

#[derive(Debug, Clone, PartialEq)]
pub enum ContainerState {
    None,
    Starting,
    Ready,
    Idle,
    Stopped,
}

struct ManagedContainer {
    state: ContainerState,
    container_id: Option<String>,
    ready_tx: Option<watch::Sender<bool>>,
    ready_rx: watch::Receiver<bool>,
}

pub struct ContainerManager {
    docker: Docker,
    containers: Arc<DashMap<String, Arc<Mutex<ManagedContainer>>>>,
}

impl ContainerManager {
    pub fn new() -> Result<Self, bollard::errors::Error> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(ContainerManager {
            docker,
            containers: Arc::new(DashMap::new()),
        })
    }

    pub async fn ensure_ready(
        &self,
        service_name: &str,
        image: &str,
        idle_timeout: Duration,
    ) -> Result<(), ContainerError> {
        let entry = self
            .containers
            .entry(service_name.to_string())
            .or_insert_with(|| {
                let (tx, rx) = watch::channel(false);
                Arc::new(Mutex::new(ManagedContainer {
                    state: ContainerState::None,
                    container_id: None,
                    ready_tx: Some(tx),
                    ready_rx: rx,
                }))
            })
            .clone();

        let mut container = entry.lock().await;

        match container.state {
            ContainerState::Ready | ContainerState::Idle => {
                container.state = ContainerState::Ready;
                return Ok(());
            }
            ContainerState::Starting => {
                // Wait for ready signal
                let mut rx = container.ready_rx.clone();
                drop(container);
                rx.changed()
                    .await
                    .map_err(|e| ContainerError::Start(e.to_string()))?;
                return Ok(());
            }
            ContainerState::None | ContainerState::Stopped => {
                container.state = ContainerState::Starting;
            }
        }

        // Start the container
        let container_name = format!("proxium-{}", service_name.replace('.', "-"));

        let config = Config {
            image: Some(image.to_string()),
            hostname: Some(service_name.to_string()),
            ..Default::default()
        };

        let id = match self
            .docker
            .create_container(
                Some(CreateContainerOptions {
                    name: &container_name,
                    platform: None,
                }),
                config,
            )
            .await
        {
            Ok(resp) => resp.id,
            Err(bollard::errors::Error::DockerResponseServerError { status_code: 409, .. }) => {
                // Container already exists, start it
                container_name.clone()
            }
            Err(e) => return Err(ContainerError::Start(e.to_string())),
        };

        self.docker
            .start_container(&id, None::<StartContainerOptions<String>>)
            .await
            .or_else(|e| {
                // Already running is fine
                if e.to_string().contains("already started") {
                    Ok(())
                } else {
                    Err(ContainerError::Start(e.to_string()))
                }
            })?;

        container.container_id = Some(id.clone());
        container.state = ContainerState::Ready;
        if let Some(tx) = container.ready_tx.take() {
            let _ = tx.send(true);
        }

        info!(service = service_name, container_id = %id, "container started");

        // Spawn idle timer
        let docker = self.docker.clone();
        let containers = self.containers.clone();
        let svc = service_name.to_string();
        tokio::spawn(async move {
            idle_watcher(docker, containers, svc, id, idle_timeout).await;
        });

        Ok(())
    }
}

async fn idle_watcher(
    docker: Docker,
    containers: Arc<DashMap<String, Arc<Mutex<ManagedContainer>>>>,
    service_name: String,
    container_id: String,
    timeout: Duration,
) {
    sleep(timeout).await;

    if let Some(entry) = containers.get(&service_name) {
        let mut container = entry.lock().await;
        if container.state == ContainerState::Idle || container.state == ContainerState::Ready {
            info!(service = %service_name, "idle timeout, stopping container");
            if let Err(e) = docker
                .stop_container(
                    &container_id,
                    Some(StopContainerOptions { t: 10 }),
                )
                .await
            {
                warn!(service = %service_name, error = %e, "failed to stop container");
            }
            container.state = ContainerState::Stopped;
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ContainerError {
    #[error("failed to start container: {0}")]
    Start(String),
}
