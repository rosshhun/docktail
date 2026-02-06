use bollard::models::{ContainerSummary, ContainerInspectResponse};
use chrono::DateTime;

/// Port mapping information
#[derive(Debug, Clone, serde::Serialize)]
pub struct PortMapping {
    pub container_port: u16,
    pub protocol: String,
    pub host_ip: Option<String>,
    pub host_port: Option<u16>,
}

/// Detailed container state information from docker inspect
#[derive(Debug, Clone, serde::Serialize)]
pub struct ContainerStateInfo {
    pub oom_killed: bool,
    pub pid: i64,
    pub exit_code: i32,
    pub started_at: String,
    pub finished_at: String,
    pub restart_count: i32,
}

/// Basic container information derived from Docker's list API.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ContainerInfo {
    pub id: String,         // Full container ID 64-char hash
    pub name: String,      // Without leading slash
    pub image: String,        
    pub state: String,       // "running", "paused", "exited"
    pub status: String,      // "Up 2 hours"
    pub log_driver: Option<String>,  // Critical for checking "Time-Travel" support
    pub labels: std::collections::HashMap<String, String>,
    pub created_at: i64,     // Unix timestamp (better for gRPC)
    pub ports: Vec<PortMapping>,  // Structured port mappings
    pub state_info: Option<ContainerStateInfo>,  // Detailed state from inspect
}

impl From<ContainerSummary> for ContainerInfo {
    fn from(s: ContainerSummary) -> Self {
        // Convert PortSummary to our PortMapping struct
        let ports = s.ports
            .unwrap_or_default()
            .into_iter()
            .map(|p| {
                let protocol = p.typ
                    .map(|t| t.to_string().to_lowercase())
                    .unwrap_or_else(|| "tcp".to_string());
                
                PortMapping {
                    container_port: p.private_port,
                    protocol,
                    host_ip: if p.public_port.is_some() { p.ip } else { None },
                    host_port: p.public_port,
                }
            })
            .collect();

        Self {
            id: s.id.unwrap_or_default(),
            name: s.names.as_deref()             // Turn Option<Vec> into Option<&[String]>
                .and_then(|n| n.first())         // Get first item
                .map(|n| n.trim_start_matches('/'))
                .unwrap_or("unknown")            // Fallback
                .to_string(),
            image: s.image.unwrap_or_default(),
            state: s.state
                .map(|s| s.to_string())
                .unwrap_or_else(|| "unknown".into()),
            status: s.status.unwrap_or_default(),
            log_driver: None, // Not available in list API
            labels: s.labels.unwrap_or_default(),
            created_at: s.created.unwrap_or_default(),
            ports,
            state_info: None, // Not available in list API
        }
    }
}

impl From<ContainerInspectResponse> for ContainerInfo {
    fn from(details: ContainerInspectResponse) -> Self {
        // Extract Log Driver safely from deep nesting
        // Path: HostConfig -> LogConfig -> Type
        let log_driver = details.host_config
            .as_ref()
            .and_then(|hc| hc.log_config.as_ref())
            .and_then(|lc| lc.typ.clone());

        // Parse "Created" time (RFC3339 string format)
        // Inspect returns a String (RFC3339) unlike List which returns i64
        let created_at = details.created.as_deref()
            .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
            .map(|dt| dt.timestamp())
            .unwrap_or(0);

        // Extract ports from inspect response
        // NetworkSettings -> Ports is a HashMap<String, Option<Vec<PortBinding>>>
        let ports = details.network_settings
            .as_ref()
            .and_then(|ns| ns.ports.as_ref())
            .map(|port_map| {
                port_map.iter()
                    .flat_map(|(container_port_str, bindings)| {
                        // Parse container_port from "80/tcp" format
                        let (port_num, protocol) = container_port_str
                            .split_once('/')
                            .unwrap_or((container_port_str.as_str(), "tcp"));
                        let container_port = port_num.parse::<u16>().unwrap_or(0);
                        
                        // Handle both None and Some([]) as "exposed but not bound"
                        let bindings_list = bindings.as_deref().unwrap_or(&[]);
                        
                        if !bindings_list.is_empty() {
                            // Port is bound to host (can have multiple bindings)
                            bindings_list.iter().map(|binding| {
                                let host_ip = binding.host_ip.clone();
                                let host_port = binding.host_port.as_ref()
                                    .and_then(|p| p.parse::<u16>().ok());
                                
                                PortMapping {
                                    container_port,
                                    protocol: protocol.to_string(),
                                    host_ip,
                                    host_port,
                                }
                            }).collect::<Vec<_>>()
                        } else {
                            // Port is exposed but not bound
                            vec![PortMapping {
                                container_port,
                                protocol: protocol.to_string(),
                                host_ip: None,
                                host_port: None,
                            }]
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Extract detailed state information from inspect
        let state_info = details.state.as_ref().map(|s| {
            ContainerStateInfo {
                oom_killed: s.oom_killed.unwrap_or(false),
                pid: s.pid.map(|p| p as i64).unwrap_or(0),
                exit_code: s.exit_code.map(|c| c as i32).unwrap_or(0),
                started_at: s.started_at.clone().unwrap_or_default(),
                finished_at: s.finished_at.clone().unwrap_or_default(),
                restart_count: details.restart_count.map(|c| c as i32).unwrap_or(0),
            }
        });

        Self {
            id: details.id.unwrap_or_default(),
            name: details.name
                .map(|n| n.trim_start_matches('/').to_string())
                .unwrap_or_else(|| "unknown".into()),
            image: details.image.unwrap_or_default(),
            state: details.state.as_ref()
                .and_then(|s| s.status.as_ref())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "unknown".into()),
            // Status is often empty in Inspect, unlike List
            // We reconstruct it from state
            status: details.state.as_ref()
                .and_then(|s| s.status.as_ref())
                .map(|s| format!("{:?}", s))
                .unwrap_or_default(),
            
            log_driver, // Critical for time-travel support validation
            
            labels: details.config
                .and_then(|c| c.labels)
                .unwrap_or_default(),
            created_at,
            ports,
            state_info,
        }
    }
}