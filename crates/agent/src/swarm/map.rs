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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    // ── Helpers ──────────────────────────────────────────────────

    fn make_service(
        id: &str,
        name: &str,
        image: &str,
        replicas: Option<i64>,
    ) -> bollard::models::Service {
        bollard::models::Service {
            id: Some(id.to_string()),
            created_at: Some("2025-06-01T12:00:00Z".to_string()),
            updated_at: Some("2025-06-02T12:00:00Z".to_string()),
            spec: Some(bollard::models::ServiceSpec {
                name: Some(name.to_string()),
                mode: Some(bollard::models::ServiceSpecMode {
                    replicated: Some(bollard::models::ServiceSpecModeReplicated {
                        replicas,
                    }),
                    ..Default::default()
                }),
                task_template: Some(bollard::models::TaskSpec {
                    container_spec: Some(bollard::models::TaskSpecContainerSpec {
                        image: Some(image.to_string()),
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                labels: Some({
                    let mut m = HashMap::new();
                    m.insert("com.docker.stack.namespace".to_string(), "mystack".to_string());
                    m
                }),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn make_running_task(task_id: &str, service_id: &str, node_id: &str) -> bollard::models::Task {
        bollard::models::Task {
            id: Some(task_id.to_string()),
            service_id: Some(service_id.to_string()),
            node_id: Some(node_id.to_string()),
            slot: Some(1),
            status: Some(bollard::models::TaskStatus {
                state: Some(bollard::models::TaskState::RUNNING),
                message: Some("running".to_string()),
                ..Default::default()
            }),
            desired_state: Some(bollard::models::TaskState::RUNNING),
            created_at: Some("2025-06-01T12:00:00Z".to_string()),
            updated_at: Some("2025-06-02T12:00:00Z".to_string()),
            ..Default::default()
        }
    }

    fn make_node(
        id: &str,
        hostname: &str,
        role: bollard::models::NodeSpecRoleEnum,
        availability: bollard::models::NodeSpecAvailabilityEnum,
        state: bollard::models::NodeState,
    ) -> bollard::models::Node {
        bollard::models::Node {
            id: Some(id.to_string()),
            spec: Some(bollard::models::NodeSpec {
                role: Some(role),
                availability: Some(availability),
                labels: Some({
                    let mut m = HashMap::new();
                    m.insert("env".to_string(), "production".to_string());
                    m
                }),
                ..Default::default()
            }),
            description: Some(bollard::models::NodeDescription {
                hostname: Some(hostname.to_string()),
                platform: Some(bollard::models::Platform {
                    architecture: Some("x86_64".to_string()),
                    os: Some("linux".to_string()),
                }),
                engine: Some(bollard::models::EngineDescription {
                    engine_version: Some("24.0.7".to_string()),
                    ..Default::default()
                }),
                resources: Some(bollard::models::ResourceObject {
                    nano_cpus: Some(4_000_000_000),
                    memory_bytes: Some(8_589_934_592),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            status: Some(bollard::models::NodeStatus {
                state: Some(state),
                addr: Some("192.168.1.10".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    // ── convert_service_to_proto ─────────────────────────────────

    #[test]
    fn service_basic_replicated() {
        let svc = make_service("svc-1", "web", "nginx:latest", Some(3));
        let tasks = vec![
            make_running_task("t1", "svc-1", "node-a"),
            make_running_task("t2", "svc-1", "node-b"),
        ];

        let info = convert_service_to_proto(&svc, &tasks);

        assert_eq!(info.id, "svc-1");
        assert_eq!(info.name, "web");
        assert_eq!(info.image, "nginx:latest");
        assert_eq!(info.mode, ServiceMode::Replicated as i32);
        assert_eq!(info.replicas_desired, 3);
        assert_eq!(info.replicas_running, 2);
        assert_eq!(info.stack_namespace, Some("mystack".to_string()));
    }

    #[test]
    fn service_global_mode() {
        let mut svc = make_service("svc-g", "agent", "monitor:1.0", None);
        if let Some(ref mut spec) = svc.spec {
            spec.mode = Some(bollard::models::ServiceSpecMode {
                global: Some(Default::default()),
                ..Default::default()
            });
        }

        let info = convert_service_to_proto(&svc, &[]);
        assert_eq!(info.mode, ServiceMode::Global as i32);
        assert_eq!(info.replicas_desired, 0);
    }

    #[test]
    fn service_with_ports() {
        let mut svc = make_service("svc-p", "web", "nginx", Some(1));
        svc.endpoint = Some(bollard::models::ServiceEndpoint {
            ports: Some(vec![
                bollard::models::EndpointPortConfig {
                    protocol: Some(bollard::models::EndpointPortConfigProtocolEnum::TCP),
                    target_port: Some(80),
                    published_port: Some(8080),
                    publish_mode: Some(bollard::models::EndpointPortConfigPublishModeEnum::INGRESS),
                    ..Default::default()
                },
            ]),
            virtual_ips: Some(vec![
                bollard::models::ServiceEndpointVirtualIps {
                    network_id: Some("net-1".to_string()),
                    addr: Some("10.0.0.5/24".to_string()),
                },
            ]),
            ..Default::default()
        });

        let info = convert_service_to_proto(&svc, &[]);
        assert_eq!(info.ports.len(), 1);
        assert_eq!(info.ports[0].target_port, 80);
        assert_eq!(info.ports[0].published_port, 8080);
        assert_eq!(info.ports[0].protocol, "tcp");
        assert_eq!(info.virtual_ips.len(), 1);
        assert_eq!(info.virtual_ips[0].network_id, "net-1");
        assert_eq!(info.virtual_ips[0].addr, "10.0.0.5/24");
    }

    #[test]
    fn service_with_update_status() {
        let mut svc = make_service("svc-u", "api", "app:2", Some(2));
        svc.update_status = Some(bollard::models::ServiceUpdateStatus {
            state: Some(bollard::models::ServiceUpdateStatusStateEnum::UPDATING),
            started_at: Some("2025-06-02T10:00:00Z".to_string()),
            completed_at: None,
            message: Some("updating 1/2".to_string()),
        });

        let info = convert_service_to_proto(&svc, &[]);
        let us = info.update_status.unwrap();
        assert_eq!(us.state, "updating");
        assert!(us.started_at.is_some());
        assert!(us.completed_at.is_none());
        assert_eq!(us.message, "updating 1/2");
    }

    #[test]
    fn service_with_placement_constraints() {
        let mut svc = make_service("svc-pc", "db", "postgres", Some(1));
        if let Some(ref mut spec) = svc.spec {
            if let Some(ref mut tt) = spec.task_template {
                tt.placement = Some(bollard::models::TaskSpecPlacement {
                    constraints: Some(vec!["node.role == manager".to_string()]),
                    preferences: Some(vec![
                        bollard::models::TaskSpecPlacementPreferences {
                            spread: Some(bollard::models::TaskSpecPlacementSpread {
                                spread_descriptor: Some("node.labels.zone".to_string()),
                            }),
                        },
                    ]),
                    max_replicas: Some(2),
                    platforms: Some(vec![
                        bollard::models::Platform {
                            architecture: Some("amd64".to_string()),
                            os: Some("linux".to_string()),
                        },
                    ]),
                    ..Default::default()
                });
            }
        }

        let info = convert_service_to_proto(&svc, &[]);
        assert_eq!(info.placement_constraints, vec!["node.role == manager"]);
        let placement = info.placement.unwrap();
        assert_eq!(placement.constraints, vec!["node.role == manager"]);
        assert_eq!(placement.preferences.len(), 1);
        assert_eq!(placement.preferences[0].spread_descriptor, "node.labels.zone");
        assert_eq!(placement.max_replicas_per_node, Some(2));
        assert_eq!(placement.platforms.len(), 1);
        assert_eq!(placement.platforms[0].os, "linux");
    }

    #[test]
    fn service_with_update_and_rollback_config() {
        let mut svc = make_service("svc-uc", "web", "nginx", Some(3));
        if let Some(ref mut spec) = svc.spec {
            spec.update_config = Some(bollard::models::ServiceSpecUpdateConfig {
                parallelism: Some(2),
                delay: Some(5_000_000_000),
                failure_action: Some(bollard::models::ServiceSpecUpdateConfigFailureActionEnum::ROLLBACK),
                monitor: Some(10_000_000_000),
                max_failure_ratio: Some(0.1),
                order: Some(bollard::models::ServiceSpecUpdateConfigOrderEnum::START_FIRST),
            });
            spec.rollback_config = Some(bollard::models::ServiceSpecRollbackConfig {
                parallelism: Some(1),
                delay: Some(1_000_000_000),
                failure_action: Some(bollard::models::ServiceSpecRollbackConfigFailureActionEnum::PAUSE),
                monitor: Some(5_000_000_000),
                max_failure_ratio: Some(0.2),
                order: Some(bollard::models::ServiceSpecRollbackConfigOrderEnum::STOP_FIRST),
            });
        }

        let info = convert_service_to_proto(&svc, &[]);
        let uc = info.update_config.unwrap();
        assert_eq!(uc.parallelism, 2);
        assert_eq!(uc.delay_ns, 5_000_000_000);
        assert!(uc.failure_action.contains("rollback"));
        assert!(uc.order.contains("start-first"));

        let rc = info.rollback_config.unwrap();
        assert_eq!(rc.parallelism, 1);
        assert_eq!(rc.delay_ns, 1_000_000_000);
    }

    #[test]
    fn service_with_secret_and_config_refs() {
        let mut svc = make_service("svc-sc", "app", "myapp:1", Some(1));
        if let Some(ref mut spec) = svc.spec {
            if let Some(ref mut tt) = spec.task_template {
                if let Some(ref mut cs) = tt.container_spec {
                    cs.secrets = Some(vec![
                        bollard::models::TaskSpecContainerSpecSecrets {
                            secret_id: Some("secret-abc".to_string()),
                            secret_name: Some("db_password".to_string()),
                            file: Some(bollard::models::TaskSpecContainerSpecFile {
                                name: Some("db_pass".to_string()),
                                uid: Some("0".to_string()),
                                gid: Some("0".to_string()),
                                mode: Some(0o400),
                            }),
                        },
                    ]);
                    cs.configs = Some(vec![
                        bollard::models::TaskSpecContainerSpecConfigs {
                            config_id: Some("config-xyz".to_string()),
                            config_name: Some("app_config".to_string()),
                            file: Some(bollard::models::TaskSpecContainerSpecFile1 {
                                name: Some("config.json".to_string()),
                                uid: Some("0".to_string()),
                                gid: Some("0".to_string()),
                                mode: Some(0o444),
                            }),
                            ..Default::default()
                        },
                    ]);
                }
            }
        }

        let info = convert_service_to_proto(&svc, &[]);
        assert_eq!(info.secret_references.len(), 1);
        assert_eq!(info.secret_references[0].secret_id, "secret-abc");
        assert_eq!(info.secret_references[0].secret_name, "db_password");
        assert_eq!(info.secret_references[0].file_name, "db_pass");
        assert_eq!(info.secret_references[0].file_mode, 0o400);
        assert_eq!(info.config_references.len(), 1);
        assert_eq!(info.config_references[0].config_id, "config-xyz");
        assert_eq!(info.config_references[0].config_name, "app_config");
    }

    #[test]
    fn service_with_restart_policy() {
        let mut svc = make_service("svc-rp", "worker", "app:1", Some(1));
        if let Some(ref mut spec) = svc.spec {
            if let Some(ref mut tt) = spec.task_template {
                tt.restart_policy = Some(bollard::models::TaskSpecRestartPolicy {
                    condition: Some(bollard::models::TaskSpecRestartPolicyConditionEnum::ON_FAILURE),
                    delay: Some(5_000_000_000),
                    max_attempts: Some(3),
                    window: Some(60_000_000_000),
                });
            }
        }

        let info = convert_service_to_proto(&svc, &[]);
        let rp = info.restart_policy.unwrap();
        assert!(rp.condition.contains("failure"));
        assert_eq!(rp.delay_ns, 5_000_000_000);
        assert_eq!(rp.max_attempts, 3);
        assert_eq!(rp.window_ns, 60_000_000_000);
    }

    #[test]
    fn service_empty_defaults() {
        let svc = bollard::models::Service::default();
        let info = convert_service_to_proto(&svc, &[]);

        assert_eq!(info.id, "");
        assert_eq!(info.name, "");
        assert_eq!(info.image, "");
        assert_eq!(info.mode, ServiceMode::Unknown as i32);
        assert_eq!(info.replicas_desired, 0);
        assert_eq!(info.replicas_running, 0);
        assert!(info.ports.is_empty());
        assert!(info.virtual_ips.is_empty());
        assert!(info.update_status.is_none());
        assert!(info.placement.is_none());
    }

    #[test]
    fn service_timestamps_parsed() {
        let svc = make_service("svc-ts", "web", "nginx", Some(1));
        let info = convert_service_to_proto(&svc, &[]);

        // "2025-06-01T12:00:00Z" → 1748779200
        assert_eq!(info.created_at, 1748779200);
        // "2025-06-02T12:00:00Z" → 1748865600
        assert_eq!(info.updated_at, 1748865600);
    }

    // ── convert_network_to_proto ─────────────────────────────────

    #[test]
    fn network_basic() {
        let net = bollard::models::Network {
            id: Some("net-1".to_string()),
            name: Some("my-overlay".to_string()),
            driver: Some("overlay".to_string()),
            scope: Some("swarm".to_string()),
            internal: Some(false),
            attachable: Some(true),
            ingress: Some(false),
            enable_ipv6: Some(false),
            created: Some("2025-06-01T12:00:00Z".to_string()),
            labels: Some({
                let mut m = HashMap::new();
                m.insert("com.docker.stack.namespace".to_string(), "mystack".to_string());
                m
            }),
            options: Some({
                let mut m = HashMap::new();
                m.insert("com.docker.network.driver.overlay.vxlanid_list".to_string(), "4097".to_string());
                m
            }),
            ..Default::default()
        };

        let info = convert_network_to_proto(&net, &[]);
        assert_eq!(info.id, "net-1");
        assert_eq!(info.name, "my-overlay");
        assert_eq!(info.driver, "overlay");
        assert_eq!(info.scope, "swarm");
        assert!(!info.is_internal);
        assert!(info.is_attachable);
        assert!(!info.is_ingress);
        assert!(!info.enable_ipv6);
        assert_eq!(info.created_at, 1748779200);
        assert_eq!(info.labels.get("com.docker.stack.namespace").unwrap(), "mystack");
    }

    #[test]
    fn network_with_ipam() {
        let net = bollard::models::Network {
            id: Some("net-2".to_string()),
            name: Some("backend".to_string()),
            ipam: Some(bollard::models::Ipam {
                config: Some(vec![
                    bollard::models::IpamConfig {
                        subnet: Some("10.0.1.0/24".to_string()),
                        gateway: Some("10.0.1.1".to_string()),
                        ip_range: Some("10.0.1.0/25".to_string()),
                        ..Default::default()
                    },
                ]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let info = convert_network_to_proto(&net, &[]);
        assert_eq!(info.ipam_configs.len(), 1);
        assert_eq!(info.ipam_configs[0].subnet, Some("10.0.1.0/24".to_string()));
        assert_eq!(info.ipam_configs[0].gateway, Some("10.0.1.1".to_string()));
        assert_eq!(info.ipam_configs[0].ip_range, Some("10.0.1.0/25".to_string()));
    }

    #[test]
    fn network_with_peers() {
        let net = bollard::models::Network {
            id: Some("net-3".to_string()),
            name: Some("ingress".to_string()),
            peers: Some(vec![
                bollard::models::PeerInfo {
                    name: Some("node-a".to_string()),
                    ip: Some("192.168.1.10".to_string()),
                },
                bollard::models::PeerInfo {
                    name: Some("node-b".to_string()),
                    ip: Some("192.168.1.11".to_string()),
                },
            ]),
            ..Default::default()
        };

        let info = convert_network_to_proto(&net, &[]);
        assert_eq!(info.peers.len(), 2);
        assert_eq!(info.peers[0].name, "node-a");
        assert_eq!(info.peers[1].ip, "192.168.1.11");
    }

    #[test]
    fn network_with_service_attachments() {
        let net = bollard::models::Network {
            id: Some("net-4".to_string()),
            name: Some("frontend".to_string()),
            ..Default::default()
        };

        let services = vec![
            bollard::models::Service {
                id: Some("svc-1".to_string()),
                spec: Some(bollard::models::ServiceSpec {
                    name: Some("web".to_string()),
                    ..Default::default()
                }),
                endpoint: Some(bollard::models::ServiceEndpoint {
                    virtual_ips: Some(vec![
                        bollard::models::ServiceEndpointVirtualIps {
                            network_id: Some("net-4".to_string()),
                            addr: Some("10.0.0.5/24".to_string()),
                        },
                    ]),
                    ..Default::default()
                }),
                ..Default::default()
            },
            bollard::models::Service {
                id: Some("svc-2".to_string()),
                spec: Some(bollard::models::ServiceSpec {
                    name: Some("api".to_string()),
                    ..Default::default()
                }),
                endpoint: Some(bollard::models::ServiceEndpoint {
                    virtual_ips: Some(vec![
                        bollard::models::ServiceEndpointVirtualIps {
                            network_id: Some("net-other".to_string()),
                            addr: Some("10.0.1.5/24".to_string()),
                        },
                    ]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];

        let info = convert_network_to_proto(&net, &services);
        assert_eq!(info.service_attachments.len(), 1);
        assert_eq!(info.service_attachments[0].service_id, "svc-1");
        assert_eq!(info.service_attachments[0].service_name, "web");
        assert_eq!(info.service_attachments[0].virtual_ip, "10.0.0.5/24");
    }

    #[test]
    fn network_empty_defaults() {
        let net = bollard::models::Network::default();
        let info = convert_network_to_proto(&net, &[]);

        assert_eq!(info.id, "");
        assert_eq!(info.name, "");
        assert_eq!(info.driver, "");
        assert!(!info.is_internal);
        assert!(info.ipam_configs.is_empty());
        assert!(info.peers.is_empty());
        assert!(info.service_attachments.is_empty());
    }

    // ── convert_node_to_proto ────────────────────────────────────

    #[test]
    fn node_manager_ready() {
        let node = make_node(
            "node-1", "manager-host",
            bollard::models::NodeSpecRoleEnum::MANAGER,
            bollard::models::NodeSpecAvailabilityEnum::ACTIVE,
            bollard::models::NodeState::READY,
        );

        let info = convert_node_to_proto(&node);
        assert_eq!(info.id, "node-1");
        assert_eq!(info.hostname, "manager-host");
        assert_eq!(info.role, NodeRole::Manager as i32);
        assert_eq!(info.availability, NodeAvailability::Active as i32);
        assert_eq!(info.status, NodeState::Ready as i32);
        assert_eq!(info.addr, "192.168.1.10");
        assert_eq!(info.engine_version, "24.0.7");
        assert_eq!(info.os, "linux");
        assert_eq!(info.architecture, "x86_64");
        assert_eq!(info.nano_cpus, 4_000_000_000);
        assert_eq!(info.memory_bytes, 8_589_934_592);
        assert_eq!(info.labels.get("env").unwrap(), "production");
    }

    #[test]
    fn node_worker_paused() {
        let node = make_node(
            "node-2", "worker-host",
            bollard::models::NodeSpecRoleEnum::WORKER,
            bollard::models::NodeSpecAvailabilityEnum::PAUSE,
            bollard::models::NodeState::READY,
        );

        let info = convert_node_to_proto(&node);
        assert_eq!(info.role, NodeRole::Worker as i32);
        assert_eq!(info.availability, NodeAvailability::Pause as i32);
    }

    #[test]
    fn node_drain_down() {
        let node = make_node(
            "node-3", "drain-host",
            bollard::models::NodeSpecRoleEnum::WORKER,
            bollard::models::NodeSpecAvailabilityEnum::DRAIN,
            bollard::models::NodeState::DOWN,
        );

        let info = convert_node_to_proto(&node);
        assert_eq!(info.availability, NodeAvailability::Drain as i32);
        assert_eq!(info.status, NodeState::Down as i32);
    }

    #[test]
    fn node_disconnected() {
        let node = make_node(
            "node-4", "disc-host",
            bollard::models::NodeSpecRoleEnum::WORKER,
            bollard::models::NodeSpecAvailabilityEnum::ACTIVE,
            bollard::models::NodeState::DISCONNECTED,
        );

        let info = convert_node_to_proto(&node);
        assert_eq!(info.status, NodeState::Disconnected as i32);
    }

    #[test]
    fn node_with_manager_status() {
        let mut node = make_node(
            "node-m", "leader-host",
            bollard::models::NodeSpecRoleEnum::MANAGER,
            bollard::models::NodeSpecAvailabilityEnum::ACTIVE,
            bollard::models::NodeState::READY,
        );
        node.manager_status = Some(bollard::models::ManagerStatus {
            leader: Some(true),
            reachability: Some(bollard::models::Reachability::REACHABLE),
            addr: Some("192.168.1.10:2377".to_string()),
        });

        let info = convert_node_to_proto(&node);
        let ms = info.manager_status.unwrap();
        assert!(ms.leader);
        assert_eq!(ms.addr, "192.168.1.10:2377");
        assert!(ms.reachability.contains("reachable"));
    }

    #[test]
    fn node_empty_defaults() {
        let node = bollard::models::Node::default();
        let info = convert_node_to_proto(&node);

        assert_eq!(info.id, "");
        assert_eq!(info.hostname, "");
        assert_eq!(info.role, NodeRole::Unknown as i32);
        assert_eq!(info.availability, NodeAvailability::Unknown as i32);
        assert_eq!(info.status, NodeState::Unknown as i32);
        assert_eq!(info.nano_cpus, 0);
        assert_eq!(info.memory_bytes, 0);
        assert!(info.manager_status.is_none());
    }

    // ── convert_task_to_proto ────────────────────────────────────

    #[test]
    fn task_running() {
        let task = make_running_task("task-1", "svc-1", "node-a");
        let info = convert_task_to_proto(&task);

        assert_eq!(info.id, "task-1");
        assert_eq!(info.service_id, "svc-1");
        assert_eq!(info.node_id, "node-a");
        assert_eq!(info.slot, Some(1));
        assert_eq!(info.state, "running");
        assert_eq!(info.desired_state, "running");
        assert_eq!(info.status_message, "running");
    }

    #[test]
    fn task_failed_with_error() {
        let task = bollard::models::Task {
            id: Some("task-f".to_string()),
            service_id: Some("svc-1".to_string()),
            node_id: Some("node-a".to_string()),
            slot: Some(2),
            status: Some(bollard::models::TaskStatus {
                state: Some(bollard::models::TaskState::FAILED),
                message: Some("task exited".to_string()),
                err: Some("exit code 137".to_string()),
                container_status: Some(bollard::models::ContainerStatus {
                    container_id: Some("container-abc".to_string()),
                    exit_code: Some(137),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            desired_state: Some(bollard::models::TaskState::SHUTDOWN),
            created_at: Some("2025-06-01T12:00:00Z".to_string()),
            updated_at: Some("2025-06-01T12:05:00Z".to_string()),
            ..Default::default()
        };

        let info = convert_task_to_proto(&task);
        assert_eq!(info.id, "task-f");
        assert_eq!(info.state, "failed");
        assert_eq!(info.desired_state, "shutdown");
        assert_eq!(info.status_message, "task exited");
        assert_eq!(info.status_err, Some("exit code 137".to_string()));
        assert_eq!(info.container_id, Some("container-abc".to_string()));
        assert_eq!(info.exit_code, Some(137));
    }

    #[test]
    fn task_with_name_override() {
        let task = make_running_task("task-n", "svc-1", "node-a");
        let info = convert_task_to_proto_with_name(&task, "my-service");
        assert_eq!(info.service_name, "my-service");
    }

    #[test]
    fn task_name_from_labels_when_empty() {
        let mut task = make_running_task("task-l", "svc-1", "node-a");
        task.spec = Some(bollard::models::TaskSpec {
            container_spec: Some(bollard::models::TaskSpecContainerSpec {
                labels: Some({
                    let mut m = HashMap::new();
                    m.insert("com.docker.swarm.service.name".to_string(), "labeled-svc".to_string());
                    m
                }),
                ..Default::default()
            }),
            ..Default::default()
        });

        let info = convert_task_to_proto_with_name(&task, "");
        assert_eq!(info.service_name, "labeled-svc");
    }

    #[test]
    fn task_empty_defaults() {
        let task = bollard::models::Task::default();
        let info = convert_task_to_proto(&task);

        assert_eq!(info.id, "");
        assert_eq!(info.service_id, "");
        assert_eq!(info.node_id, "");
        assert_eq!(info.slot, None);
        assert_eq!(info.state, "unknown");
        assert_eq!(info.desired_state, "unknown");
        assert_eq!(info.container_id, None);
        assert_eq!(info.exit_code, None);
    }

    #[test]
    fn task_timestamps_parsed() {
        let task = make_running_task("task-ts", "svc-1", "node-a");
        let info = convert_task_to_proto(&task);

        assert_eq!(info.created_at, 1748779200);
        assert_eq!(info.updated_at, 1748865600);
    }

    // ── Service running task counting ────────────────────────────

    #[test]
    fn service_counts_only_running_tasks_for_this_service() {
        let svc = make_service("svc-A", "web", "nginx", Some(3));
        let tasks = vec![
            make_running_task("t1", "svc-A", "n1"),
            make_running_task("t2", "svc-A", "n2"),
            make_running_task("t3", "svc-B", "n3"), // different service
            bollard::models::Task {
                id: Some("t4".to_string()),
                service_id: Some("svc-A".to_string()),
                status: Some(bollard::models::TaskStatus {
                    state: Some(bollard::models::TaskState::FAILED),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];

        let info = convert_service_to_proto(&svc, &tasks);
        assert_eq!(info.replicas_running, 2); // only t1 and t2, not t3 (wrong svc) or t4 (failed)
    }

    // ── Service networks from task template ──────────────────────

    #[test]
    fn service_networks_from_task_template() {
        let mut svc = make_service("svc-net", "web", "nginx", Some(1));
        if let Some(ref mut spec) = svc.spec {
            if let Some(ref mut tt) = spec.task_template {
                tt.networks = Some(vec![
                    bollard::models::NetworkAttachmentConfig {
                        target: Some("net-1".to_string()),
                        ..Default::default()
                    },
                    bollard::models::NetworkAttachmentConfig {
                        target: Some("net-2".to_string()),
                        ..Default::default()
                    },
                ]);
            }
        }

        let info = convert_service_to_proto(&svc, &[]);
        assert_eq!(info.networks, vec!["net-1", "net-2"]);
    }
}
