//! Docker trait — abstract interface for all Docker operations.
//!
//! Every domain module accesses Docker through this trait.
//! `live.rs` provides the real Bollard-backed implementation.
//! `fake.rs` provides a test double.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use crate::docker::client::{DockerError, SwarmInspectResult};
use crate::docker::inventory::ContainerInfo;
use crate::docker::stream::{LogStream, LogStreamRequest};
use crate::filter::engine::FilterEngine;

/// Unified async interface over the Docker daemon.
///
/// Object-safe thanks to `Pin<Box<…>>` returns for streaming methods.
/// Implementations must be `Send + Sync` so they can live inside `Arc<AgentState>`.
pub trait DockerOps: Send + Sync {
    // ── Container queries ───────────────────────────────────────

    fn list_containers(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<ContainerInfo>, DockerError>> + Send + '_>>;

    fn inspect_container<'a>(
        &'a self,
        id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ContainerInfo, DockerError>> + Send + 'a>>;

    fn inspect_container_raw<'a>(
        &'a self,
        id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::ContainerInspectResponse, DockerError>> + Send + 'a>>;

    // ── Logs ────────────────────────────────────────────────────

    fn stream_logs(
        &self,
        request: LogStreamRequest,
        filter: Option<Arc<FilterEngine>>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<LogStream, DockerError>> + Send + '_>>;

    // ── Stats ───────────────────────────────────────────────────

    fn stats<'a>(
        &'a self,
        container_id: &'a str,
        stream: bool,
    ) -> Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        Pin<Box<dyn tokio_stream::Stream<Item = Result<bollard::models::ContainerStatsResponse, bollard::errors::Error>> + Send>>,
                        DockerError,
                    >,
                > + Send
                + 'a,
        >,
    >;

    // ── Container lifecycle ─────────────────────────────────────

    fn start_container<'a>(
        &'a self,
        container_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    fn stop_container<'a>(
        &'a self,
        container_id: &'a str,
        timeout_secs: Option<u32>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    fn restart_container<'a>(
        &'a self,
        container_id: &'a str,
        timeout_secs: Option<u32>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    fn pause_container<'a>(
        &'a self,
        container_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    fn unpause_container<'a>(
        &'a self,
        container_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    fn remove_container<'a>(
        &'a self,
        container_id: &'a str,
        force: bool,
        remove_volumes: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    // ── Images ──────────────────────────────────────────────────

    fn list_images(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::ImageSummary>, DockerError>> + Send + '_>>;

    fn inspect_image<'a>(
        &'a self,
        image_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::ImageInspect, DockerError>> + Send + 'a>>;

    fn pull_image<'a>(
        &'a self,
        image: &'a str,
        tag: &'a str,
        registry_auth: Option<&'a str>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    fn remove_image<'a>(
        &'a self,
        image_id: &'a str,
        force: bool,
        no_prune: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    // ── Volumes ─────────────────────────────────────────────────

    fn list_volumes(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::VolumeListResponse, DockerError>> + Send + '_>>;

    fn inspect_volume<'a>(
        &'a self,
        name: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::Volume, DockerError>> + Send + 'a>>;

    fn create_volume<'a>(
        &'a self,
        name: &'a str,
        driver: Option<&'a str>,
        labels: HashMap<String, String>,
        driver_opts: HashMap<String, String>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::Volume, DockerError>> + Send + 'a>>;

    fn remove_volume<'a>(
        &'a self,
        name: &'a str,
        force: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    // ── Networks ────────────────────────────────────────────────

    fn list_networks(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Network>, DockerError>> + Send + '_>>;

    fn inspect_network<'a>(
        &'a self,
        network_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::NetworkInspect, DockerError>> + Send + 'a>>;

    fn create_network<'a>(
        &'a self,
        name: &'a str,
        driver: Option<&'a str>,
        labels: HashMap<String, String>,
        internal: bool,
        attachable: bool,
        enable_ipv6: bool,
        options: HashMap<String, String>,
        ipam: Option<bollard::models::Ipam>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::NetworkCreateResponse, DockerError>> + Send + 'a>>;

    fn remove_network<'a>(
        &'a self,
        network_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    fn network_connect<'a>(
        &'a self,
        network_id: &'a str,
        container_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    fn network_disconnect<'a>(
        &'a self,
        network_id: &'a str,
        container_id: &'a str,
        force: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    // ── System ──────────────────────────────────────────────────

    fn system_info(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::SystemInfo, DockerError>> + Send + '_>>;

    // ── Swarm ───────────────────────────────────────────────────

    fn swarm_inspect(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<SwarmInspectResult, DockerError>> + Send + '_>>;

    fn list_nodes(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Node>, DockerError>> + Send + '_>>;

    fn list_services(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Service>, DockerError>> + Send + '_>>;

    fn inspect_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::Service, DockerError>> + Send + 'a>>;

    fn list_tasks(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Task>, DockerError>> + Send + '_>>;

    fn list_secrets(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Secret>, DockerError>> + Send + '_>>;

    fn list_configs(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Config>, DockerError>> + Send + '_>>;

    fn inspect_node<'a>(
        &'a self,
        node_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Option<bollard::models::Node>, DockerError>> + Send + 'a>>;

    fn update_node<'a>(
        &'a self,
        node_id: &'a str,
        spec: bollard::models::NodeSpec,
        version: i64,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    // ── Service CRUD ────────────────────────────────────────────

    fn stream_service_logs<'a>(
        &'a self,
        service_id: &'a str,
        follow: bool,
        tail: Option<String>,
        since: i32,
        until: i32,
        timestamps: bool,
    ) -> Pin<Box<dyn tokio_stream::Stream<Item = Result<bollard::container::LogOutput, bollard::errors::Error>> + Send + 'a>>;

    fn create_service<'a>(
        &'a self,
        spec: bollard::models::ServiceSpec,
        registry_auth: Option<&'a str>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + 'a>>;

    fn delete_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    fn update_service<'a>(
        &'a self,
        service_id: &'a str,
        spec: bollard::models::ServiceSpec,
        version: u64,
        force: bool,
        registry_auth: Option<&'a str>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    fn rollback_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    // ── Secret & Config CRUD ────────────────────────────────────

    fn create_secret<'a>(
        &'a self,
        name: &'a str,
        data: &'a [u8],
        labels: HashMap<String, String>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + 'a>>;

    fn delete_secret<'a>(
        &'a self,
        secret_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    fn create_config<'a>(
        &'a self,
        name: &'a str,
        data: &'a [u8],
        labels: HashMap<String, String>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + 'a>>;

    fn delete_config<'a>(
        &'a self,
        config_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    // ── Swarm Init / Join / Leave ───────────────────────────────

    fn swarm_init<'a>(
        &'a self,
        listen_addr: &'a str,
        advertise_addr: &'a str,
        force_new_cluster: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + 'a>>;

    fn swarm_join<'a>(
        &'a self,
        remote_addrs: Vec<String>,
        join_token: &'a str,
        listen_addr: &'a str,
        advertise_addr: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    fn swarm_leave(
        &self,
        force: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + '_>>;

    fn remove_node<'a>(
        &'a self,
        node_id: &'a str,
        force: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    // ── Events ──────────────────────────────────────────────────

    fn stream_events(
        &self,
        type_filters: Vec<String>,
        since: Option<i64>,
        until: Option<i64>,
    ) -> Pin<Box<dyn tokio_stream::Stream<Item = Result<bollard::models::EventMessage, DockerError>> + Send + '_>>;

    // ── Exec / Shell ────────────────────────────────────────────

    fn create_exec<'a>(
        &'a self,
        container_id: &'a str,
        cmd: Vec<String>,
        tty: bool,
        working_dir: Option<String>,
        env: Vec<String>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + 'a>>;

    fn start_exec<'a>(
        &'a self,
        exec_id: &'a str,
        tty: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::exec::StartExecResults, DockerError>> + Send + 'a>>;

    fn resize_exec<'a>(
        &'a self,
        exec_id: &'a str,
        height: u16,
        width: u16,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;

    fn inspect_exec<'a>(
        &'a self,
        exec_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::ExecInspectResponse, DockerError>> + Send + 'a>>;

    // ── Task ────────────────────────────────────────────────────

    fn inspect_task<'a>(
        &'a self,
        task_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Option<bollard::models::Task>, DockerError>> + Send + 'a>>;

    fn stream_task_logs<'a>(
        &'a self,
        task_id: &'a str,
        follow: bool,
        tail: Option<String>,
        since: i32,
        until: i32,
        timestamps: bool,
    ) -> Pin<Box<dyn tokio_stream::Stream<Item = Result<bollard::container::LogOutput, bollard::errors::Error>> + Send + 'a>>;

    // ── Swarm update / unlock ───────────────────────────────────

    fn swarm_update(
        &self,
        spec: bollard::models::SwarmSpec,
        version: i64,
        rotate_worker_token: bool,
        rotate_manager_token: bool,
        rotate_manager_unlock_key: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + '_>>;

    fn swarm_unlock_key(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + '_>>;

    fn swarm_unlock<'a>(
        &'a self,
        unlock_key: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>>;
}
