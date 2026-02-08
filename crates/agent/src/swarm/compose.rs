//! Compose — compose file parsing and stack deployment logic.
//!
//! Extracted from `service/swarm.rs` `deploy_compose_stack`.
//! Pure domain logic: parses a compose YAML, creates networks/volumes/services.

use std::collections::HashMap;
use tracing::{info, warn};

use crate::docker::client::DockerClient;
use crate::proto::DeployComposeStackResponse;

/// Result of a compose stack deployment.
pub(crate) struct DeployResult {
    pub service_ids: Vec<String>,
    pub network_names: Vec<String>,
    pub volume_names: Vec<String>,
    pub failed: Vec<String>,
}

/// Deploy a compose stack by parsing the YAML and creating Docker resources.
///
/// Returns a [`DeployResult`] with the created resource IDs/names and any failures.
pub(crate) async fn deploy(
    docker: &DockerClient,
    stack_name: &str,
    compose_yaml: &str,
) -> DeployResult {
    // Parse the compose YAML
    let compose: serde_yaml::Value = match serde_yaml::from_str(compose_yaml) {
        Ok(v) => v,
        Err(e) => {
            return DeployResult {
                service_ids: Vec::new(),
                network_names: Vec::new(),
                volume_names: Vec::new(),
                failed: vec![format!("Failed to parse compose YAML: {}", e)],
            };
        }
    };

    let mut service_ids = Vec::new();
    let mut network_names = Vec::new();
    let mut volume_names = Vec::new();
    let mut failed = Vec::new();

    // Create the implicit _default overlay network
    create_default_network(docker, stack_name, &mut network_names, &mut failed).await;

    // Track external resource aliases
    let mut external_networks: HashMap<String, String> = HashMap::new();
    let mut external_volumes: HashMap<String, String> = HashMap::new();

    // Create networks
    create_networks(docker, stack_name, &compose, &mut network_names, &mut failed, &mut external_networks).await;

    // Create volumes
    create_volumes(docker, stack_name, &compose, &mut volume_names, &mut failed, &mut external_volumes).await;

    // Create services
    create_services(docker, stack_name, &compose, &external_networks, &external_volumes, &mut service_ids, &mut failed).await;

    DeployResult { service_ids, network_names, volume_names, failed }
}

/// Build the proto response from a [`DeployResult`].
pub(crate) fn into_response(stack_name: &str, result: DeployResult) -> DeployComposeStackResponse {
    let all_ok = result.failed.is_empty();
    DeployComposeStackResponse {
        success: all_ok,
        message: if all_ok {
            format!("Stack '{}' deployed: {} services, {} networks, {} volumes",
                stack_name, result.service_ids.len(), result.network_names.len(), result.volume_names.len())
        } else {
            format!("Stack '{}' partially deployed: {} failed", stack_name, result.failed.len())
        },
        service_ids: result.service_ids,
        network_names: result.network_names,
        volume_names: result.volume_names,
        failed_services: result.failed,
    }
}

// ── Internal helpers ────────────────────────────────────────────

async fn create_default_network(
    docker: &DockerClient,
    stack_name: &str,
    network_names: &mut Vec<String>,
    failed: &mut Vec<String>,
) {
    let default_net = format!("{}_default", stack_name);
    let mut labels = HashMap::new();
    labels.insert("com.docker.stack.namespace".to_string(), stack_name.to_string());
    match docker.create_network(
        &default_net, Some("overlay"), labels,
        false, false, false,
        HashMap::new(), None,
    ).await {
        Ok(_) => network_names.push(default_net),
        Err(e) => {
            let err_str = format!("{}", e);
            if err_str.contains("409") || err_str.contains("already exists") {
                info!(network = %default_net, "Default network already exists, reusing");
                network_names.push(default_net);
            } else {
                warn!(network = %default_net, "Failed to create default network: {}", e);
                failed.push(format!("network/{}: {}", default_net, e));
            }
        }
    }
}

async fn create_networks(
    docker: &DockerClient,
    stack_name: &str,
    compose: &serde_yaml::Value,
    network_names: &mut Vec<String>,
    failed: &mut Vec<String>,
    external_networks: &mut HashMap<String, String>,
) {
    let Some(networks) = compose.get("networks").and_then(|n| n.as_mapping()) else { return };

    for (name, config) in networks {
        let raw_name = name.as_str().unwrap_or("default");

        let is_external = config.get("external")
            .map(|v| v.as_bool().unwrap_or(false) || v.is_mapping())
            .unwrap_or(false);

        if is_external {
            let ext_name = config.get("external")
                .and_then(|v| v.as_mapping())
                .and_then(|m| m.get(serde_yaml::Value::String("name".into())))
                .and_then(|v| v.as_str())
                .or_else(|| config.get("name").and_then(|v| v.as_str()))
                .unwrap_or(raw_name);
            external_networks.insert(raw_name.to_string(), ext_name.to_string());
            info!(network = %ext_name, alias = %raw_name, "External network — not creating, using as-is");
            continue;
        }

        let net_name = format!("{}_{}", stack_name, raw_name);
        let driver = config.get("driver").and_then(|d| d.as_str()).unwrap_or("overlay");
        let mut labels = HashMap::new();
        labels.insert("com.docker.stack.namespace".to_string(), stack_name.to_string());

        match docker.create_network(
            &net_name, Some(driver), labels,
            false, false, false,
            HashMap::new(), None,
        ).await {
            Ok(_) => network_names.push(net_name),
            Err(e) => {
                let err_str = format!("{}", e);
                if err_str.contains("409") || err_str.contains("already exists") {
                    info!(network = %net_name, "Network already exists, reusing");
                    network_names.push(net_name);
                } else {
                    warn!(network = %net_name, "Failed to create network: {}", e);
                    failed.push(format!("network/{}: {}", net_name, e));
                }
            }
        }
    }
}

async fn create_volumes(
    docker: &DockerClient,
    stack_name: &str,
    compose: &serde_yaml::Value,
    volume_names: &mut Vec<String>,
    failed: &mut Vec<String>,
    external_volumes: &mut HashMap<String, String>,
) {
    let Some(volumes) = compose.get("volumes").and_then(|v| v.as_mapping()) else { return };

    for (name, config) in volumes {
        let raw_name = name.as_str().unwrap_or("default");

        let is_external = config.get("external")
            .map(|v| v.as_bool().unwrap_or(false) || v.is_mapping())
            .unwrap_or(false);

        if is_external {
            let ext_name = config.get("external")
                .and_then(|v| v.as_mapping())
                .and_then(|m| m.get(serde_yaml::Value::String("name".into())))
                .and_then(|v| v.as_str())
                .or_else(|| config.get("name").and_then(|v| v.as_str()))
                .unwrap_or(raw_name);
            external_volumes.insert(raw_name.to_string(), ext_name.to_string());
            info!(volume = %ext_name, alias = %raw_name, "External volume — not creating, using as-is");
            continue;
        }

        let vol_name = format!("{}_{}", stack_name, raw_name);
        let driver = config.get("driver").and_then(|d| d.as_str());
        let mut labels = HashMap::new();
        labels.insert("com.docker.stack.namespace".to_string(), stack_name.to_string());

        match docker.create_volume(&vol_name, driver, labels, HashMap::new()).await {
            Ok(_) => volume_names.push(vol_name),
            Err(e) => {
                let err_str = format!("{}", e);
                if err_str.contains("409") || err_str.contains("already exists") {
                    info!(volume = %vol_name, "Volume already exists, reusing");
                    volume_names.push(vol_name);
                } else {
                    warn!(volume = %vol_name, "Failed to create volume: {}", e);
                    failed.push(format!("volume/{}: {}", vol_name, e));
                }
            }
        }
    }
}

async fn create_services(
    docker: &DockerClient,
    stack_name: &str,
    compose: &serde_yaml::Value,
    external_networks: &HashMap<String, String>,
    external_volumes: &HashMap<String, String>,
    service_ids: &mut Vec<String>,
    failed: &mut Vec<String>,
) {
    let Some(services) = compose.get("services").and_then(|s| s.as_mapping()) else { return };

    for (name, config) in services {
        let svc_name = format!("{}_{}", stack_name, name.as_str().unwrap_or("unnamed"));
        let image = config.get("image").and_then(|i| i.as_str()).unwrap_or("").to_string();
        if image.is_empty() {
            failed.push(format!("{}: no image specified", svc_name));
            continue;
        }

        let replicas = config.get("deploy")
            .and_then(|d| d.get("replicas"))
            .and_then(|r| r.as_u64())
            .unwrap_or(1);

        let env_vec = parse_environment(config);
        let port_configs = parse_ports(config);
        let networks = parse_networks(config, stack_name, external_networks);
        let command = parse_command(config);
        let mounts = parse_volumes(config, stack_name, external_volumes);

        let mut labels = HashMap::new();
        labels.insert("com.docker.stack.namespace".to_string(), stack_name.to_string());
        labels.insert("com.docker.stack.image".to_string(), image.clone());

        let spec = bollard::models::ServiceSpec {
            name: Some(svc_name.clone()),
            mode: Some(bollard::models::ServiceSpecMode {
                replicated: Some(bollard::models::ServiceSpecModeReplicated {
                    replicas: Some(replicas as i64),
                }),
                ..Default::default()
            }),
            task_template: Some(bollard::models::TaskSpec {
                container_spec: Some(bollard::models::TaskSpecContainerSpec {
                    image: Some(image),
                    env: if env_vec.is_empty() { None } else { Some(env_vec) },
                    command,
                    mounts: if mounts.is_empty() { None } else { Some(mounts) },
                    ..Default::default()
                }),
                networks,
                ..Default::default()
            }),
            labels: Some(labels),
            endpoint_spec: if port_configs.is_empty() {
                None
            } else {
                Some(bollard::models::EndpointSpec {
                    ports: Some(port_configs),
                    ..Default::default()
                })
            },
            ..Default::default()
        };

        match docker.create_service(spec, None).await {
            Ok(id) => {
                info!(service = %svc_name, id = %id, "Compose service created");
                service_ids.push(id);
            }
            Err(e) => {
                warn!(service = %svc_name, "Failed to create compose service: {}", e);
                failed.push(format!("{}: {}", svc_name, e));
            }
        }
    }
}

// ── Parsing helpers ─────────────────────────────────────────────

fn parse_environment(config: &serde_yaml::Value) -> Vec<String> {
    let mut env_vec = Vec::new();
    let Some(env) = config.get("environment") else { return env_vec };

    if let Some(seq) = env.as_sequence() {
        for item in seq {
            if let Some(s) = item.as_str() {
                env_vec.push(s.to_string());
            }
        }
    } else if let Some(map) = env.as_mapping() {
        for (k, v) in map {
            if let Some(key) = k.as_str() {
                let val = if let Some(s) = v.as_str() {
                    s.to_string()
                } else if let Some(b) = v.as_bool() {
                    b.to_string()
                } else if let Some(i) = v.as_i64() {
                    i.to_string()
                } else if let Some(f) = v.as_f64() {
                    f.to_string()
                } else if v.is_null() {
                    String::new()
                } else {
                    continue;
                };
                env_vec.push(format!("{}={}", key, val));
            }
        }
    }
    env_vec
}

fn parse_ports(config: &serde_yaml::Value) -> Vec<bollard::models::EndpointPortConfig> {
    let mut port_configs = Vec::new();
    let Some(ports) = config.get("ports").and_then(|p| p.as_sequence()) else { return port_configs };

    for port in ports {
        if let Some(port_str) = port.as_str() {
            let (main, protocol) = if let Some(idx) = port_str.rfind('/') {
                (&port_str[..idx], &port_str[idx+1..])
            } else {
                (port_str, "tcp")
            };
            let parts: Vec<&str> = main.split(':').collect();
            let (published, target) = match parts.len() {
                1 => (0i64, parts[0].parse::<i64>().unwrap_or(0)),
                2 => (parts[0].parse::<i64>().unwrap_or(0), parts[1].parse::<i64>().unwrap_or(0)),
                3 => (parts[1].parse::<i64>().unwrap_or(0), parts[2].parse::<i64>().unwrap_or(0)),
                _ => (0, 0),
            };
            if target > 0 {
                port_configs.push(bollard::models::EndpointPortConfig {
                    target_port: Some(target),
                    published_port: if published > 0 { Some(published) } else { None },
                    protocol: Some(match protocol {
                        "udp" => bollard::models::EndpointPortConfigProtocolEnum::UDP,
                        _ => bollard::models::EndpointPortConfigProtocolEnum::TCP,
                    }),
                    publish_mode: Some(bollard::models::EndpointPortConfigPublishModeEnum::INGRESS),
                    ..Default::default()
                });
            }
        } else if let Some(port_map) = port.as_mapping() {
            let target = port_map.get(serde_yaml::Value::String("target".into()))
                .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
                .unwrap_or(0) as i64;
            let published = port_map.get(serde_yaml::Value::String("published".into()))
                .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
                .unwrap_or(0) as i64;
            let protocol = port_map.get(serde_yaml::Value::String("protocol".into()))
                .and_then(|v| v.as_str())
                .unwrap_or("tcp");
            let mode = port_map.get(serde_yaml::Value::String("mode".into()))
                .and_then(|v| v.as_str())
                .unwrap_or("ingress");
            if target > 0 {
                port_configs.push(bollard::models::EndpointPortConfig {
                    target_port: Some(target),
                    published_port: if published > 0 { Some(published) } else { None },
                    protocol: Some(match protocol {
                        "udp" => bollard::models::EndpointPortConfigProtocolEnum::UDP,
                        _ => bollard::models::EndpointPortConfigProtocolEnum::TCP,
                    }),
                    publish_mode: Some(match mode {
                        "host" => bollard::models::EndpointPortConfigPublishModeEnum::HOST,
                        _ => bollard::models::EndpointPortConfigPublishModeEnum::INGRESS,
                    }),
                    ..Default::default()
                });
            }
        }
    }
    port_configs
}

fn parse_networks(
    config: &serde_yaml::Value,
    stack_name: &str,
    external_networks: &HashMap<String, String>,
) -> Option<Vec<bollard::models::NetworkAttachmentConfig>> {
    let Some(net_val) = config.get("networks") else {
        return Some(vec![bollard::models::NetworkAttachmentConfig {
            target: Some(format!("{}_default", stack_name)),
            ..Default::default()
        }]);
    };

    if let Some(seq) = net_val.as_sequence() {
        Some(seq.iter().filter_map(|n| n.as_str()).map(|n| {
            let net_name = if let Some(ext) = external_networks.get(n) {
                ext.clone()
            } else {
                format!("{}_{}", stack_name, n)
            };
            bollard::models::NetworkAttachmentConfig {
                target: Some(net_name),
                ..Default::default()
            }
        }).collect())
    } else if let Some(map) = net_val.as_mapping() {
        Some(map.keys().filter_map(|k| k.as_str()).map(|n| {
            let net_name = if let Some(ext) = external_networks.get(n) {
                ext.clone()
            } else {
                format!("{}_{}", stack_name, n)
            };
            bollard::models::NetworkAttachmentConfig {
                target: Some(net_name),
                ..Default::default()
            }
        }).collect())
    } else {
        None
    }
}

fn parse_command(config: &serde_yaml::Value) -> Option<Vec<String>> {
    config.get("command").and_then(|c| {
        if let Some(s) = c.as_str() {
            Some(vec!["/bin/sh".to_string(), "-c".to_string(), s.to_string()])
        } else if let Some(seq) = c.as_sequence() {
            Some(seq.iter().filter_map(|i| i.as_str().map(|s| s.to_string())).collect())
        } else {
            None
        }
    })
}

fn parse_volumes(
    config: &serde_yaml::Value,
    stack_name: &str,
    external_volumes: &HashMap<String, String>,
) -> Vec<bollard::models::Mount> {
    let mut mounts = Vec::new();
    let Some(volumes) = config.get("volumes").and_then(|v| v.as_sequence()) else { return mounts };

    for vol in volumes {
        if let Some(vol_str) = vol.as_str() {
            let (main, read_only) = if vol_str.ends_with(":ro") {
                (&vol_str[..vol_str.len()-3], true)
            } else if vol_str.ends_with(":rw") {
                (&vol_str[..vol_str.len()-3], false)
            } else {
                (vol_str, false)
            };
            let parts: Vec<&str> = main.splitn(2, ':').collect();
            if parts.len() == 2 {
                let source_raw = parts[0];
                let target = parts[1].to_string();
                let (typ, source) = if source_raw.starts_with('/') || source_raw.starts_with('.') {
                    (bollard::models::MountTypeEnum::BIND, source_raw.to_string())
                } else if let Some(ext) = external_volumes.get(source_raw) {
                    (bollard::models::MountTypeEnum::VOLUME, ext.clone())
                } else {
                    (bollard::models::MountTypeEnum::VOLUME, format!("{}_{}", stack_name, source_raw))
                };
                mounts.push(bollard::models::Mount {
                    target: Some(target),
                    source: Some(source),
                    typ: Some(typ),
                    read_only: Some(read_only),
                    ..Default::default()
                });
            } else {
                mounts.push(bollard::models::Mount {
                    target: Some(parts[0].to_string()),
                    typ: Some(bollard::models::MountTypeEnum::VOLUME),
                    ..Default::default()
                });
            }
        } else if let Some(vol_map) = vol.as_mapping() {
            let mount_type_str = vol_map.get(serde_yaml::Value::String("type".into()))
                .and_then(|v| v.as_str())
                .unwrap_or("volume");
            let source_raw = vol_map.get(serde_yaml::Value::String("source".into()))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let target = vol_map.get(serde_yaml::Value::String("target".into()))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let read_only = vol_map.get(serde_yaml::Value::String("read_only".into()))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if !target.is_empty() {
                let typ = match mount_type_str {
                    "bind" => bollard::models::MountTypeEnum::BIND,
                    "tmpfs" => bollard::models::MountTypeEnum::TMPFS,
                    _ => bollard::models::MountTypeEnum::VOLUME,
                };
                let source = if mount_type_str == "volume" && !source_raw.is_empty() && !source_raw.starts_with('/') {
                    if let Some(ext) = external_volumes.get(source_raw) {
                        ext.clone()
                    } else {
                        format!("{}_{}", stack_name, source_raw)
                    }
                } else {
                    source_raw.to_string()
                };
                mounts.push(bollard::models::Mount {
                    target: Some(target.to_string()),
                    source: if source.is_empty() { None } else { Some(source) },
                    typ: Some(typ),
                    read_only: Some(read_only),
                    ..Default::default()
                });
            }
        }
    }
    mounts
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── into_response ───────────────────────────────────────────

    #[test]
    fn response_all_ok() {
        let result = DeployResult {
            service_ids: vec!["svc-1".into(), "svc-2".into()],
            network_names: vec!["mystack_default".into()],
            volume_names: vec!["mystack_data".into()],
            failed: vec![],
        };
        let resp = into_response("mystack", result);
        assert!(resp.success);
        assert!(resp.message.contains("mystack"));
        assert!(resp.message.contains("2 services"));
        assert!(resp.message.contains("1 networks"));
        assert!(resp.message.contains("1 volumes"));
        assert_eq!(resp.service_ids, vec!["svc-1", "svc-2"]);
        assert_eq!(resp.network_names, vec!["mystack_default"]);
        assert_eq!(resp.volume_names, vec!["mystack_data"]);
        assert!(resp.failed_services.is_empty());
    }

    #[test]
    fn response_partial_failure() {
        let result = DeployResult {
            service_ids: vec!["svc-1".into()],
            network_names: vec!["mystack_default".into()],
            volume_names: vec![],
            failed: vec!["svc-2: image not found".into()],
        };
        let resp = into_response("mystack", result);
        assert!(!resp.success);
        assert!(resp.message.contains("partially deployed"));
        assert!(resp.message.contains("1 failed"));
        assert_eq!(resp.failed_services.len(), 1);
    }

    #[test]
    fn response_empty_deploy() {
        let result = DeployResult {
            service_ids: vec![],
            network_names: vec![],
            volume_names: vec![],
            failed: vec![],
        };
        let resp = into_response("empty", result);
        assert!(resp.success);
        assert!(resp.message.contains("0 services"));
    }

    // ── parse_environment ───────────────────────────────────────

    #[test]
    fn env_list_format() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            environment:
              - FOO=bar
              - BAZ=qux
        "#).unwrap();
        let env = parse_environment(&yaml);
        assert_eq!(env, vec!["FOO=bar", "BAZ=qux"]);
    }

    #[test]
    fn env_map_format() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            environment:
              DB_HOST: localhost
              DB_PORT: 5432
              DEBUG: true
        "#).unwrap();
        let env = parse_environment(&yaml);
        assert!(env.contains(&"DB_HOST=localhost".to_string()));
        assert!(env.contains(&"DB_PORT=5432".to_string()));
        assert!(env.contains(&"DEBUG=true".to_string()));
    }

    #[test]
    fn env_map_with_null_value() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            environment:
              EMPTY_VAR:
        "#).unwrap();
        let env = parse_environment(&yaml);
        assert_eq!(env, vec!["EMPTY_VAR="]);
    }

    #[test]
    fn env_map_float_value() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            environment:
              RATIO: 0.75
        "#).unwrap();
        let env = parse_environment(&yaml);
        assert_eq!(env, vec!["RATIO=0.75"]);
    }

    #[test]
    fn env_missing() {
        let yaml: serde_yaml::Value = serde_yaml::from_str("image: nginx").unwrap();
        let env = parse_environment(&yaml);
        assert!(env.is_empty());
    }

    // ── parse_ports ─────────────────────────────────────────────

    #[test]
    fn ports_simple_string() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            ports:
              - "8080:80"
        "#).unwrap();
        let ports = parse_ports(&yaml);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].target_port, Some(80));
        assert_eq!(ports[0].published_port, Some(8080));
        assert_eq!(ports[0].protocol, Some(bollard::models::EndpointPortConfigProtocolEnum::TCP));
    }

    #[test]
    fn ports_with_protocol() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            ports:
              - "53:53/udp"
        "#).unwrap();
        let ports = parse_ports(&yaml);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].target_port, Some(53));
        assert_eq!(ports[0].protocol, Some(bollard::models::EndpointPortConfigProtocolEnum::UDP));
    }

    #[test]
    fn ports_target_only() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            ports:
              - "80"
        "#).unwrap();
        let ports = parse_ports(&yaml);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].target_port, Some(80));
        assert_eq!(ports[0].published_port, None); // published=0 → None
    }

    #[test]
    fn ports_host_published_target() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            ports:
              - "0.0.0.0:8080:80"
        "#).unwrap();
        let ports = parse_ports(&yaml);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].published_port, Some(8080));
        assert_eq!(ports[0].target_port, Some(80));
    }

    #[test]
    fn ports_map_format() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            ports:
              - target: 3000
                published: 3000
                protocol: tcp
                mode: host
        "#).unwrap();
        let ports = parse_ports(&yaml);
        assert_eq!(ports.len(), 1);
        assert_eq!(ports[0].target_port, Some(3000));
        assert_eq!(ports[0].published_port, Some(3000));
        assert_eq!(ports[0].publish_mode, Some(bollard::models::EndpointPortConfigPublishModeEnum::HOST));
    }

    #[test]
    fn ports_missing() {
        let yaml: serde_yaml::Value = serde_yaml::from_str("image: nginx").unwrap();
        let ports = parse_ports(&yaml);
        assert!(ports.is_empty());
    }

    // ── parse_networks ──────────────────────────────────────────

    #[test]
    fn networks_list_format() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            networks:
              - frontend
              - backend
        "#).unwrap();
        let ext = HashMap::new();
        let nets = parse_networks(&yaml, "mystack", &ext);
        let targets: Vec<String> = nets.unwrap().iter()
            .map(|n| n.target.clone().unwrap())
            .collect();
        assert_eq!(targets, vec!["mystack_frontend", "mystack_backend"]);
    }

    #[test]
    fn networks_map_format() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            networks:
              frontend: {}
              backend: {}
        "#).unwrap();
        let ext = HashMap::new();
        let nets = parse_networks(&yaml, "mystack", &ext);
        let targets: Vec<String> = nets.unwrap().iter()
            .map(|n| n.target.clone().unwrap())
            .collect();
        assert!(targets.contains(&"mystack_frontend".to_string()));
        assert!(targets.contains(&"mystack_backend".to_string()));
    }

    #[test]
    fn networks_with_external() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            networks:
              - shared
        "#).unwrap();
        let mut ext = HashMap::new();
        ext.insert("shared".to_string(), "external-net".to_string());
        let nets = parse_networks(&yaml, "mystack", &ext);
        let targets: Vec<String> = nets.unwrap().iter()
            .map(|n| n.target.clone().unwrap())
            .collect();
        assert_eq!(targets, vec!["external-net"]);
    }

    #[test]
    fn networks_missing_returns_default() {
        let yaml: serde_yaml::Value = serde_yaml::from_str("image: nginx").unwrap();
        let ext = HashMap::new();
        let nets = parse_networks(&yaml, "mystack", &ext);
        let targets: Vec<String> = nets.unwrap().iter()
            .map(|n| n.target.clone().unwrap())
            .collect();
        assert_eq!(targets, vec!["mystack_default"]);
    }

    // ── parse_command ───────────────────────────────────────────

    #[test]
    fn command_string_format() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            command: "echo hello"
        "#).unwrap();
        let cmd = parse_command(&yaml);
        assert_eq!(cmd, Some(vec!["/bin/sh".to_string(), "-c".to_string(), "echo hello".to_string()]));
    }

    #[test]
    fn command_list_format() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            command:
              - python
              - app.py
              - --port=8000
        "#).unwrap();
        let cmd = parse_command(&yaml);
        assert_eq!(cmd, Some(vec!["python".to_string(), "app.py".to_string(), "--port=8000".to_string()]));
    }

    #[test]
    fn command_missing() {
        let yaml: serde_yaml::Value = serde_yaml::from_str("image: nginx").unwrap();
        let cmd = parse_command(&yaml);
        assert_eq!(cmd, None);
    }

    // ── parse_volumes (in service config) ───────────────────────

    #[test]
    fn volumes_string_named_volume() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            volumes:
              - data:/var/lib/data
        "#).unwrap();
        let ext = HashMap::new();
        let mounts = parse_volumes(&yaml, "mystack", &ext);
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].source, Some("mystack_data".to_string()));
        assert_eq!(mounts[0].target, Some("/var/lib/data".to_string()));
        assert_eq!(mounts[0].typ, Some(bollard::models::MountTypeEnum::VOLUME));
        assert_eq!(mounts[0].read_only, Some(false));
    }

    #[test]
    fn volumes_string_bind_mount() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            volumes:
              - /host/path:/container/path
        "#).unwrap();
        let ext = HashMap::new();
        let mounts = parse_volumes(&yaml, "mystack", &ext);
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].source, Some("/host/path".to_string()));
        assert_eq!(mounts[0].typ, Some(bollard::models::MountTypeEnum::BIND));
    }

    #[test]
    fn volumes_string_relative_bind() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            volumes:
              - ./local:/app
        "#).unwrap();
        let ext = HashMap::new();
        let mounts = parse_volumes(&yaml, "mystack", &ext);
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].source, Some("./local".to_string()));
        assert_eq!(mounts[0].typ, Some(bollard::models::MountTypeEnum::BIND));
    }

    #[test]
    fn volumes_string_read_only() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            volumes:
              - data:/var/lib/data:ro
        "#).unwrap();
        let ext = HashMap::new();
        let mounts = parse_volumes(&yaml, "mystack", &ext);
        assert_eq!(mounts[0].read_only, Some(true));
    }

    #[test]
    fn volumes_string_read_write() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            volumes:
              - data:/var/lib/data:rw
        "#).unwrap();
        let ext = HashMap::new();
        let mounts = parse_volumes(&yaml, "mystack", &ext);
        assert_eq!(mounts[0].read_only, Some(false));
    }

    #[test]
    fn volumes_string_external_volume() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            volumes:
              - shared:/data
        "#).unwrap();
        let mut ext = HashMap::new();
        ext.insert("shared".to_string(), "ext-shared-vol".to_string());
        let mounts = parse_volumes(&yaml, "mystack", &ext);
        assert_eq!(mounts[0].source, Some("ext-shared-vol".to_string()));
        assert_eq!(mounts[0].typ, Some(bollard::models::MountTypeEnum::VOLUME));
    }

    #[test]
    fn volumes_string_target_only() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            volumes:
              - /var/log
        "#).unwrap();
        let ext = HashMap::new();
        let mounts = parse_volumes(&yaml, "mystack", &ext);
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].target, Some("/var/log".to_string()));
        assert_eq!(mounts[0].typ, Some(bollard::models::MountTypeEnum::VOLUME));
    }

    #[test]
    fn volumes_map_format_bind() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            volumes:
              - type: bind
                source: /host/path
                target: /container/path
                read_only: true
        "#).unwrap();
        let ext = HashMap::new();
        let mounts = parse_volumes(&yaml, "mystack", &ext);
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].typ, Some(bollard::models::MountTypeEnum::BIND));
        assert_eq!(mounts[0].source, Some("/host/path".to_string()));
        assert_eq!(mounts[0].target, Some("/container/path".to_string()));
        assert_eq!(mounts[0].read_only, Some(true));
    }

    #[test]
    fn volumes_map_format_tmpfs() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            volumes:
              - type: tmpfs
                target: /tmp/cache
        "#).unwrap();
        let ext = HashMap::new();
        let mounts = parse_volumes(&yaml, "mystack", &ext);
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].typ, Some(bollard::models::MountTypeEnum::TMPFS));
        assert_eq!(mounts[0].target, Some("/tmp/cache".to_string()));
        assert_eq!(mounts[0].source, None);
    }

    #[test]
    fn volumes_map_format_volume_with_stack_prefix() {
        let yaml: serde_yaml::Value = serde_yaml::from_str(r#"
            volumes:
              - type: volume
                source: dbdata
                target: /var/lib/postgresql/data
        "#).unwrap();
        let ext = HashMap::new();
        let mounts = parse_volumes(&yaml, "mystack", &ext);
        assert_eq!(mounts[0].source, Some("mystack_dbdata".to_string()));
    }

    #[test]
    fn volumes_missing() {
        let yaml: serde_yaml::Value = serde_yaml::from_str("image: nginx").unwrap();
        let ext = HashMap::new();
        let mounts = parse_volumes(&yaml, "mystack", &ext);
        assert!(mounts.is_empty());
    }

    // ── DeployResult struct ─────────────────────────────────────

    #[test]
    fn deploy_result_into_response_preserves_all_fields() {
        let result = DeployResult {
            service_ids: vec!["a".into(), "b".into(), "c".into()],
            network_names: vec!["net-1".into(), "net-2".into()],
            volume_names: vec!["vol-1".into()],
            failed: vec![],
        };
        let resp = into_response("stack", result);
        assert_eq!(resp.service_ids.len(), 3);
        assert_eq!(resp.network_names.len(), 2);
        assert_eq!(resp.volume_names.len(), 1);
    }
}
