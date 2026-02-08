//! Live — implements `DockerOps` for the real Bollard-backed `DockerClient`.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use crate::client::docker::DockerOps;
use crate::docker::client::{DockerClient, DockerError, SwarmInspectResult};
use crate::docker::inventory::ContainerInfo;
use crate::docker::stream::{LogStream, LogStreamRequest};
use crate::filter::engine::FilterEngine;

impl DockerOps for DockerClient {
    // ── Container queries ───────────────────────────────────────

    fn list_containers(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<ContainerInfo>, DockerError>> + Send + '_>> {
        Box::pin(self.list_containers())
    }

    fn inspect_container<'a>(
        &'a self,
        id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<ContainerInfo, DockerError>> + Send + 'a>> {
        Box::pin(self.inspect_container(id))
    }

    fn inspect_container_raw<'a>(
        &'a self,
        id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::ContainerInspectResponse, DockerError>> + Send + 'a>> {
        Box::pin(self.inspect_container_raw(id))
    }

    // ── Logs ────────────────────────────────────────────────────

    fn stream_logs(
        &self,
        request: LogStreamRequest,
        filter: Option<Arc<FilterEngine>>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<LogStream, DockerError>> + Send + '_>> {
        Box::pin(self.stream_logs(request, filter))
    }

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
    > {
        Box::pin(async move {
            let s = DockerClient::stats(self, container_id, stream).await?;
            Ok(Box::pin(s) as Pin<Box<dyn tokio_stream::Stream<Item = _> + Send>>)
        })
    }

    // ── Container lifecycle ─────────────────────────────────────

    fn start_container<'a>(
        &'a self,
        container_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.start_container(container_id))
    }

    fn stop_container<'a>(
        &'a self,
        container_id: &'a str,
        timeout_secs: Option<u32>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.stop_container(container_id, timeout_secs))
    }

    fn restart_container<'a>(
        &'a self,
        container_id: &'a str,
        timeout_secs: Option<u32>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.restart_container(container_id, timeout_secs))
    }

    fn pause_container<'a>(
        &'a self,
        container_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.pause_container(container_id))
    }

    fn unpause_container<'a>(
        &'a self,
        container_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.unpause_container(container_id))
    }

    fn remove_container<'a>(
        &'a self,
        container_id: &'a str,
        force: bool,
        remove_volumes: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.remove_container(container_id, force, remove_volumes))
    }

    // ── Images ──────────────────────────────────────────────────

    fn list_images(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::ImageSummary>, DockerError>> + Send + '_>> {
        Box::pin(self.list_images())
    }

    fn inspect_image<'a>(
        &'a self,
        image_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::ImageInspect, DockerError>> + Send + 'a>> {
        Box::pin(self.inspect_image(image_id))
    }

    fn pull_image<'a>(
        &'a self,
        image: &'a str,
        tag: &'a str,
        registry_auth: Option<&'a str>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.pull_image(image, tag, registry_auth))
    }

    fn remove_image<'a>(
        &'a self,
        image_id: &'a str,
        force: bool,
        no_prune: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.remove_image(image_id, force, no_prune))
    }

    // ── Volumes ─────────────────────────────────────────────────

    fn list_volumes(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::VolumeListResponse, DockerError>> + Send + '_>> {
        Box::pin(self.list_volumes())
    }

    fn inspect_volume<'a>(
        &'a self,
        name: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::Volume, DockerError>> + Send + 'a>> {
        Box::pin(self.inspect_volume(name))
    }

    fn create_volume<'a>(
        &'a self,
        name: &'a str,
        driver: Option<&'a str>,
        labels: HashMap<String, String>,
        driver_opts: HashMap<String, String>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::Volume, DockerError>> + Send + 'a>> {
        Box::pin(self.create_volume(name, driver, labels, driver_opts))
    }

    fn remove_volume<'a>(
        &'a self,
        name: &'a str,
        force: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.remove_volume(name, force))
    }

    // ── Networks ────────────────────────────────────────────────

    fn list_networks(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Network>, DockerError>> + Send + '_>> {
        Box::pin(self.list_networks())
    }

    fn inspect_network<'a>(
        &'a self,
        network_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::NetworkInspect, DockerError>> + Send + 'a>> {
        Box::pin(self.inspect_network(network_id))
    }

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
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::NetworkCreateResponse, DockerError>> + Send + 'a>> {
        Box::pin(self.create_network(name, driver, labels, internal, attachable, enable_ipv6, options, ipam))
    }

    fn remove_network<'a>(
        &'a self,
        network_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.remove_network(network_id))
    }

    fn network_connect<'a>(
        &'a self,
        network_id: &'a str,
        container_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.network_connect(network_id, container_id))
    }

    fn network_disconnect<'a>(
        &'a self,
        network_id: &'a str,
        container_id: &'a str,
        force: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.network_disconnect(network_id, container_id, force))
    }

    // ── System ──────────────────────────────────────────────────

    fn system_info(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::SystemInfo, DockerError>> + Send + '_>> {
        Box::pin(self.system_info())
    }

    // ── Swarm ───────────────────────────────────────────────────

    fn swarm_inspect(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<SwarmInspectResult, DockerError>> + Send + '_>> {
        Box::pin(self.swarm_inspect())
    }

    fn list_nodes(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Node>, DockerError>> + Send + '_>> {
        Box::pin(self.list_nodes())
    }

    fn list_services(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Service>, DockerError>> + Send + '_>> {
        Box::pin(self.list_services())
    }

    fn inspect_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::Service, DockerError>> + Send + 'a>> {
        Box::pin(self.inspect_service(service_id))
    }

    fn list_tasks(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Task>, DockerError>> + Send + '_>> {
        Box::pin(self.list_tasks())
    }

    fn list_secrets(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Secret>, DockerError>> + Send + '_>> {
        Box::pin(self.list_secrets())
    }

    fn list_configs(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Vec<bollard::models::Config>, DockerError>> + Send + '_>> {
        Box::pin(self.list_configs())
    }

    fn inspect_node<'a>(
        &'a self,
        node_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Option<bollard::models::Node>, DockerError>> + Send + 'a>> {
        Box::pin(self.inspect_node(node_id))
    }

    fn update_node<'a>(
        &'a self,
        node_id: &'a str,
        spec: bollard::models::NodeSpec,
        version: i64,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.update_node(node_id, spec, version))
    }

    // ── Service CRUD ────────────────────────────────────────────

    fn stream_service_logs<'a>(
        &'a self,
        service_id: &'a str,
        follow: bool,
        tail: Option<String>,
        since: i32,
        until: i32,
        timestamps: bool,
    ) -> Pin<Box<dyn tokio_stream::Stream<Item = Result<bollard::container::LogOutput, bollard::errors::Error>> + Send + 'a>> {
        self.stream_service_logs(service_id, follow, tail, since, until, timestamps)
    }

    fn create_service<'a>(
        &'a self,
        spec: bollard::models::ServiceSpec,
        registry_auth: Option<&'a str>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + 'a>> {
        Box::pin(self.create_service(spec, registry_auth))
    }

    fn delete_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.delete_service(service_id))
    }

    fn update_service<'a>(
        &'a self,
        service_id: &'a str,
        spec: bollard::models::ServiceSpec,
        version: u64,
        force: bool,
        registry_auth: Option<&'a str>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.update_service(service_id, spec, version, force, registry_auth))
    }

    fn rollback_service<'a>(
        &'a self,
        service_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.rollback_service(service_id))
    }

    // ── Secret & Config CRUD ────────────────────────────────────

    fn create_secret<'a>(
        &'a self,
        name: &'a str,
        data: &'a [u8],
        labels: HashMap<String, String>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + 'a>> {
        Box::pin(self.create_secret(name, data, labels))
    }

    fn delete_secret<'a>(
        &'a self,
        secret_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.delete_secret(secret_id))
    }

    fn create_config<'a>(
        &'a self,
        name: &'a str,
        data: &'a [u8],
        labels: HashMap<String, String>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + 'a>> {
        Box::pin(self.create_config(name, data, labels))
    }

    fn delete_config<'a>(
        &'a self,
        config_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.delete_config(config_id))
    }

    // ── Swarm Init / Join / Leave ───────────────────────────────

    fn swarm_init<'a>(
        &'a self,
        listen_addr: &'a str,
        advertise_addr: &'a str,
        force_new_cluster: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + 'a>> {
        Box::pin(self.swarm_init(listen_addr, advertise_addr, force_new_cluster))
    }

    fn swarm_join<'a>(
        &'a self,
        remote_addrs: Vec<String>,
        join_token: &'a str,
        listen_addr: &'a str,
        advertise_addr: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.swarm_join(remote_addrs, join_token, listen_addr, advertise_addr))
    }

    fn swarm_leave(
        &self,
        force: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + '_>> {
        Box::pin(self.swarm_leave(force))
    }

    fn remove_node<'a>(
        &'a self,
        node_id: &'a str,
        force: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.remove_node(node_id, force))
    }

    // ── Events ──────────────────────────────────────────────────

    fn stream_events(
        &self,
        type_filters: Vec<String>,
        since: Option<i64>,
        until: Option<i64>,
    ) -> Pin<Box<dyn tokio_stream::Stream<Item = Result<bollard::models::EventMessage, DockerError>> + Send + '_>> {
        Box::pin(self.stream_events(type_filters, since, until))
    }

    // ── Exec / Shell ────────────────────────────────────────────

    fn create_exec<'a>(
        &'a self,
        container_id: &'a str,
        cmd: Vec<String>,
        tty: bool,
        working_dir: Option<String>,
        env: Vec<String>,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + 'a>> {
        Box::pin(self.create_exec(container_id, cmd, tty, working_dir, env))
    }

    fn start_exec<'a>(
        &'a self,
        exec_id: &'a str,
        tty: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::exec::StartExecResults, DockerError>> + Send + 'a>> {
        Box::pin(self.start_exec(exec_id, tty))
    }

    fn resize_exec<'a>(
        &'a self,
        exec_id: &'a str,
        height: u16,
        width: u16,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.resize_exec(exec_id, height, width))
    }

    fn inspect_exec<'a>(
        &'a self,
        exec_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<bollard::models::ExecInspectResponse, DockerError>> + Send + 'a>> {
        Box::pin(self.inspect_exec(exec_id))
    }

    // ── Task ────────────────────────────────────────────────────

    fn inspect_task<'a>(
        &'a self,
        task_id: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<Option<bollard::models::Task>, DockerError>> + Send + 'a>> {
        Box::pin(self.inspect_task(task_id))
    }

    fn stream_task_logs<'a>(
        &'a self,
        task_id: &'a str,
        follow: bool,
        tail: Option<String>,
        since: i32,
        until: i32,
        timestamps: bool,
    ) -> Pin<Box<dyn tokio_stream::Stream<Item = Result<bollard::container::LogOutput, bollard::errors::Error>> + Send + 'a>> {
        self.stream_task_logs(task_id, follow, tail, since, until, timestamps)
    }

    // ── Swarm update / unlock ───────────────────────────────────

    fn swarm_update(
        &self,
        spec: bollard::models::SwarmSpec,
        version: i64,
        rotate_worker_token: bool,
        rotate_manager_token: bool,
        rotate_manager_unlock_key: bool,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + '_>> {
        Box::pin(self.swarm_update(spec, version, rotate_worker_token, rotate_manager_token, rotate_manager_unlock_key))
    }

    fn swarm_unlock_key(
        &self,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<String, DockerError>> + Send + '_>> {
        Box::pin(self.swarm_unlock_key())
    }

    fn swarm_unlock<'a>(
        &'a self,
        unlock_key: &'a str,
    ) -> Pin<Box<dyn std::future::Future<Output = Result<(), DockerError>> + Send + 'a>> {
        Box::pin(self.swarm_unlock(unlock_key))
    }
}
