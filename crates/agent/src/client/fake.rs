//! Fake — test double for Docker operations.
//!
//! Provides a deterministic [`FakeDocker`] that implements [`DockerOps`]
//! using in-memory state. Useful for unit-testing domain modules and
//! integration tests without a running Docker daemon.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::client::docker::DockerOps;
use crate::docker::client::{DockerError, SwarmInspectResult};
use crate::docker::inventory::ContainerInfo;
use crate::docker::stream::{LogStream, LogLine, LogStreamRequest, LogLevel};
use crate::filter::engine::FilterEngine;

// ── In-memory state ─────────────────────────────────────────────

/// A canned container for the fake store.
#[derive(Clone, Debug)]
pub struct FakeContainer {
    pub info: ContainerInfo,
    pub logs: Vec<FakeLogLine>,
    pub running: bool,
    pub paused: bool,
}

/// A canned log line for the fake store.
#[derive(Clone, Debug)]
pub struct FakeLogLine {
    pub timestamp: i64,
    pub level: LogLevel,
    pub content: Vec<u8>,
}

/// Mutable inner state protected by a mutex.
#[derive(Default)]
struct Inner {
    containers: HashMap<String, FakeContainer>,
    images: Vec<bollard::models::ImageSummary>,
    volumes: Vec<bollard::models::Volume>,
    networks: Vec<bollard::models::Network>,
    nodes: Vec<bollard::models::Node>,
    services: Vec<bollard::models::Service>,
    tasks: Vec<bollard::models::Task>,
    secrets: Vec<bollard::models::Secret>,
    configs: Vec<bollard::models::Config>,
    swarm: Option<bollard::models::Swarm>,
}

/// A fake Docker client for deterministic testing.
///
/// All methods operate on in-memory state. The builder methods allow
/// pre-populating containers, images, etc. before running test code.
pub struct FakeDocker {
    inner: Mutex<Inner>,
}

impl FakeDocker {
    /// Create an empty fake Docker client.
    pub fn new() -> Self {
        Self { inner: Mutex::new(Inner::default()) }
    }

    /// Seed a container into the fake store.
    pub async fn add_container(&self, container: FakeContainer) {
        let mut state = self.inner.lock().await;
        state.containers.insert(container.info.id.clone(), container);
    }

    /// Seed an image.
    pub async fn add_image(&self, image: bollard::models::ImageSummary) {
        self.inner.lock().await.images.push(image);
    }

    /// Seed a volume.
    pub async fn add_volume(&self, volume: bollard::models::Volume) {
        self.inner.lock().await.volumes.push(volume);
    }

    /// Seed a network.
    pub async fn add_network(&self, network: bollard::models::Network) {
        self.inner.lock().await.networks.push(network);
    }

    /// Seed a node.
    pub async fn add_node(&self, node: bollard::models::Node) {
        self.inner.lock().await.nodes.push(node);
    }

    /// Seed a service.
    pub async fn add_service(&self, service: bollard::models::Service) {
        self.inner.lock().await.services.push(service);
    }

    /// Seed a task.
    pub async fn add_task(&self, task: bollard::models::Task) {
        self.inner.lock().await.tasks.push(task);
    }

    /// Seed a swarm (makes this node a manager).
    pub async fn set_swarm(&self, swarm: bollard::models::Swarm) {
        self.inner.lock().await.swarm = Some(swarm);
    }
}

impl Default for FakeDocker {
    fn default() -> Self {
        Self::new()
    }
}

// ── DockerOps implementation ────────────────────────────────────

impl DockerOps for FakeDocker {
    // ── Container queries ───────────────────────────────────────

    fn list_containers(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<ContainerInfo>, DockerError>> + Send + '_>> {
        Box::pin(async {
            let state = self.inner.lock().await;
            Ok(state.containers.values().map(|c| c.info.clone()).collect())
        })
    }

    fn inspect_container<'a>(
        &'a self,
        id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ContainerInfo, DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let state = self.inner.lock().await;
            state.containers.get(id)
                .map(|c| c.info.clone())
                .ok_or_else(|| DockerError::ContainerNotFound(id.to_string()))
        })
    }

    fn inspect_container_raw<'a>(
        &'a self,
        id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::ContainerInspectResponse, DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let state = self.inner.lock().await;
            if state.containers.contains_key(id) {
                Ok(bollard::models::ContainerInspectResponse {
                    id: Some(id.to_string()),
                    ..Default::default()
                })
            } else {
                Err(DockerError::ContainerNotFound(id.to_string()))
            }
        })
    }

    // ── Logs ────────────────────────────────────────────────────

    fn stream_logs(
        &self,
        request: LogStreamRequest,
        filter: Option<Arc<FilterEngine>>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<LogStream, DockerError>> + Send + '_>> {
        Box::pin(async move {
            let state = self.inner.lock().await;
            let container = state.containers.get(&request.container_id)
                .ok_or_else(|| DockerError::ContainerNotFound(request.container_id.clone()))?;

            let lines: Vec<Result<LogLine, DockerError>> = container.logs.iter().map(|l| {
                Ok(LogLine {
                    timestamp: l.timestamp,
                    stream_type: l.level,
                    content: bytes::Bytes::from(l.content.clone()),
                })
            }).collect();

            let stream = tokio_stream::iter(lines);
            Ok(LogStream::new(request.container_id.clone(), stream, filter))
        })
    }

    // ── Stats ───────────────────────────────────────────────────

    fn stats<'a>(
        &'a self,
        container_id: &'a str,
        _stream: bool,
    ) -> Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        Pin<Box<dyn tokio_stream::Stream<Item = Result<bollard::models::ContainerStatsResponse, bollard::errors::Error>> + Send>>,
                        DockerError,
                    >,
                > + Send + 'a,
        >,
    > {
        Box::pin(async move {
            let state = self.inner.lock().await;
            if !state.containers.contains_key(container_id) {
                return Err(DockerError::ContainerNotFound(container_id.to_string()));
            }
            let empty: Vec<Result<bollard::models::ContainerStatsResponse, bollard::errors::Error>> = vec![
                Ok(bollard::models::ContainerStatsResponse::default()),
            ];
            Ok(Box::pin(tokio_stream::iter(empty)) as Pin<Box<dyn tokio_stream::Stream<Item = _> + Send>>)
        })
    }

    // ── Container lifecycle ─────────────────────────────────────

    fn start_container<'a>(
        &'a self,
        container_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let mut state = self.inner.lock().await;
            match state.containers.get_mut(container_id) {
                Some(c) => { c.running = true; c.info.state = "running".into(); Ok(()) }
                None => Err(DockerError::ContainerNotFound(container_id.to_string())),
            }
        })
    }

    fn stop_container<'a>(
        &'a self,
        container_id: &'a str,
        _timeout_secs: Option<u32>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let mut state = self.inner.lock().await;
            match state.containers.get_mut(container_id) {
                Some(c) => { c.running = false; c.info.state = "exited".into(); Ok(()) }
                None => Err(DockerError::ContainerNotFound(container_id.to_string())),
            }
        })
    }

    fn restart_container<'a>(
        &'a self,
        container_id: &'a str,
        _timeout_secs: Option<u32>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let mut state = self.inner.lock().await;
            match state.containers.get_mut(container_id) {
                Some(c) => { c.running = true; c.info.state = "running".into(); Ok(()) }
                None => Err(DockerError::ContainerNotFound(container_id.to_string())),
            }
        })
    }

    fn pause_container<'a>(
        &'a self,
        container_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let mut state = self.inner.lock().await;
            match state.containers.get_mut(container_id) {
                Some(c) => { c.paused = true; c.info.state = "paused".into(); Ok(()) }
                None => Err(DockerError::ContainerNotFound(container_id.to_string())),
            }
        })
    }

    fn unpause_container<'a>(
        &'a self,
        container_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let mut state = self.inner.lock().await;
            match state.containers.get_mut(container_id) {
                Some(c) => { c.paused = false; c.info.state = "running".into(); Ok(()) }
                None => Err(DockerError::ContainerNotFound(container_id.to_string())),
            }
        })
    }

    fn remove_container<'a>(
        &'a self,
        container_id: &'a str,
        _force: bool,
        _remove_volumes: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let mut state = self.inner.lock().await;
            state.containers.remove(container_id)
                .map(|_| ())
                .ok_or_else(|| DockerError::ContainerNotFound(container_id.to_string()))
        })
    }

    // ── Images ──────────────────────────────────────────────────

    fn list_images(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::ImageSummary>, DockerError>> + Send + '_>> {
        Box::pin(async { Ok(self.inner.lock().await.images.clone()) })
    }

    fn inspect_image<'a>(
        &'a self,
        _image_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::ImageInspect, DockerError>> + Send + 'a>> {
        Box::pin(async { Ok(bollard::models::ImageInspect::default()) })
    }

    fn pull_image<'a>(
        &'a self,
        _image: &'a str,
        _tag: &'a str,
        _registry_auth: Option<&'a str>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn remove_image<'a>(
        &'a self,
        _image_id: &'a str,
        _force: bool,
        _no_prune: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    // ── Volumes ─────────────────────────────────────────────────

    fn list_volumes(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::VolumeListResponse, DockerError>> + Send + '_>> {
        Box::pin(async {
            let state = self.inner.lock().await;
            Ok(bollard::models::VolumeListResponse {
                volumes: Some(state.volumes.clone()),
                warnings: None,
            })
        })
    }

    fn inspect_volume<'a>(
        &'a self,
        name: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::Volume, DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let state = self.inner.lock().await;
            state.volumes.iter().find(|v| v.name == name).cloned()
                .ok_or_else(|| DockerError::ContainerNotFound(format!("Volume not found: {}", name)))
        })
    }

    fn create_volume<'a>(
        &'a self,
        name: &'a str,
        driver: Option<&'a str>,
        labels: HashMap<String, String>,
        _driver_opts: HashMap<String, String>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::Volume, DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let vol = bollard::models::Volume {
                name: name.to_string(),
                driver: driver.unwrap_or("local").to_string(),
                labels: labels,
                ..Default::default()
            };
            self.inner.lock().await.volumes.push(vol.clone());
            Ok(vol)
        })
    }

    fn remove_volume<'a>(
        &'a self,
        name: &'a str,
        _force: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let mut state = self.inner.lock().await;
            state.volumes.retain(|v| v.name != name);
            Ok(())
        })
    }

    // ── Networks ────────────────────────────────────────────────

    fn list_networks(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Network>, DockerError>> + Send + '_>> {
        Box::pin(async { Ok(self.inner.lock().await.networks.clone()) })
    }

    fn inspect_network<'a>(
        &'a self,
        _network_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::NetworkInspect, DockerError>> + Send + 'a>> {
        Box::pin(async { Ok(Default::default()) })
    }

    fn create_network<'a>(
        &'a self,
        name: &'a str,
        _driver: Option<&'a str>,
        _labels: HashMap<String, String>,
        _internal: bool,
        _attachable: bool,
        _enable_ipv6: bool,
        _options: HashMap<String, String>,
        _ipam: Option<bollard::models::Ipam>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::NetworkCreateResponse, DockerError>> + Send + 'a>> {
        Box::pin(async move {
            Ok(bollard::models::NetworkCreateResponse {
                id: format!("fake-net-{}", name),
                warning: String::new(),
            })
        })
    }

    fn remove_network<'a>(
        &'a self,
        network_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let mut state = self.inner.lock().await;
            state.networks.retain(|n| n.id.as_deref() != Some(network_id));
            Ok(())
        })
    }

    fn network_connect<'a>(
        &'a self,
        _network_id: &'a str,
        _container_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn network_disconnect<'a>(
        &'a self,
        _network_id: &'a str,
        _container_id: &'a str,
        _force: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    // ── System ──────────────────────────────────────────────────

    fn system_info(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::SystemInfo, DockerError>> + Send + '_>> {
        Box::pin(async { Ok(Default::default()) })
    }

    // ── Swarm ───────────────────────────────────────────────────

    fn swarm_inspect(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<SwarmInspectResult, DockerError>> + Send + '_>> {
        Box::pin(async {
            let state = self.inner.lock().await;
            match &state.swarm {
                Some(s) => Ok(SwarmInspectResult::Manager(s.clone())),
                None => Ok(SwarmInspectResult::NotInSwarm),
            }
        })
    }

    fn list_nodes(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Node>, DockerError>> + Send + '_>> {
        Box::pin(async { Ok(self.inner.lock().await.nodes.clone()) })
    }

    fn list_services(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Service>, DockerError>> + Send + '_>> {
        Box::pin(async { Ok(self.inner.lock().await.services.clone()) })
    }

    fn inspect_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::Service, DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let state = self.inner.lock().await;
            state.services.iter().find(|s| s.id.as_deref() == Some(service_id)).cloned()
                .ok_or_else(|| DockerError::BollardError(
                    bollard::errors::Error::DockerResponseServerError {
                        status_code: 404,
                        message: format!("service {} not found", service_id),
                    },
                ))
        })
    }

    fn list_tasks(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Task>, DockerError>> + Send + '_>> {
        Box::pin(async { Ok(self.inner.lock().await.tasks.clone()) })
    }

    fn list_secrets(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Secret>, DockerError>> + Send + '_>> {
        Box::pin(async { Ok(self.inner.lock().await.secrets.clone()) })
    }

    fn list_configs(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Config>, DockerError>> + Send + '_>> {
        Box::pin(async { Ok(self.inner.lock().await.configs.clone()) })
    }

    fn inspect_node<'a>(
        &'a self,
        node_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Option<bollard::models::Node>, DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let state = self.inner.lock().await;
            Ok(state.nodes.iter().find(|n| n.id.as_deref() == Some(node_id)).cloned())
        })
    }

    fn update_node<'a>(
        &'a self,
        _node_id: &'a str,
        _spec: bollard::models::NodeSpec,
        _version: i64,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    // ── Service CRUD ────────────────────────────────────────────

    fn stream_service_logs<'a>(
        &'a self,
        _service_id: &'a str,
        _follow: bool,
        _tail: Option<String>,
        _since: i32,
        _until: i32,
        _timestamps: bool,
    ) -> Pin<Box<dyn tokio_stream::Stream<Item = Result<bollard::container::LogOutput, bollard::errors::Error>> + Send + 'a>> {
        let empty: Vec<Result<bollard::container::LogOutput, bollard::errors::Error>> = vec![];
        Box::pin(tokio_stream::iter(empty))
    }

    fn create_service<'a>(
        &'a self,
        spec: bollard::models::ServiceSpec,
        _registry_auth: Option<&'a str>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let id = format!("fake-svc-{}", spec.name.as_deref().unwrap_or("unnamed"));
            let svc = bollard::models::Service {
                id: Some(id.clone()),
                spec: Some(spec),
                ..Default::default()
            };
            self.inner.lock().await.services.push(svc);
            Ok(id)
        })
    }

    fn delete_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let mut state = self.inner.lock().await;
            state.services.retain(|s| s.id.as_deref() != Some(service_id));
            Ok(())
        })
    }

    fn update_service<'a>(
        &'a self,
        _service_id: &'a str,
        _spec: bollard::models::ServiceSpec,
        _version: u64,
        _force: bool,
        _registry_auth: Option<&'a str>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn rollback_service<'a>(
        &'a self,
        _service_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    // ── Secret & Config CRUD ────────────────────────────────────

    fn create_secret<'a>(
        &'a self,
        name: &'a str,
        _data: &'a [u8],
        _labels: HashMap<String, String>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let id = format!("fake-secret-{}", name);
            Ok(id)
        })
    }

    fn delete_secret<'a>(
        &'a self,
        secret_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let mut state = self.inner.lock().await;
            state.secrets.retain(|s| s.id.as_deref() != Some(secret_id));
            Ok(())
        })
    }

    fn create_config<'a>(
        &'a self,
        name: &'a str,
        _data: &'a [u8],
        _labels: HashMap<String, String>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let id = format!("fake-config-{}", name);
            Ok(id)
        })
    }

    fn delete_config<'a>(
        &'a self,
        config_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let mut state = self.inner.lock().await;
            state.configs.retain(|c| c.id.as_deref() != Some(config_id));
            Ok(())
        })
    }

    // ── Swarm Init / Join / Leave ───────────────────────────────

    fn swarm_init<'a>(
        &'a self,
        _listen_addr: &'a str,
        _advertise_addr: &'a str,
        _force_new_cluster: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + 'a>> {
        Box::pin(async {
            let swarm = bollard::models::Swarm {
                id: Some("fake-swarm-id".into()),
                ..Default::default()
            };
            self.inner.lock().await.swarm = Some(swarm);
            Ok("fake-node-id".to_string())
        })
    }

    fn swarm_join<'a>(
        &'a self,
        _remote_addrs: Vec<String>,
        _join_token: &'a str,
        _listen_addr: &'a str,
        _advertise_addr: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn swarm_leave(
        &self,
        _force: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + '_>> {
        Box::pin(async {
            self.inner.lock().await.swarm = None;
            Ok(())
        })
    }

    fn remove_node<'a>(
        &'a self,
        node_id: &'a str,
        _force: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let mut state = self.inner.lock().await;
            state.nodes.retain(|n| n.id.as_deref() != Some(node_id));
            Ok(())
        })
    }

    // ── Events ──────────────────────────────────────────────────

    fn stream_events(
        &self,
        _type_filters: Vec<String>,
        _since: Option<i64>,
        _until: Option<i64>,
    ) -> Pin<Box<dyn tokio_stream::Stream<Item = Result<bollard::models::EventMessage, DockerError>> + Send + '_>> {
        let empty: Vec<Result<bollard::models::EventMessage, DockerError>> = vec![];
        Box::pin(tokio_stream::iter(empty))
    }

    // ── Exec / Shell ────────────────────────────────────────────

    fn create_exec<'a>(
        &'a self,
        _container_id: &'a str,
        _cmd: Vec<String>,
        _tty: bool,
        _working_dir: Option<String>,
        _env: Vec<String>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + 'a>> {
        Box::pin(async { Ok("fake-exec-id".to_string()) })
    }

    fn start_exec<'a>(
        &'a self,
        _exec_id: &'a str,
        _tty: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::exec::StartExecResults, DockerError>> + Send + 'a>> {
        Box::pin(async {
            Err(DockerError::StreamClosed)
        })
    }

    fn resize_exec<'a>(
        &'a self,
        _exec_id: &'a str,
        _height: u16,
        _width: u16,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn inspect_exec<'a>(
        &'a self,
        _exec_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::ExecInspectResponse, DockerError>> + Send + 'a>> {
        Box::pin(async { Ok(Default::default()) })
    }

    // ── Task ────────────────────────────────────────────────────

    fn inspect_task<'a>(
        &'a self,
        task_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Option<bollard::models::Task>, DockerError>> + Send + 'a>> {
        Box::pin(async move {
            let state = self.inner.lock().await;
            Ok(state.tasks.iter().find(|t| t.id.as_deref() == Some(task_id)).cloned())
        })
    }

    fn stream_task_logs<'a>(
        &'a self,
        _task_id: &'a str,
        _follow: bool,
        _tail: Option<String>,
        _since: i32,
        _until: i32,
        _timestamps: bool,
    ) -> Pin<Box<dyn tokio_stream::Stream<Item = Result<bollard::container::LogOutput, bollard::errors::Error>> + Send + 'a>> {
        let empty: Vec<Result<bollard::container::LogOutput, bollard::errors::Error>> = vec![];
        Box::pin(tokio_stream::iter(empty))
    }

    // ── Swarm update / unlock ───────────────────────────────────

    fn swarm_update(
        &self,
        _spec: bollard::models::SwarmSpec,
        _version: i64,
        _rotate_worker_token: bool,
        _rotate_manager_token: bool,
        _rotate_manager_unlock_key: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }

    fn swarm_unlock_key(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + '_>> {
        Box::pin(async { Ok("fake-unlock-key".to_string()) })
    }

    fn swarm_unlock<'a>(
        &'a self,
        _unlock_key: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }
}

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::engine::FilterMode;

    fn make_container(id: &str, name: &str, state: &str) -> FakeContainer {
        FakeContainer {
            info: ContainerInfo {
                id: id.to_string(),
                name: name.to_string(),
                image: "nginx:latest".to_string(),
                state: state.to_string(),
                status: format!("Up 2 hours"),
                log_driver: Some("json-file".to_string()),
                labels: HashMap::new(),
                created_at: 1700000000,
                ports: vec![],
                state_info: None,
            },
            logs: vec![
                FakeLogLine { timestamp: 1, level: LogLevel::Stdout, content: b"hello world".to_vec() },
                FakeLogLine { timestamp: 2, level: LogLevel::Stderr, content: b"ERROR: something broke".to_vec() },
            ],
            running: state == "running",
            paused: state == "paused",
        }
    }

    #[tokio::test]
    async fn test_list_containers() {
        let fake = FakeDocker::new();
        fake.add_container(make_container("abc123", "web", "running")).await;
        fake.add_container(make_container("def456", "db", "exited")).await;

        let containers = fake.list_containers().await.unwrap();
        assert_eq!(containers.len(), 2);
    }

    #[tokio::test]
    async fn test_inspect_container_found() {
        let fake = FakeDocker::new();
        fake.add_container(make_container("abc123", "web", "running")).await;

        let info = fake.inspect_container("abc123").await.unwrap();
        assert_eq!(info.name, "web");
    }

    #[tokio::test]
    async fn test_inspect_container_not_found() {
        let fake = FakeDocker::new();
        let result = fake.inspect_container("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_container_lifecycle() {
        let fake = FakeDocker::new();
        fake.add_container(make_container("abc123", "web", "running")).await;

        // Stop
        fake.stop_container("abc123", None).await.unwrap();
        let info = fake.inspect_container("abc123").await.unwrap();
        assert_eq!(info.state, "exited");

        // Start
        fake.start_container("abc123").await.unwrap();
        let info = fake.inspect_container("abc123").await.unwrap();
        assert_eq!(info.state, "running");

        // Pause
        fake.pause_container("abc123").await.unwrap();
        let info = fake.inspect_container("abc123").await.unwrap();
        assert_eq!(info.state, "paused");

        // Unpause
        fake.unpause_container("abc123").await.unwrap();
        let info = fake.inspect_container("abc123").await.unwrap();
        assert_eq!(info.state, "running");

        // Remove
        fake.remove_container("abc123", false, false).await.unwrap();
        assert!(fake.inspect_container("abc123").await.is_err());
    }

    #[tokio::test]
    async fn test_log_stream() {
        use tokio_stream::StreamExt;

        let fake = FakeDocker::new();
        fake.add_container(make_container("abc123", "web", "running")).await;

        let request = LogStreamRequest {
            container_id: "abc123".to_string(),
            since: None,
            until: None,
            follow: false,
            filter_pattern: None,
            filter_mode: FilterMode::Include,
            tail_lines: None,
        };

        let mut stream = fake.stream_logs(request, None).await.unwrap();
        let mut count = 0;
        while let Some(Ok(_)) = stream.inner_stream.next().await {
            count += 1;
        }
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_swarm_not_in_swarm() {
        let fake = FakeDocker::new();
        match fake.swarm_inspect().await.unwrap() {
            SwarmInspectResult::NotInSwarm => {}
            _ => panic!("Expected NotInSwarm"),
        }
    }

    #[tokio::test]
    async fn test_swarm_init_and_leave() {
        let fake = FakeDocker::new();

        // Init
        let node_id = fake.swarm_init("0.0.0.0:2377", "", false).await.unwrap();
        assert!(!node_id.is_empty());

        // Should be manager now
        match fake.swarm_inspect().await.unwrap() {
            SwarmInspectResult::Manager(_) => {}
            _ => panic!("Expected Manager"),
        }

        // Leave
        fake.swarm_leave(false).await.unwrap();
        match fake.swarm_inspect().await.unwrap() {
            SwarmInspectResult::NotInSwarm => {}
            _ => panic!("Expected NotInSwarm after leave"),
        }
    }

    #[tokio::test]
    async fn test_service_crud() {
        let fake = FakeDocker::new();

        let spec = bollard::models::ServiceSpec {
            name: Some("my-svc".into()),
            ..Default::default()
        };
        let id = fake.create_service(spec, None).await.unwrap();
        assert!(id.contains("my-svc"));

        let services = fake.list_services().await.unwrap();
        assert_eq!(services.len(), 1);

        fake.delete_service(&id).await.unwrap();
        let services = fake.list_services().await.unwrap();
        assert_eq!(services.len(), 0);
    }
}
