//! Service — swarm service CRUD spec-building and handler operations.

use tonic::Status;
use tracing::{info, warn};

use crate::docker::client::DockerClient;
use crate::swarm::map::convert_service_to_proto;
use crate::proto::{
    CreateServiceRequest, UpdateServiceRequest,
    ServiceListResponse, ServiceInfo,
    ServiceInspectResponse,
    CreateServiceResponse, DeleteServiceResponse,
    UpdateServiceResponse, RollbackServiceResponse,
};

/// List all swarm services with task counts.
pub(crate) async fn list_all(docker: &DockerClient) -> Result<ServiceListResponse, Status> {
    let services = docker.list_services().await
        .map_err(|e| Status::internal(format!("Failed to list services: {}", e)))?;
    let tasks = docker.list_tasks().await
        .map_err(|e| Status::internal(format!("Failed to list tasks: {}", e)))?;
    let service_infos: Vec<ServiceInfo> = services.into_iter()
        .map(|s| convert_service_to_proto(&s, &tasks))
        .collect();
    Ok(ServiceListResponse { services: service_infos })
}

/// Inspect a single swarm service with task counts.
pub(crate) async fn inspect_full(docker: &DockerClient, service_id: &str) -> Result<ServiceInspectResponse, Status> {
    let service = docker.inspect_service(service_id).await
        .map_err(|e| crate::docker::error_map::map_docker_error_with_context(
            &format!("inspecting service {}", service_id), e,
        ))?;
    let tasks = docker.list_tasks().await
        .map_err(|e| Status::internal(format!("Failed to list tasks: {}", e)))?;
    let info = convert_service_to_proto(&service, &tasks);
    Ok(ServiceInspectResponse { service: Some(info) })
}

/// Create a new swarm service.
pub(crate) async fn create(docker: &DockerClient, req: CreateServiceRequest) -> Result<CreateServiceResponse, Status> {
    let spec = build_create_spec(&req);
    let registry_auth_opt = if req.registry_auth.is_empty() { None } else { Some(req.registry_auth.as_str()) };
    match docker.create_service(spec, registry_auth_opt).await {
        Ok(service_id) => {
            info!(service_id = %service_id, name = %req.name, "Service created");
            Ok(CreateServiceResponse {
                service_id,
                success: true,
                message: format!("Service '{}' created successfully", req.name),
            })
        }
        Err(e) => {
            warn!(name = %req.name, "Failed to create service: {}", e);
            Ok(CreateServiceResponse {
                service_id: String::new(),
                success: false,
                message: format!("Failed to create service: {}", e),
            })
        }
    }
}

/// Delete a swarm service by ID.
pub(crate) async fn delete(docker: &DockerClient, service_id: &str) -> Result<DeleteServiceResponse, Status> {
    match docker.delete_service(service_id).await {
        Ok(()) => {
            info!(service_id = %service_id, "Service deleted");
            Ok(DeleteServiceResponse {
                success: true,
                message: format!("Service '{}' deleted successfully", service_id),
            })
        }
        Err(e) => {
            warn!(service_id = %service_id, "Failed to delete service: {}", e);
            Ok(DeleteServiceResponse {
                success: false,
                message: format!("Failed to delete service: {}", e),
            })
        }
    }
}

/// Update an existing swarm service (inspect → merge → update).
pub(crate) async fn update_existing(docker: &DockerClient, req: UpdateServiceRequest) -> Result<UpdateServiceResponse, Status> {
    let service_id = &req.service_id;
    let current = docker.inspect_service(service_id).await
        .map_err(|e| crate::docker::error_map::map_docker_error_with_context(
            &format!("updating service {}", service_id), e,
        ))?;

    let version = current.version.as_ref()
        .and_then(|v| v.index)
        .unwrap_or(0);

    let spec = apply_update(current.spec.unwrap_or_default(), &req);
    let registry_auth_opt = if req.registry_auth.is_empty() { None } else { Some(req.registry_auth.as_str()) };

    match docker.update_service(service_id, spec, version, req.force, registry_auth_opt).await {
        Ok(()) => {
            info!(service_id = %service_id, "Service updated");
            Ok(UpdateServiceResponse {
                success: true,
                message: format!("Service '{}' updated successfully", service_id),
            })
        }
        Err(e) => {
            warn!(service_id = %service_id, "Failed to update service: {}", e);
            Ok(UpdateServiceResponse {
                success: false,
                message: format!("Failed to update service: {}", e),
            })
        }
    }
}

/// Rollback a service to its previous spec.
pub(crate) async fn rollback(docker: &DockerClient, service_id: &str) -> Result<RollbackServiceResponse, Status> {
    match docker.rollback_service(service_id).await {
        Ok(()) => {
            info!(service_id = %service_id, "Service rollback initiated");
            Ok(RollbackServiceResponse {
                success: true,
                message: format!("Service {} rollback initiated", service_id),
            })
        }
        Err(e) => Err(Status::internal(format!("Failed to rollback service: {}", e))),
    }
}

/// Build a `ServiceSpec` from a `CreateServiceRequest`.
pub(crate) fn build_create_spec(
    req: &CreateServiceRequest,
) -> bollard::models::ServiceSpec {
    let mode = if req.global {
        Some(bollard::models::ServiceSpecMode {
            global: Some(Default::default()),
            ..Default::default()
        })
    } else {
        Some(bollard::models::ServiceSpecMode {
            replicated: Some(bollard::models::ServiceSpecModeReplicated {
                replicas: Some(if req.replicas > 0 { req.replicas as i64 } else { 1 }),
            }),
            ..Default::default()
        })
    };

    let endpoint_spec = convert_ports_to_endpoint_spec(&req.ports);

    let env: Vec<String> = req.env.iter()
        .map(|(k, v)| format!("{}={}", k, v))
        .collect();

    let networks = if !req.networks.is_empty() {
        Some(req.networks.iter().map(|n| {
            bollard::models::NetworkAttachmentConfig {
                target: Some(n.clone()),
                ..Default::default()
            }
        }).collect::<Vec<_>>())
    } else {
        None
    };

    let mounts = convert_mounts(&req.mounts);

    let resources = {
        let limits = req.resource_limits.as_ref().map(|rl| {
            bollard::models::Limit {
                nano_cpus: if rl.nano_cpus > 0 { Some(rl.nano_cpus) } else { None },
                memory_bytes: if rl.memory_bytes > 0 { Some(rl.memory_bytes) } else { None },
                pids: None,
            }
        });
        let reservations = req.resource_reservations.as_ref().map(|rr| {
            bollard::models::ResourceObject {
                nano_cpus: if rr.nano_cpus > 0 { Some(rr.nano_cpus) } else { None },
                memory_bytes: if rr.memory_bytes > 0 { Some(rr.memory_bytes) } else { None },
                ..Default::default()
            }
        });
        if limits.is_some() || reservations.is_some() {
            Some(bollard::models::TaskSpecResources {
                limits,
                reservations,
                swap_bytes: None,
                memory_swappiness: None,
            })
        } else {
            None
        }
    };

    let restart_policy = req.restart_policy.as_ref().map(convert_restart_policy);
    let update_config = req.update_config.as_ref().map(convert_update_config);
    let rollback_config = req.rollback_config.as_ref().map(convert_rollback_config);

    let health_check = req.health_check.as_ref().map(|hc| {
        bollard::models::HealthConfig {
            test: if hc.test.is_empty() { None } else { Some(hc.test.clone()) },
            interval: if hc.interval_ns > 0 { Some(hc.interval_ns) } else { None },
            timeout: if hc.timeout_ns > 0 { Some(hc.timeout_ns) } else { None },
            start_period: if hc.start_period_ns > 0 { Some(hc.start_period_ns) } else { None },
            retries: if hc.retries > 0 { Some(hc.retries as i64) } else { None },
            ..Default::default()
        }
    });

    let secrets = if !req.secrets.is_empty() {
        Some(req.secrets.iter().map(|s| {
            bollard::models::TaskSpecContainerSpecSecrets {
                file: Some(bollard::models::TaskSpecContainerSpecFile {
                    name: Some(s.file_name.clone()),
                    uid: if s.file_uid.is_empty() { None } else { Some(s.file_uid.clone()) },
                    gid: if s.file_gid.is_empty() { None } else { Some(s.file_gid.clone()) },
                    mode: Some(if s.file_mode > 0 { s.file_mode } else { 0o444 }),
                }),
                secret_id: Some(s.secret_id.clone()),
                secret_name: Some(s.secret_name.clone()),
            }
        }).collect::<Vec<_>>())
    } else {
        None
    };

    let configs = if !req.configs.is_empty() {
        Some(req.configs.iter().map(|c| {
            bollard::models::TaskSpecContainerSpecConfigs {
                file: Some(bollard::models::TaskSpecContainerSpecFile1 {
                    name: Some(c.file_name.clone()),
                    uid: if c.file_uid.is_empty() { None } else { Some(c.file_uid.clone()) },
                    gid: if c.file_gid.is_empty() { None } else { Some(c.file_gid.clone()) },
                    mode: Some(if c.file_mode > 0 { c.file_mode } else { 0o444 }),
                }),
                config_id: Some(c.config_id.clone()),
                config_name: Some(c.config_name.clone()),
                ..Default::default()
            }
        }).collect::<Vec<_>>())
    } else {
        None
    };

    let log_driver = if !req.log_driver.is_empty() {
        Some(bollard::models::TaskSpecLogDriver {
            name: Some(req.log_driver.clone()),
            options: if req.log_driver_opts.is_empty() { None } else { Some(req.log_driver_opts.clone()) },
        })
    } else {
        None
    };

    bollard::models::ServiceSpec {
        name: Some(req.name.clone()),
        mode,
        task_template: Some(bollard::models::TaskSpec {
            container_spec: Some(bollard::models::TaskSpecContainerSpec {
                image: Some(req.image.clone()),
                env: if env.is_empty() { None } else { Some(env) },
                command: if req.command.is_empty() { None } else { Some(req.command.clone()) },
                mounts,
                health_check,
                secrets,
                configs,
                ..Default::default()
            }),
            networks,
            placement: if req.constraints.is_empty() {
                None
            } else {
                Some(bollard::models::TaskSpecPlacement {
                    constraints: Some(req.constraints.clone()),
                    ..Default::default()
                })
            },
            resources,
            restart_policy,
            log_driver,
            ..Default::default()
        }),
        labels: if req.labels.is_empty() { None } else { Some(req.labels.clone()) },
        endpoint_spec,
        update_config,
        rollback_config,
        ..Default::default()
    }
}

/// Apply partial updates from `UpdateServiceRequest` onto an existing
/// `ServiceSpec`, returning the mutated spec.
pub(crate) fn apply_update(
    mut spec: bollard::models::ServiceSpec,
    req: &UpdateServiceRequest,
) -> bollard::models::ServiceSpec {
    // Image
    if let Some(ref image) = req.image {
        if let Some(ref mut tt) = spec.task_template {
            if let Some(ref mut cs) = tt.container_spec {
                cs.image = Some(image.clone());
            }
        }
    }

    // Replicas
    if let Some(replicas) = req.replicas {
        if let Some(ref mut mode) = spec.mode {
            if let Some(ref mut replicated) = mode.replicated {
                replicated.replicas = Some(replicas as i64);
            }
        }
    }

    // Environment
    if !req.env.is_empty() || req.clear_env {
        if let Some(ref mut tt) = spec.task_template {
            if let Some(ref mut cs) = tt.container_spec {
                cs.env = if req.env.is_empty() {
                    Some(Vec::new())
                } else {
                    Some(req.env.iter().map(|(k, v)| format!("{}={}", k, v)).collect())
                };
            }
        }
    }

    // Labels
    if !req.labels.is_empty() || req.clear_labels {
        spec.labels = Some(req.labels.clone());
    }

    // Networks
    if !req.networks.is_empty() || req.clear_networks {
        if let Some(ref mut tt) = spec.task_template {
            tt.networks = if req.networks.is_empty() {
                Some(Vec::new())
            } else {
                Some(req.networks.iter().map(|n| {
                    bollard::models::NetworkAttachmentConfig {
                        target: Some(n.clone()),
                        ..Default::default()
                    }
                }).collect())
            };
        }
    }

    // Ports
    if !req.ports.is_empty() || req.clear_ports {
        spec.endpoint_spec = if req.ports.is_empty() {
            Some(bollard::models::EndpointSpec {
                ports: Some(Vec::new()),
                ..Default::default()
            })
        } else {
            convert_ports_to_endpoint_spec(&req.ports)
        };
    }

    // Resource limits / reservations
    if req.resource_limits.is_some() || req.resource_reservations.is_some() {
        if let Some(ref mut tt) = spec.task_template {
            let mut resources = tt.resources.take().unwrap_or_default();
            if let Some(ref rl) = req.resource_limits {
                resources.limits = Some(bollard::models::Limit {
                    nano_cpus: if rl.nano_cpus > 0 { Some(rl.nano_cpus) } else { None },
                    memory_bytes: if rl.memory_bytes > 0 { Some(rl.memory_bytes) } else { None },
                    pids: None,
                });
            }
            if let Some(ref rr) = req.resource_reservations {
                resources.reservations = Some(bollard::models::ResourceObject {
                    nano_cpus: if rr.nano_cpus > 0 { Some(rr.nano_cpus) } else { None },
                    memory_bytes: if rr.memory_bytes > 0 { Some(rr.memory_bytes) } else { None },
                    ..Default::default()
                });
            }
            tt.resources = Some(resources);
        }
    }

    // Mounts
    if !req.mounts.is_empty() || req.clear_mounts {
        if let Some(ref mut tt) = spec.task_template {
            if let Some(ref mut cs) = tt.container_spec {
                cs.mounts = if req.mounts.is_empty() {
                    Some(Vec::new())
                } else {
                    convert_mounts(&req.mounts)
                };
            }
        }
    }

    // Restart policy
    if let Some(ref rp) = req.restart_policy {
        if let Some(ref mut tt) = spec.task_template {
            tt.restart_policy = Some(convert_restart_policy(rp));
        }
    }

    // Update config
    if let Some(ref uc) = req.update_config {
        spec.update_config = Some(convert_update_config(uc));
    }

    // Rollback config
    if let Some(ref rc) = req.rollback_config {
        spec.rollback_config = Some(convert_rollback_config(rc));
    }

    // Constraints
    if !req.constraints.is_empty() || req.clear_constraints {
        if let Some(ref mut tt) = spec.task_template {
            let mut placement = tt.placement.take().unwrap_or_default();
            placement.constraints = Some(req.constraints.clone());
            tt.placement = Some(placement);
        }
    }

    // Command
    if !req.command.is_empty() || req.clear_command {
        if let Some(ref mut tt) = spec.task_template {
            if let Some(ref mut cs) = tt.container_spec {
                cs.command = if req.command.is_empty() { None } else { Some(req.command.clone()) };
            }
        }
    }

    // Force update
    if req.force {
        if let Some(ref mut tt) = spec.task_template {
            let current_force = tt.force_update.unwrap_or(0);
            tt.force_update = Some(current_force + 1);
        }
    }

    spec
}

// ── Shared spec-building helpers ────────────────────────────────────────

/// Convert a slice of proto [`ServicePortConfig`] into a bollard `EndpointSpec`.
/// Returns `None` when the slice is empty.
fn convert_ports_to_endpoint_spec(ports: &[crate::proto::ServicePortConfig]) -> Option<bollard::models::EndpointSpec> {
    if ports.is_empty() {
        return None;
    }
    Some(bollard::models::EndpointSpec {
        ports: Some(ports.iter().map(convert_port_config).collect()),
        ..Default::default()
    })
}

/// Convert a single proto port config to a bollard `EndpointPortConfig`.
fn convert_port_config(p: &crate::proto::ServicePortConfig) -> bollard::models::EndpointPortConfig {
    bollard::models::EndpointPortConfig {
        protocol: if p.protocol.is_empty() {
            Some(bollard::models::EndpointPortConfigProtocolEnum::TCP)
        } else {
            match p.protocol.to_lowercase().as_str() {
                "udp" => Some(bollard::models::EndpointPortConfigProtocolEnum::UDP),
                "sctp" => Some(bollard::models::EndpointPortConfigProtocolEnum::SCTP),
                _ => Some(bollard::models::EndpointPortConfigProtocolEnum::TCP),
            }
        },
        target_port: Some(p.target_port as i64),
        published_port: if p.published_port > 0 { Some(p.published_port as i64) } else { None },
        publish_mode: if p.publish_mode == "host" {
            Some(bollard::models::EndpointPortConfigPublishModeEnum::HOST)
        } else {
            Some(bollard::models::EndpointPortConfigPublishModeEnum::INGRESS)
        },
        ..Default::default()
    }
}

/// Convert a slice of proto [`ServiceMount`] to bollard `Mount` objects.
/// Returns `None` when the slice is empty.
fn convert_mounts(mounts: &[crate::proto::ServiceMount]) -> Option<Vec<bollard::models::Mount>> {
    if mounts.is_empty() {
        return None;
    }
    Some(mounts.iter().map(|m| {
        bollard::models::Mount {
            target: Some(m.target.clone()),
            source: Some(m.source.clone()),
            typ: Some(match m.r#type.as_str() {
                "volume" => bollard::models::MountTypeEnum::VOLUME,
                "tmpfs" => bollard::models::MountTypeEnum::TMPFS,
                _ => bollard::models::MountTypeEnum::BIND,
            }),
            read_only: Some(m.read_only),
            ..Default::default()
        }
    }).collect())
}

/// Convert a proto [`ServiceRestartPolicy`] to a bollard `TaskSpecRestartPolicy`.
fn convert_restart_policy(rp: &crate::proto::ServiceRestartPolicy) -> bollard::models::TaskSpecRestartPolicy {
    bollard::models::TaskSpecRestartPolicy {
        condition: Some(match rp.condition.as_str() {
            "none" => bollard::models::TaskSpecRestartPolicyConditionEnum::NONE,
            "on-failure" => bollard::models::TaskSpecRestartPolicyConditionEnum::ON_FAILURE,
            _ => bollard::models::TaskSpecRestartPolicyConditionEnum::ANY,
        }),
        delay: if rp.delay_ns > 0 { Some(rp.delay_ns) } else { None },
        max_attempts: if rp.max_attempts > 0 { Some(rp.max_attempts as i64) } else { None },
        window: if rp.window_ns > 0 { Some(rp.window_ns) } else { None },
    }
}

/// Convert a proto [`ServiceUpdateConfig`] to a bollard `ServiceSpecUpdateConfig`.
fn convert_update_config(uc: &crate::proto::ServiceUpdateConfig) -> bollard::models::ServiceSpecUpdateConfig {
    bollard::models::ServiceSpecUpdateConfig {
        parallelism: Some(uc.parallelism as i64),
        delay: if uc.delay_ns > 0 { Some(uc.delay_ns) } else { None },
        failure_action: Some(match uc.failure_action.as_str() {
            "continue" => bollard::models::ServiceSpecUpdateConfigFailureActionEnum::CONTINUE,
            "rollback" => bollard::models::ServiceSpecUpdateConfigFailureActionEnum::ROLLBACK,
            _ => bollard::models::ServiceSpecUpdateConfigFailureActionEnum::PAUSE,
        }),
        monitor: if uc.monitor_ns > 0 { Some(uc.monitor_ns) } else { None },
        max_failure_ratio: Some(uc.max_failure_ratio),
        order: Some(match uc.order.as_str() {
            "start-first" => bollard::models::ServiceSpecUpdateConfigOrderEnum::START_FIRST,
            _ => bollard::models::ServiceSpecUpdateConfigOrderEnum::STOP_FIRST,
        }),
    }
}

/// Convert a proto [`ServiceUpdateConfig`] (used for rollback too) to a bollard
/// `ServiceSpecRollbackConfig`.
fn convert_rollback_config(rc: &crate::proto::ServiceUpdateConfig) -> bollard::models::ServiceSpecRollbackConfig {
    bollard::models::ServiceSpecRollbackConfig {
        parallelism: Some(rc.parallelism as i64),
        delay: if rc.delay_ns > 0 { Some(rc.delay_ns) } else { None },
        failure_action: Some(match rc.failure_action.as_str() {
            "continue" => bollard::models::ServiceSpecRollbackConfigFailureActionEnum::CONTINUE,
            _ => bollard::models::ServiceSpecRollbackConfigFailureActionEnum::PAUSE,
        }),
        monitor: if rc.monitor_ns > 0 { Some(rc.monitor_ns) } else { None },
        max_failure_ratio: Some(rc.max_failure_ratio),
        order: Some(match rc.order.as_str() {
            "start-first" => bollard::models::ServiceSpecRollbackConfigOrderEnum::START_FIRST,
            _ => bollard::models::ServiceSpecRollbackConfigOrderEnum::STOP_FIRST,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use crate::proto::{
        CreateServiceRequest, UpdateServiceRequest,
        ServicePortConfig, ServiceMount,
        ServiceResourceLimits, ServiceResourceReservations,
        ServiceRestartPolicy as ProtoRestartPolicy,
        ServiceUpdateConfig as ProtoUpdateConfig,
    };

    // ── Helper ──────────────────────────────────────────────────

    fn minimal_create_request() -> CreateServiceRequest {
        CreateServiceRequest {
            name: "web".to_string(),
            image: "nginx:latest".to_string(),
            replicas: 3,
            global: false,
            ports: vec![],
            env: HashMap::new(),
            labels: HashMap::new(),
            networks: vec![],
            command: vec![],
            constraints: vec![],
            resource_limits: None,
            resource_reservations: None,
            mounts: vec![],
            restart_policy: None,
            update_config: None,
            rollback_config: None,
            secrets: vec![],
            configs: vec![],
            health_check: None,
            registry_auth: String::new(),
            log_driver: String::new(),
            log_driver_opts: HashMap::new(),
        }
    }

    fn minimal_update_request(service_id: &str) -> UpdateServiceRequest {
        UpdateServiceRequest {
            service_id: service_id.to_string(),
            image: None,
            replicas: None,
            force: false,
            env: HashMap::new(),
            labels: HashMap::new(),
            networks: vec![],
            ports: vec![],
            resource_limits: None,
            resource_reservations: None,
            mounts: vec![],
            restart_policy: None,
            update_config: None,
            rollback_config: None,
            constraints: vec![],
            command: vec![],
            registry_auth: String::new(),
            clear_env: false,
            clear_labels: false,
            clear_networks: false,
            clear_ports: false,
            clear_mounts: false,
            clear_constraints: false,
            clear_command: false,
        }
    }

    fn base_spec() -> bollard::models::ServiceSpec {
        bollard::models::ServiceSpec {
            name: Some("old-web".to_string()),
            mode: Some(bollard::models::ServiceSpecMode {
                replicated: Some(bollard::models::ServiceSpecModeReplicated {
                    replicas: Some(2),
                }),
                ..Default::default()
            }),
            task_template: Some(bollard::models::TaskSpec {
                container_spec: Some(bollard::models::TaskSpecContainerSpec {
                    image: Some("nginx:1.0".to_string()),
                    env: Some(vec!["FOO=bar".to_string()]),
                    command: Some(vec!["nginx".to_string()]),
                    ..Default::default()
                }),
                networks: Some(vec![bollard::models::NetworkAttachmentConfig {
                    target: Some("old-net".to_string()),
                    ..Default::default()
                }]),
                placement: Some(bollard::models::TaskSpecPlacement {
                    constraints: Some(vec!["node.role == manager".to_string()]),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            labels: Some({
                let mut m = HashMap::new();
                m.insert("app".to_string(), "web".to_string());
                m
            }),
            endpoint_spec: Some(bollard::models::EndpointSpec {
                ports: Some(vec![bollard::models::EndpointPortConfig {
                    target_port: Some(80),
                    published_port: Some(8080),
                    protocol: Some(bollard::models::EndpointPortConfigProtocolEnum::TCP),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    // ── build_create_spec ───────────────────────────────────────

    #[test]
    fn create_spec_basic_replicated() {
        let req = minimal_create_request();
        let spec = build_create_spec(&req);

        assert_eq!(spec.name, Some("web".to_string()));
        let mode = spec.mode.unwrap();
        let replicas = mode.replicated.unwrap().replicas.unwrap();
        assert_eq!(replicas, 3);
        assert!(mode.global.is_none());

        let cs = spec.task_template.unwrap().container_spec.unwrap();
        assert_eq!(cs.image, Some("nginx:latest".to_string()));
    }

    #[test]
    fn create_spec_global_mode() {
        let mut req = minimal_create_request();
        req.global = true;
        let spec = build_create_spec(&req);

        let mode = spec.mode.unwrap();
        assert!(mode.global.is_some());
        assert!(mode.replicated.is_none());
    }

    #[test]
    fn create_spec_default_replicas_when_zero() {
        let mut req = minimal_create_request();
        req.replicas = 0;
        let spec = build_create_spec(&req);

        let mode = spec.mode.unwrap();
        let replicas = mode.replicated.unwrap().replicas.unwrap();
        assert_eq!(replicas, 1); // defaults to 1
    }

    #[test]
    fn create_spec_with_ports() {
        let mut req = minimal_create_request();
        req.ports = vec![
            ServicePortConfig {
                target_port: 80,
                published_port: 8080,
                protocol: "tcp".to_string(),
                publish_mode: "ingress".to_string(),
            },
            ServicePortConfig {
                target_port: 443,
                published_port: 8443,
                protocol: "tcp".to_string(),
                publish_mode: "host".to_string(),
            },
        ];
        let spec = build_create_spec(&req);

        let endpoint = spec.endpoint_spec.unwrap();
        let ports = endpoint.ports.unwrap();
        assert_eq!(ports.len(), 2);
        assert_eq!(ports[0].target_port, Some(80));
        assert_eq!(ports[0].published_port, Some(8080));
        assert_eq!(ports[1].publish_mode, Some(bollard::models::EndpointPortConfigPublishModeEnum::HOST));
    }

    #[test]
    fn create_spec_with_env() {
        let mut req = minimal_create_request();
        req.env.insert("DB_HOST".to_string(), "localhost".to_string());
        req.env.insert("DB_PORT".to_string(), "5432".to_string());
        let spec = build_create_spec(&req);

        let env = spec.task_template.unwrap().container_spec.unwrap().env.unwrap();
        assert_eq!(env.len(), 2);
        assert!(env.iter().any(|e| e == "DB_HOST=localhost"));
        assert!(env.iter().any(|e| e == "DB_PORT=5432"));
    }

    #[test]
    fn create_spec_with_labels() {
        let mut req = minimal_create_request();
        req.labels.insert("app".to_string(), "web".to_string());
        let spec = build_create_spec(&req);
        assert_eq!(spec.labels.unwrap().get("app").unwrap(), "web");
    }

    #[test]
    fn create_spec_with_networks() {
        let mut req = minimal_create_request();
        req.networks = vec!["frontend".to_string(), "backend".to_string()];
        let spec = build_create_spec(&req);

        let nets = spec.task_template.unwrap().networks.unwrap();
        assert_eq!(nets.len(), 2);
        assert_eq!(nets[0].target, Some("frontend".to_string()));
        assert_eq!(nets[1].target, Some("backend".to_string()));
    }

    #[test]
    fn create_spec_with_command() {
        let mut req = minimal_create_request();
        req.command = vec!["python".to_string(), "app.py".to_string()];
        let spec = build_create_spec(&req);

        let cmd = spec.task_template.unwrap().container_spec.unwrap().command.unwrap();
        assert_eq!(cmd, vec!["python", "app.py"]);
    }

    #[test]
    fn create_spec_with_constraints() {
        let mut req = minimal_create_request();
        req.constraints = vec!["node.role == manager".to_string()];
        let spec = build_create_spec(&req);

        let placement = spec.task_template.unwrap().placement.unwrap();
        assert_eq!(placement.constraints.unwrap(), vec!["node.role == manager"]);
    }

    #[test]
    fn create_spec_with_mounts() {
        let mut req = minimal_create_request();
        req.mounts = vec![
            ServiceMount {
                r#type: "volume".to_string(),
                source: "data".to_string(),
                target: "/var/data".to_string(),
                read_only: false,
            },
            ServiceMount {
                r#type: "bind".to_string(),
                source: "/host/config".to_string(),
                target: "/etc/config".to_string(),
                read_only: true,
            },
        ];
        let spec = build_create_spec(&req);

        let mounts = spec.task_template.unwrap().container_spec.unwrap().mounts.unwrap();
        assert_eq!(mounts.len(), 2);
        assert_eq!(mounts[0].typ, Some(bollard::models::MountTypeEnum::VOLUME));
        assert_eq!(mounts[0].target, Some("/var/data".to_string()));
        assert_eq!(mounts[1].typ, Some(bollard::models::MountTypeEnum::BIND));
        assert_eq!(mounts[1].read_only, Some(true));
    }

    #[test]
    fn create_spec_with_resource_limits() {
        let mut req = minimal_create_request();
        req.resource_limits = Some(ServiceResourceLimits {
            nano_cpus: 2_000_000_000,
            memory_bytes: 536_870_912,
        });
        req.resource_reservations = Some(ServiceResourceReservations {
            nano_cpus: 1_000_000_000,
            memory_bytes: 268_435_456,
        });
        let spec = build_create_spec(&req);

        let resources = spec.task_template.unwrap().resources.unwrap();
        let limits = resources.limits.unwrap();
        assert_eq!(limits.nano_cpus, Some(2_000_000_000));
        assert_eq!(limits.memory_bytes, Some(536_870_912));
        let reservations = resources.reservations.unwrap();
        assert_eq!(reservations.nano_cpus, Some(1_000_000_000));
    }

    #[test]
    fn create_spec_with_restart_policy() {
        let mut req = minimal_create_request();
        req.restart_policy = Some(ProtoRestartPolicy {
            condition: "on-failure".to_string(),
            delay_ns: 5_000_000_000,
            max_attempts: 3,
            window_ns: 60_000_000_000,
        });
        let spec = build_create_spec(&req);

        let rp = spec.task_template.unwrap().restart_policy.unwrap();
        assert_eq!(rp.condition, Some(bollard::models::TaskSpecRestartPolicyConditionEnum::ON_FAILURE));
        assert_eq!(rp.delay, Some(5_000_000_000));
        assert_eq!(rp.max_attempts, Some(3));
        assert_eq!(rp.window, Some(60_000_000_000));
    }

    #[test]
    fn create_spec_restart_policy_none() {
        let mut req = minimal_create_request();
        req.restart_policy = Some(ProtoRestartPolicy {
            condition: "none".to_string(),
            delay_ns: 0,
            max_attempts: 0,
            window_ns: 0,
        });
        let spec = build_create_spec(&req);

        let rp = spec.task_template.unwrap().restart_policy.unwrap();
        assert_eq!(rp.condition, Some(bollard::models::TaskSpecRestartPolicyConditionEnum::NONE));
    }

    #[test]
    fn create_spec_restart_policy_any_default() {
        let mut req = minimal_create_request();
        req.restart_policy = Some(ProtoRestartPolicy {
            condition: "any".to_string(),
            delay_ns: 0,
            max_attempts: 0,
            window_ns: 0,
        });
        let spec = build_create_spec(&req);

        let rp = spec.task_template.unwrap().restart_policy.unwrap();
        assert_eq!(rp.condition, Some(bollard::models::TaskSpecRestartPolicyConditionEnum::ANY));
    }

    #[test]
    fn create_spec_with_update_config() {
        let mut req = minimal_create_request();
        req.update_config = Some(ProtoUpdateConfig {
            parallelism: 2,
            delay_ns: 10_000_000_000,
            failure_action: "rollback".to_string(),
            monitor_ns: 5_000_000_000,
            max_failure_ratio: 0.1,
            order: "start-first".to_string(),
        });
        let spec = build_create_spec(&req);

        let uc = spec.update_config.unwrap();
        assert_eq!(uc.parallelism, Some(2));
        assert_eq!(uc.delay, Some(10_000_000_000));
        assert_eq!(uc.failure_action, Some(bollard::models::ServiceSpecUpdateConfigFailureActionEnum::ROLLBACK));
        assert_eq!(uc.order, Some(bollard::models::ServiceSpecUpdateConfigOrderEnum::START_FIRST));
    }

    #[test]
    fn create_spec_with_rollback_config() {
        let mut req = minimal_create_request();
        req.rollback_config = Some(ProtoUpdateConfig {
            parallelism: 1,
            delay_ns: 0,
            failure_action: "continue".to_string(),
            monitor_ns: 0,
            max_failure_ratio: 0.0,
            order: "stop-first".to_string(),
        });
        let spec = build_create_spec(&req);

        let rc = spec.rollback_config.unwrap();
        assert_eq!(rc.parallelism, Some(1));
        assert_eq!(rc.failure_action, Some(bollard::models::ServiceSpecRollbackConfigFailureActionEnum::CONTINUE));
        assert_eq!(rc.order, Some(bollard::models::ServiceSpecRollbackConfigOrderEnum::STOP_FIRST));
    }

    #[test]
    fn create_spec_with_log_driver() {
        let mut req = minimal_create_request();
        req.log_driver = "syslog".to_string();
        req.log_driver_opts.insert("syslog-address".to_string(), "tcp://localhost:514".to_string());
        let spec = build_create_spec(&req);

        let ld = spec.task_template.unwrap().log_driver.unwrap();
        assert_eq!(ld.name, Some("syslog".to_string()));
        assert_eq!(ld.options.unwrap().get("syslog-address").unwrap(), "tcp://localhost:514");
    }

    #[test]
    fn create_spec_empty_ports_no_endpoint() {
        let req = minimal_create_request();
        let spec = build_create_spec(&req);
        assert!(spec.endpoint_spec.is_none());
    }

    #[test]
    fn create_spec_empty_env_none() {
        let req = minimal_create_request();
        let spec = build_create_spec(&req);
        let cs = spec.task_template.unwrap().container_spec.unwrap();
        assert!(cs.env.is_none());
    }

    // ── convert_port_config ─────────────────────────────────────

    #[test]
    fn port_config_tcp_default() {
        let p = ServicePortConfig {
            target_port: 80,
            published_port: 8080,
            protocol: "".to_string(),
            publish_mode: "".to_string(),
        };
        let epc = convert_port_config(&p);
        assert_eq!(epc.protocol, Some(bollard::models::EndpointPortConfigProtocolEnum::TCP));
        assert_eq!(epc.publish_mode, Some(bollard::models::EndpointPortConfigPublishModeEnum::INGRESS));
    }

    #[test]
    fn port_config_udp_host() {
        let p = ServicePortConfig {
            target_port: 53,
            published_port: 53,
            protocol: "udp".to_string(),
            publish_mode: "host".to_string(),
        };
        let epc = convert_port_config(&p);
        assert_eq!(epc.protocol, Some(bollard::models::EndpointPortConfigProtocolEnum::UDP));
        assert_eq!(epc.publish_mode, Some(bollard::models::EndpointPortConfigPublishModeEnum::HOST));
    }

    #[test]
    fn port_config_sctp() {
        let p = ServicePortConfig {
            target_port: 9999,
            published_port: 0,
            protocol: "sctp".to_string(),
            publish_mode: "ingress".to_string(),
        };
        let epc = convert_port_config(&p);
        assert_eq!(epc.protocol, Some(bollard::models::EndpointPortConfigProtocolEnum::SCTP));
        assert!(epc.published_port.is_none()); // 0 → None
    }

    // ── convert_mounts ──────────────────────────────────────────

    #[test]
    fn mounts_volume() {
        let mounts = convert_mounts(&[ServiceMount {
            r#type: "volume".to_string(),
            source: "data".to_string(),
            target: "/data".to_string(),
            read_only: false,
        }]);
        let m = &mounts.unwrap()[0];
        assert_eq!(m.typ, Some(bollard::models::MountTypeEnum::VOLUME));
    }

    #[test]
    fn mounts_bind() {
        let mounts = convert_mounts(&[ServiceMount {
            r#type: "bind".to_string(),
            source: "/host".to_string(),
            target: "/container".to_string(),
            read_only: true,
        }]);
        let m = &mounts.unwrap()[0];
        assert_eq!(m.typ, Some(bollard::models::MountTypeEnum::BIND));
        assert_eq!(m.read_only, Some(true));
    }

    #[test]
    fn mounts_tmpfs() {
        let mounts = convert_mounts(&[ServiceMount {
            r#type: "tmpfs".to_string(),
            source: "".to_string(),
            target: "/tmp".to_string(),
            read_only: false,
        }]);
        let m = &mounts.unwrap()[0];
        assert_eq!(m.typ, Some(bollard::models::MountTypeEnum::TMPFS));
    }

    #[test]
    fn mounts_empty_returns_none() {
        let mounts = convert_mounts(&[]);
        assert!(mounts.is_none());
    }

    // ── convert_restart_policy ──────────────────────────────────

    #[test]
    fn restart_policy_on_failure() {
        let rp = convert_restart_policy(&ProtoRestartPolicy {
            condition: "on-failure".to_string(),
            delay_ns: 5_000_000_000,
            max_attempts: 3,
            window_ns: 120_000_000_000,
        });
        assert_eq!(rp.condition, Some(bollard::models::TaskSpecRestartPolicyConditionEnum::ON_FAILURE));
        assert_eq!(rp.delay, Some(5_000_000_000));
        assert_eq!(rp.max_attempts, Some(3));
        assert_eq!(rp.window, Some(120_000_000_000));
    }

    #[test]
    fn restart_policy_none_condition() {
        let rp = convert_restart_policy(&ProtoRestartPolicy {
            condition: "none".to_string(),
            delay_ns: 0,
            max_attempts: 0,
            window_ns: 0,
        });
        assert_eq!(rp.condition, Some(bollard::models::TaskSpecRestartPolicyConditionEnum::NONE));
        assert!(rp.delay.is_none());
        assert!(rp.max_attempts.is_none());
    }

    #[test]
    fn restart_policy_any_default() {
        let rp = convert_restart_policy(&ProtoRestartPolicy {
            condition: "unknown-value".to_string(),
            delay_ns: 0,
            max_attempts: 0,
            window_ns: 0,
        });
        assert_eq!(rp.condition, Some(bollard::models::TaskSpecRestartPolicyConditionEnum::ANY));
    }

    // ── convert_update_config ───────────────────────────────────

    #[test]
    fn update_config_rollback_start_first() {
        let uc = convert_update_config(&ProtoUpdateConfig {
            parallelism: 2,
            delay_ns: 10_000_000_000,
            failure_action: "rollback".to_string(),
            monitor_ns: 5_000_000_000,
            max_failure_ratio: 0.25,
            order: "start-first".to_string(),
        });
        assert_eq!(uc.parallelism, Some(2));
        assert_eq!(uc.delay, Some(10_000_000_000));
        assert_eq!(uc.failure_action, Some(bollard::models::ServiceSpecUpdateConfigFailureActionEnum::ROLLBACK));
        assert_eq!(uc.order, Some(bollard::models::ServiceSpecUpdateConfigOrderEnum::START_FIRST));
        assert_eq!(uc.max_failure_ratio, Some(0.25));
    }

    #[test]
    fn update_config_continue_action() {
        let uc = convert_update_config(&ProtoUpdateConfig {
            parallelism: 1,
            delay_ns: 0,
            failure_action: "continue".to_string(),
            monitor_ns: 0,
            max_failure_ratio: 0.0,
            order: "stop-first".to_string(),
        });
        assert_eq!(uc.failure_action, Some(bollard::models::ServiceSpecUpdateConfigFailureActionEnum::CONTINUE));
        assert_eq!(uc.order, Some(bollard::models::ServiceSpecUpdateConfigOrderEnum::STOP_FIRST));
    }

    #[test]
    fn update_config_pause_default() {
        let uc = convert_update_config(&ProtoUpdateConfig {
            parallelism: 1,
            delay_ns: 0,
            failure_action: "unknown".to_string(),
            monitor_ns: 0,
            max_failure_ratio: 0.0,
            order: "unknown".to_string(),
        });
        assert_eq!(uc.failure_action, Some(bollard::models::ServiceSpecUpdateConfigFailureActionEnum::PAUSE));
        assert_eq!(uc.order, Some(bollard::models::ServiceSpecUpdateConfigOrderEnum::STOP_FIRST));
    }

    // ── convert_rollback_config ─────────────────────────────────

    #[test]
    fn rollback_config_continue() {
        let rc = convert_rollback_config(&ProtoUpdateConfig {
            parallelism: 1,
            delay_ns: 1_000_000_000,
            failure_action: "continue".to_string(),
            monitor_ns: 0,
            max_failure_ratio: 0.0,
            order: "start-first".to_string(),
        });
        assert_eq!(rc.failure_action, Some(bollard::models::ServiceSpecRollbackConfigFailureActionEnum::CONTINUE));
        assert_eq!(rc.order, Some(bollard::models::ServiceSpecRollbackConfigOrderEnum::START_FIRST));
    }

    // ── apply_update ────────────────────────────────────────────

    #[test]
    fn update_image() {
        let spec = base_spec();
        let mut req = minimal_update_request("svc-1");
        req.image = Some("nginx:2.0".to_string());
        let updated = apply_update(spec, &req);

        let image = updated.task_template.unwrap().container_spec.unwrap().image.unwrap();
        assert_eq!(image, "nginx:2.0");
    }

    #[test]
    fn update_replicas() {
        let spec = base_spec();
        let mut req = minimal_update_request("svc-1");
        req.replicas = Some(5);
        let updated = apply_update(spec, &req);

        let replicas = updated.mode.unwrap().replicated.unwrap().replicas.unwrap();
        assert_eq!(replicas, 5);
    }

    #[test]
    fn update_env() {
        let spec = base_spec();
        let mut req = minimal_update_request("svc-1");
        req.env.insert("NEW_VAR".to_string(), "new_val".to_string());
        let updated = apply_update(spec, &req);

        let env = updated.task_template.unwrap().container_spec.unwrap().env.unwrap();
        assert!(env.contains(&"NEW_VAR=new_val".to_string()));
    }

    #[test]
    fn update_clear_env() {
        let spec = base_spec();
        let mut req = minimal_update_request("svc-1");
        req.clear_env = true;
        let updated = apply_update(spec, &req);

        let env = updated.task_template.unwrap().container_spec.unwrap().env.unwrap();
        assert!(env.is_empty());
    }

    #[test]
    fn update_labels() {
        let spec = base_spec();
        let mut req = minimal_update_request("svc-1");
        req.labels.insert("version".to_string(), "2".to_string());
        let updated = apply_update(spec, &req);

        let labels = updated.labels.unwrap();
        assert_eq!(labels.get("version").unwrap(), "2");
        // Old label "app" is replaced by new labels map
        assert!(!labels.contains_key("app"));
    }

    #[test]
    fn update_networks() {
        let spec = base_spec();
        let mut req = minimal_update_request("svc-1");
        req.networks = vec!["new-net".to_string()];
        let updated = apply_update(spec, &req);

        let nets = updated.task_template.unwrap().networks.unwrap();
        assert_eq!(nets.len(), 1);
        assert_eq!(nets[0].target, Some("new-net".to_string()));
    }

    #[test]
    fn update_clear_networks() {
        let spec = base_spec();
        let mut req = minimal_update_request("svc-1");
        req.clear_networks = true;
        let updated = apply_update(spec, &req);

        let nets = updated.task_template.unwrap().networks.unwrap();
        assert!(nets.is_empty());
    }

    #[test]
    fn update_ports() {
        let spec = base_spec();
        let mut req = minimal_update_request("svc-1");
        req.ports = vec![ServicePortConfig {
            target_port: 443,
            published_port: 8443,
            protocol: "tcp".to_string(),
            publish_mode: "ingress".to_string(),
        }];
        let updated = apply_update(spec, &req);

        let ports = updated.endpoint_spec.unwrap().ports.unwrap();
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].target_port, Some(443));
    }

    #[test]
    fn update_constraints() {
        let spec = base_spec();
        let mut req = minimal_update_request("svc-1");
        req.constraints = vec!["node.labels.zone == us-east".to_string()];
        let updated = apply_update(spec, &req);

        let placement = updated.task_template.unwrap().placement.unwrap();
        assert_eq!(placement.constraints.unwrap(), vec!["node.labels.zone == us-east"]);
    }

    #[test]
    fn update_command() {
        let spec = base_spec();
        let mut req = minimal_update_request("svc-1");
        req.command = vec!["python".to_string(), "server.py".to_string()];
        let updated = apply_update(spec, &req);

        let cmd = updated.task_template.unwrap().container_spec.unwrap().command.unwrap();
        assert_eq!(cmd, vec!["python", "server.py"]);
    }

    #[test]
    fn update_clear_command() {
        let spec = base_spec();
        let mut req = minimal_update_request("svc-1");
        req.clear_command = true;
        let updated = apply_update(spec, &req);

        let cmd = updated.task_template.unwrap().container_spec.unwrap().command;
        assert!(cmd.is_none());
    }

    #[test]
    fn update_force_increments_version() {
        let spec = base_spec();
        let mut req = minimal_update_request("svc-1");
        req.force = true;
        let updated = apply_update(spec, &req);

        let force = updated.task_template.unwrap().force_update.unwrap();
        assert_eq!(force, 1);
    }

    #[test]
    fn update_force_increments_existing_version() {
        let mut spec = base_spec();
        if let Some(ref mut tt) = spec.task_template {
            tt.force_update = Some(5);
        }
        let mut req = minimal_update_request("svc-1");
        req.force = true;
        let updated = apply_update(spec, &req);

        let force = updated.task_template.unwrap().force_update.unwrap();
        assert_eq!(force, 6);
    }

    #[test]
    fn update_no_changes_preserves_spec() {
        let spec = base_spec();
        let req = minimal_update_request("svc-1");
        let updated = apply_update(spec.clone(), &req);

        // Image unchanged
        let image = updated.task_template.as_ref().unwrap()
            .container_spec.as_ref().unwrap().image.as_ref().unwrap();
        assert_eq!(image, "nginx:1.0");

        // Replicas unchanged
        let replicas = updated.mode.unwrap().replicated.unwrap().replicas.unwrap();
        assert_eq!(replicas, 2);

        // Labels unchanged
        assert_eq!(updated.labels.unwrap().get("app").unwrap(), "web");
    }

    #[test]
    fn update_resource_limits() {
        let spec = base_spec();
        let mut req = minimal_update_request("svc-1");
        req.resource_limits = Some(ServiceResourceLimits {
            nano_cpus: 4_000_000_000,
            memory_bytes: 1_073_741_824,
        });
        let updated = apply_update(spec, &req);

        let resources = updated.task_template.unwrap().resources.unwrap();
        let limits = resources.limits.unwrap();
        assert_eq!(limits.nano_cpus, Some(4_000_000_000));
        assert_eq!(limits.memory_bytes, Some(1_073_741_824));
    }

    #[test]
    fn update_mounts() {
        let spec = base_spec();
        let mut req = minimal_update_request("svc-1");
        req.mounts = vec![ServiceMount {
            r#type: "volume".to_string(),
            source: "new-vol".to_string(),
            target: "/data".to_string(),
            read_only: false,
        }];
        let updated = apply_update(spec, &req);

        let mounts = updated.task_template.unwrap().container_spec.unwrap().mounts.unwrap();
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].source, Some("new-vol".to_string()));
    }

    #[test]
    fn update_restart_policy() {
        let spec = base_spec();
        let mut req = minimal_update_request("svc-1");
        req.restart_policy = Some(ProtoRestartPolicy {
            condition: "none".to_string(),
            delay_ns: 0,
            max_attempts: 0,
            window_ns: 0,
        });
        let updated = apply_update(spec, &req);

        let rp = updated.task_template.unwrap().restart_policy.unwrap();
        assert_eq!(rp.condition, Some(bollard::models::TaskSpecRestartPolicyConditionEnum::NONE));
    }
}
