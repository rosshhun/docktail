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
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("404") || msg.to_lowercase().contains("not found") {
                Status::not_found(format!("Service not found: {}", service_id))
            } else if msg.contains("403") || msg.to_lowercase().contains("permission") {
                Status::permission_denied(format!("Permission denied inspecting service: {}", e))
            } else {
                Status::internal(format!("Failed to inspect service: {}", e))
            }
        })?;
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
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("404") || msg.to_lowercase().contains("not found") {
                Status::not_found(format!("Service not found: {}", e))
            } else if msg.contains("403") || msg.to_lowercase().contains("permission") {
                Status::permission_denied(format!("Permission denied: {}", e))
            } else {
                Status::internal(format!("Failed to inspect service: {}", e))
            }
        })?;

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

    let endpoint_spec = if !req.ports.is_empty() {
        Some(bollard::models::EndpointSpec {
            ports: Some(req.ports.iter().map(|p| {
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
            }).collect()),
            ..Default::default()
        })
    } else {
        None
    };

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

    let mounts = if !req.mounts.is_empty() {
        Some(req.mounts.iter().map(|m| {
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
        }).collect::<Vec<_>>())
    } else {
        None
    };

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

    let restart_policy = req.restart_policy.as_ref().map(|rp| {
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
    });

    let update_config = req.update_config.as_ref().map(|uc| {
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
    });

    let rollback_config = req.rollback_config.as_ref().map(|rc| {
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
    });

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
            Some(bollard::models::EndpointSpec {
                ports: Some(req.ports.iter().map(|p| {
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
                }).collect()),
                ..Default::default()
            })
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
                    Some(req.mounts.iter().map(|m| {
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
                };
            }
        }
    }

    // Restart policy
    if let Some(ref rp) = req.restart_policy {
        if let Some(ref mut tt) = spec.task_template {
            tt.restart_policy = Some(bollard::models::TaskSpecRestartPolicy {
                condition: Some(match rp.condition.as_str() {
                    "none" => bollard::models::TaskSpecRestartPolicyConditionEnum::NONE,
                    "on-failure" => bollard::models::TaskSpecRestartPolicyConditionEnum::ON_FAILURE,
                    _ => bollard::models::TaskSpecRestartPolicyConditionEnum::ANY,
                }),
                delay: if rp.delay_ns > 0 { Some(rp.delay_ns) } else { None },
                max_attempts: if rp.max_attempts > 0 { Some(rp.max_attempts as i64) } else { None },
                window: if rp.window_ns > 0 { Some(rp.window_ns) } else { None },
            });
        }
    }

    // Update config
    if let Some(ref uc) = req.update_config {
        spec.update_config = Some(bollard::models::ServiceSpecUpdateConfig {
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
        });
    }

    // Rollback config
    if let Some(ref rc) = req.rollback_config {
        spec.rollback_config = Some(bollard::models::ServiceSpecRollbackConfig {
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
        });
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
