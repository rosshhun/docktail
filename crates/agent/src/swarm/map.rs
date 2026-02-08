//! Swarm mapping helpers — pure bollard→proto conversions.
//!
//! Every function in this module is a stateless converter.
//! No Docker calls, no side effects, fully unit-testable.

use crate::proto::{
    // Service
    ServiceInfo, ServiceMode, ServicePort, UpdateStatus, VirtualIp,
    UpdateConfig, ServicePlacement, PlacementPreference, Platform,
    SecretReferenceInfo, ConfigReferenceInfo, SwarmRestartPolicy,
    // Network
    SwarmNetworkInfo, IpamConfigEntry, NetworkPeerInfo, NetworkServiceAttachment,
    // Node
    NodeInfo, NodeRole, NodeAvailability, NodeState, ManagerStatus,
    // Task
    TaskInfo,
};

// ── Service ─────────────────────────────────────────────────────

/// Convert a bollard Service to our proto ServiceInfo.
pub(crate) fn convert_service_to_proto(
    s: &bollard::models::Service,
    tasks: &[bollard::models::Task],
) -> ServiceInfo {
    let spec = s.spec.as_ref();
    let task_template = spec.and_then(|sp| sp.task_template.as_ref());

    // Determine service mode
    let mode_spec = spec.and_then(|sp| sp.mode.as_ref());
    let (mode, replicas_desired) = if let Some(mode) = mode_spec {
        if let Some(replicated) = &mode.replicated {
            (ServiceMode::Replicated as i32, replicated.replicas.unwrap_or(1) as u64)
        } else if mode.global.is_some() {
            (ServiceMode::Global as i32, 0u64)
        } else if let Some(replicated_job) = &mode.replicated_job {
            (ServiceMode::ReplicatedJob as i32, replicated_job.max_concurrent.unwrap_or(1) as u64)
        } else if mode.global_job.is_some() {
            (ServiceMode::GlobalJob as i32, 0u64)
        } else {
            (ServiceMode::Unknown as i32, 0u64)
        }
    } else {
        (ServiceMode::Unknown as i32, 0u64)
    };

    // Count running replicas from tasks
    let service_id = s.id.as_deref().unwrap_or("");
    let replicas_running = tasks.iter()
        .filter(|t| {
            t.service_id.as_deref() == Some(service_id) &&
            t.status.as_ref()
                .and_then(|s| s.state.as_ref())
                .map(|s| matches!(s, bollard::models::TaskState::RUNNING))
                .unwrap_or(false)
        })
        .count() as u64;

    // Image
    let image = task_template
        .and_then(|tt| tt.container_spec.as_ref())
        .and_then(|cs| cs.image.clone())
        .unwrap_or_default();

    // Labels
    let labels = spec
        .and_then(|sp| sp.labels.as_ref())
        .cloned()
        .unwrap_or_default();

    // Stack namespace from label
    let stack_namespace = labels.get("com.docker.stack.namespace").cloned();

    // Ports
    let ports: Vec<ServicePort> = s.endpoint.as_ref()
        .and_then(|ep| ep.ports.as_ref())
        .map(|ports| {
            ports.iter().map(|p| ServicePort {
                protocol: p.protocol.as_ref()
                    .map(|pr| format!("{:?}", pr).to_lowercase())
                    .unwrap_or_else(|| "tcp".to_string()),
                target_port: p.target_port.unwrap_or(0) as u32,
                published_port: p.published_port.unwrap_or(0) as u32,
                publish_mode: p.publish_mode.as_ref()
                    .map(|pm| format!("{:?}", pm).to_lowercase())
                    .unwrap_or_else(|| "ingress".to_string()),
            }).collect()
        })
        .unwrap_or_default();

    // Update status
    let update_status = s.update_status.as_ref().map(|us| UpdateStatus {
        state: us.state.as_ref()
            .map(|s| format!("{:?}", s).to_lowercase())
            .unwrap_or_default(),
        started_at: us.started_at.as_ref()
            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
            .map(|dt| dt.timestamp()),
        completed_at: us.completed_at.as_ref()
            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
            .map(|dt| dt.timestamp()),
        message: us.message.clone().unwrap_or_default(),
    });

    // Placement constraints
    let placement_constraints = task_template
        .and_then(|tt| tt.placement.as_ref())
        .and_then(|p| p.constraints.as_ref())
        .cloned()
        .unwrap_or_default();

    // Networks
    let networks: Vec<String> = task_template
        .and_then(|tt| tt.networks.as_ref())
        .map(|nets| {
            nets.iter()
                .filter_map(|n| n.target.clone())
                .collect()
        })
        .unwrap_or_default();

    // Timestamps
    let created_at = s.created_at.as_ref()
        .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
        .map(|dt| dt.timestamp())
        .unwrap_or(0);

    let updated_at = s.updated_at.as_ref()
        .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
        .map(|dt| dt.timestamp())
        .unwrap_or(0);

    // S5: Virtual IPs from endpoint
    let virtual_ips: Vec<VirtualIp> = s.endpoint.as_ref()
        .and_then(|ep| ep.virtual_ips.as_ref())
        .map(|vips| {
            vips.iter().map(|vip| VirtualIp {
                network_id: vip.network_id.clone().unwrap_or_default(),
                addr: vip.addr.clone().unwrap_or_default(),
            }).collect()
        })
        .unwrap_or_default();

    // S6: Update config
    let update_config = spec.and_then(|sp| sp.update_config.as_ref()).map(|uc| UpdateConfig {
        parallelism: uc.parallelism.unwrap_or(1) as u64,
        delay_ns: uc.delay.unwrap_or(0),
        failure_action: uc.failure_action.as_ref()
            .map(|fa| format!("{}", fa))
            .unwrap_or_else(|| "pause".to_string()),
        monitor_ns: uc.monitor.unwrap_or(0),
        max_failure_ratio: uc.max_failure_ratio.unwrap_or(0.0),
        order: uc.order.as_ref()
            .map(|o| format!("{}", o))
            .unwrap_or_else(|| "stop-first".to_string()),
    });

    // S6: Rollback config (same shape as update config)
    let rollback_config = spec.and_then(|sp| sp.rollback_config.as_ref()).map(|rc| UpdateConfig {
        parallelism: rc.parallelism.unwrap_or(1) as u64,
        delay_ns: rc.delay.unwrap_or(0),
        failure_action: rc.failure_action.as_ref()
            .map(|fa| format!("{}", fa))
            .unwrap_or_else(|| "pause".to_string()),
        monitor_ns: rc.monitor.unwrap_or(0),
        max_failure_ratio: rc.max_failure_ratio.unwrap_or(0.0),
        order: rc.order.as_ref()
            .map(|o| format!("{}", o))
            .unwrap_or_else(|| "stop-first".to_string()),
    });

    // S6: Placement (full detail, not just constraints)
    let placement = task_template.and_then(|tt| tt.placement.as_ref()).map(|p| {
        let constraints = p.constraints.clone().unwrap_or_default();
        let preferences = p.preferences.as_ref()
            .map(|prefs| {
                prefs.iter().map(|pref| PlacementPreference {
                    spread_descriptor: pref.spread.as_ref()
                        .and_then(|sp| sp.spread_descriptor.clone())
                        .unwrap_or_default(),
                }).collect()
            })
            .unwrap_or_default();
        let max_replicas_per_node = p.max_replicas.map(|m| m as u64);
        let platforms = p.platforms.as_ref()
            .map(|ps| {
                ps.iter().map(|plat| Platform {
                    architecture: plat.architecture.clone().unwrap_or_default(),
                    os: plat.os.clone().unwrap_or_default(),
                }).collect()
            })
            .unwrap_or_default();

        ServicePlacement {
            constraints,
            preferences,
            max_replicas_per_node,
            platforms,
        }
    });

    // S8: Secret references from container spec
    let secret_references: Vec<SecretReferenceInfo> = task_template
        .and_then(|tt| tt.container_spec.as_ref())
        .and_then(|cs| cs.secrets.as_ref())
        .map(|secrets| {
            secrets.iter().map(|sr| {
                let file = sr.file.as_ref();
                SecretReferenceInfo {
                    secret_id: sr.secret_id.clone().unwrap_or_default(),
                    secret_name: sr.secret_name.clone().unwrap_or_default(),
                    file_name: file.and_then(|f| f.name.clone()).unwrap_or_default(),
                    file_uid: file.and_then(|f| f.uid.clone()).unwrap_or_default(),
                    file_gid: file.and_then(|f| f.gid.clone()).unwrap_or_default(),
                    file_mode: file.and_then(|f| f.mode).unwrap_or(0o444),
                }
            }).collect()
        })
        .unwrap_or_default();

    // S8: Config references from container spec
    let config_references: Vec<ConfigReferenceInfo> = task_template
        .and_then(|tt| tt.container_spec.as_ref())
        .and_then(|cs| cs.configs.as_ref())
        .map(|configs| {
            configs.iter().map(|cr| {
                let file = cr.file.as_ref();
                ConfigReferenceInfo {
                    config_id: cr.config_id.clone().unwrap_or_default(),
                    config_name: cr.config_name.clone().unwrap_or_default(),
                    file_name: file.and_then(|f| f.name.clone()).unwrap_or_default(),
                    file_uid: file.and_then(|f| f.uid.clone()).unwrap_or_default(),
                    file_gid: file.and_then(|f| f.gid.clone()).unwrap_or_default(),
                    file_mode: file.and_then(|f| f.mode).unwrap_or(0o444),
                }
            }).collect()
        })
        .unwrap_or_default();

    // S11: Restart policy from task template
    let restart_policy = task_template
        .and_then(|tt| tt.restart_policy.as_ref())
        .map(|rp| SwarmRestartPolicy {
            condition: rp.condition.as_ref()
                .map(|c| format!("{}", c))
                .unwrap_or_else(|| "any".to_string()),
            delay_ns: rp.delay.unwrap_or(0),
            max_attempts: rp.max_attempts.unwrap_or(0) as u64,
            window_ns: rp.window.unwrap_or(0),
        });

    ServiceInfo {
        id: s.id.clone().unwrap_or_default(),
        name: spec.and_then(|sp| sp.name.clone()).unwrap_or_default(),
        image,
        mode,
        replicas_desired,
        replicas_running,
        labels,
        stack_namespace,
        created_at,
        updated_at,
        ports,
        update_status,
        placement_constraints,
        networks,
        virtual_ips,
        update_config,
        rollback_config,
        placement,
        secret_references,
        config_references,
        restart_policy,
    }
}

// ── Network ─────────────────────────────────────────────────────

/// Convert a bollard Network to our proto SwarmNetworkInfo.
pub(crate) fn convert_network_to_proto(
    net: &bollard::models::Network,
    services: &[bollard::models::Service],
) -> SwarmNetworkInfo {
    let net_id = net.id.as_deref().unwrap_or("");

    // IPAM configs
    let ipam_configs: Vec<IpamConfigEntry> = net.ipam.as_ref()
        .and_then(|ipam| ipam.config.as_ref())
        .map(|configs| {
            configs.iter().map(|c| IpamConfigEntry {
                subnet: c.subnet.clone(),
                gateway: c.gateway.clone(),
                ip_range: c.ip_range.clone(),
            }).collect()
        })
        .unwrap_or_default();

    // Peers
    let peers: Vec<NetworkPeerInfo> = net.peers.as_ref()
        .map(|ps| {
            ps.iter().map(|p| NetworkPeerInfo {
                name: p.name.clone().unwrap_or_default(),
                ip: p.ip.clone().unwrap_or_default(),
            }).collect()
        })
        .unwrap_or_default();

    // Labels
    let labels = net.labels.as_ref().cloned().unwrap_or_default();

    // Options
    let options = net.options.as_ref().cloned().unwrap_or_default();

    // Service attachments: find services whose endpoint VIPs reference this network
    let service_attachments: Vec<NetworkServiceAttachment> = services.iter()
        .filter_map(|svc| {
            let svc_id = svc.id.as_deref().unwrap_or("");
            let svc_name = svc.spec.as_ref()
                .and_then(|sp| sp.name.as_deref())
                .unwrap_or("");

            let vip = svc.endpoint.as_ref()
                .and_then(|ep| ep.virtual_ips.as_ref())
                .and_then(|vips| {
                    vips.iter().find(|v| {
                        v.network_id.as_deref() == Some(net_id)
                    })
                });

            vip.map(|v| NetworkServiceAttachment {
                service_id: svc_id.to_string(),
                service_name: svc_name.to_string(),
                virtual_ip: v.addr.clone().unwrap_or_default(),
            })
        })
        .collect();

    // Timestamps
    let created_at = net.created.as_ref()
        .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
        .map(|dt| dt.timestamp())
        .unwrap_or(0);

    SwarmNetworkInfo {
        id: net_id.to_string(),
        name: net.name.clone().unwrap_or_default(),
        driver: net.driver.clone().unwrap_or_default(),
        scope: net.scope.clone().unwrap_or_default(),
        is_internal: net.internal.unwrap_or(false),
        is_attachable: net.attachable.unwrap_or(false),
        is_ingress: net.ingress.unwrap_or(false),
        enable_ipv6: net.enable_ipv6.unwrap_or(false),
        created_at,
        labels,
        options,
        ipam_configs,
        peers,
        service_attachments,
    }
}

// ── Node ────────────────────────────────────────────────────────

/// Convert a bollard Node to our proto NodeInfo.
pub(crate) fn convert_node_to_proto(n: &bollard::models::Node) -> NodeInfo {
    let spec = n.spec.as_ref();
    let description = n.description.as_ref();
    let status = n.status.as_ref();
    let manager_status = n.manager_status.as_ref();

    let role = spec
        .and_then(|s| s.role.as_ref())
        .map(|r| match r {
            bollard::models::NodeSpecRoleEnum::MANAGER => NodeRole::Manager as i32,
            bollard::models::NodeSpecRoleEnum::WORKER => NodeRole::Worker as i32,
            _ => NodeRole::Unknown as i32,
        })
        .unwrap_or(NodeRole::Unknown as i32);

    let availability = spec
        .and_then(|s| s.availability.as_ref())
        .map(|a| match a {
            bollard::models::NodeSpecAvailabilityEnum::ACTIVE => NodeAvailability::Active as i32,
            bollard::models::NodeSpecAvailabilityEnum::PAUSE => NodeAvailability::Pause as i32,
            bollard::models::NodeSpecAvailabilityEnum::DRAIN => NodeAvailability::Drain as i32,
            _ => NodeAvailability::Unknown as i32,
        })
        .unwrap_or(NodeAvailability::Unknown as i32);

    let node_state = status
        .and_then(|s| s.state.as_ref())
        .map(|s| match s {
            bollard::models::NodeState::READY => NodeState::Ready as i32,
            bollard::models::NodeState::DOWN => NodeState::Down as i32,
            bollard::models::NodeState::DISCONNECTED => NodeState::Disconnected as i32,
            _ => NodeState::Unknown as i32,
        })
        .unwrap_or(NodeState::Unknown as i32);

    let platform = description.and_then(|d| d.platform.as_ref());
    let engine = description.and_then(|d| d.engine.as_ref());
    let resources = description.and_then(|d| d.resources.as_ref());

    let mgr_status = manager_status.map(|ms| ManagerStatus {
        leader: ms.leader.unwrap_or(false),
        reachability: ms.reachability.as_ref()
            .map(|r| format!("{:?}", r).to_lowercase())
            .unwrap_or_default(),
        addr: ms.addr.clone().unwrap_or_default(),
    });

    let labels = spec
        .and_then(|s| s.labels.as_ref())
        .cloned()
        .unwrap_or_default();

    NodeInfo {
        id: n.id.clone().unwrap_or_default(),
        hostname: description
            .and_then(|d| d.hostname.clone())
            .unwrap_or_default(),
        role,
        availability,
        status: node_state,
        addr: status
            .and_then(|s| s.addr.clone())
            .unwrap_or_default(),
        engine_version: engine
            .and_then(|e| e.engine_version.clone())
            .unwrap_or_default(),
        os: platform
            .and_then(|p| p.os.clone())
            .unwrap_or_default(),
        architecture: platform
            .and_then(|p| p.architecture.clone())
            .unwrap_or_default(),
        labels,
        manager_status: mgr_status,
        nano_cpus: resources
            .and_then(|r| r.nano_cpus)
            .unwrap_or(0),
        memory_bytes: resources
            .and_then(|r| r.memory_bytes)
            .unwrap_or(0),
    }
}

// ── Task ────────────────────────────────────────────────────────

/// Convert a bollard Task to our proto TaskInfo (delegates with empty service name).
pub(crate) fn convert_task_to_proto(t: &bollard::models::Task) -> TaskInfo {
    convert_task_to_proto_with_name(t, "")
}

/// Convert a bollard Task to our proto TaskInfo with an optional service name.
pub(crate) fn convert_task_to_proto_with_name(t: &bollard::models::Task, service_name: &str) -> TaskInfo {
    let status = t.status.as_ref();
    let container_status = status.and_then(|s| s.container_status.as_ref());

    TaskInfo {
        id: t.id.clone().unwrap_or_default(),
        service_id: t.service_id.clone().unwrap_or_default(),
        node_id: t.node_id.clone().unwrap_or_default(),
        slot: t.slot.map(|s| s as u64),
        container_id: container_status.and_then(|cs| cs.container_id.clone()),
        state: status
            .and_then(|s| s.state.as_ref())
            .map(|s| format!("{:?}", s).to_lowercase())
            .unwrap_or_else(|| "unknown".to_string()),
        desired_state: t.desired_state
            .as_ref()
            .map(|s| format!("{:?}", s).to_lowercase())
            .unwrap_or_else(|| "unknown".to_string()),
        status_message: status
            .and_then(|s| s.message.clone())
            .unwrap_or_default(),
        status_err: status
            .and_then(|s| s.err.clone()),
        created_at: t.created_at.as_ref()
            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
            .map(|dt| dt.timestamp())
            .unwrap_or(0),
        updated_at: t.updated_at.as_ref()
            .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
            .map(|dt| dt.timestamp())
            .unwrap_or(0),
        exit_code: container_status.and_then(|cs| cs.exit_code.map(|c| c as i32)),
        service_name: if service_name.is_empty() {
            // Fallback: try to find service name from labels
            t.spec.as_ref()
                .and_then(|s| s.container_spec.as_ref())
                .and_then(|cs| cs.labels.as_ref())
                .and_then(|l| l.get("com.docker.swarm.service.name"))
                .cloned()
                .unwrap_or_default()
        } else {
            service_name.to_string()
        },
    }
}
