use tonic::{Request, Response, Status};
use bollard::models::ContainerInspectResponse as BollardInspectResponse;

use crate::docker::client::DockerError;
use crate::state::SharedState;

use super::proto::{
    inventory_service_server::InventoryService,
    ContainerListRequest, ContainerListResponse,
    ContainerInspectRequest, ContainerInspectResponse,
    ContainerInfo as ProtoContainerInfo,
    ContainerDetails, VolumeMount, NetworkInfo, ResourceLimits,
    ContainerStateFilter, PortMapping as ProtoPortMapping,
    ContainerStateInfo as ProtoContainerStateInfo,
    RestartPolicy as ProtoRestartPolicy,
    HealthcheckConfig as ProtoHealthcheckConfig,
};

/// Implementation of the InventoryService gRPC service
/// Handles container listing and inspection with caching and filtering
pub struct InventoryServiceImpl {
    state: SharedState,
}

impl InventoryServiceImpl {
    pub fn new(state: SharedState) -> Self {
        Self { state }
    }

    /// Convert internal ContainerInfo to protobuf
    fn convert_container_info(info: crate::docker::inventory::ContainerInfo) -> ProtoContainerInfo {
        ProtoContainerInfo {
            id: info.id,
            name: info.name,
            image: info.image,
            state: info.state,
            status: info.status,
            log_driver: info.log_driver,
            labels: info.labels,
            created_at: info.created_at,
            ports: info.ports.into_iter().map(|p| ProtoPortMapping {
                container_port: p.container_port as u32,
                protocol: p.protocol,
                host_ip: p.host_ip,
                host_port: p.host_port.map(|p| p as u32),
            }).collect(),
            state_info: info.state_info.map(|si| ProtoContainerStateInfo {
                oom_killed: si.oom_killed,
                pid: si.pid,
                exit_code: si.exit_code,
                started_at: si.started_at,
                finished_at: si.finished_at,
                restart_count: si.restart_count,
            }),
        }
    }

    /// Extract ContainerDetails from Bollard's ContainerInspectResponse
    /// Includes ports, mounts, networks, and resource limits
    fn extract_container_details(inspect: &BollardInspectResponse) -> Option<ContainerDetails> {
        // Extract exposed ports from NetworkSettings.Ports
        let mut exposed_ports = Vec::new();
        
        if let Some(network_settings) = &inspect.network_settings {
            if let Some(ports) = &network_settings.ports {
                for (container_port, host_bindings) in ports {
                    // container_port is like "80/tcp"
                    // host_bindings is Option<Vec<PortBinding>>
                    
                    let bindings_list = host_bindings.as_deref().unwrap_or(&[]);
                    
                    if !bindings_list.is_empty() {
                        for binding in bindings_list {
                            // Format: "80/tcp -> 0.0.0.0:8080"
                            let host_ip = binding.host_ip.as_deref().unwrap_or("0.0.0.0");
                            let host_port = binding.host_port.as_deref().unwrap_or("?");
                            let port_str = format!("{} -> {}:{}", container_port, host_ip, host_port);
                            exposed_ports.push(port_str);
                        }
                    } else {
                        // Port exposed but not bound to host
                        exposed_ports.push(container_port.clone());
                    }
                }
            }
        }

        // Extract volume mounts
        let mounts = if let Some(bollard_mounts) = &inspect.mounts {
            bollard_mounts.iter().filter_map(|m| {
                Some(VolumeMount {
                    source: m.source.clone().unwrap_or_default(),
                    destination: m.destination.clone().unwrap_or_default(),
                    mode: m.mode.clone().unwrap_or_else(|| "rw".to_string()),
                    mount_type: m.typ.as_ref()
                        .map(|t| format!("{:?}", t).to_lowercase())
                        .unwrap_or_default(),
                    propagation: m.propagation.clone().unwrap_or_default(),
                })
            }).collect()
        } else {
            Vec::new()
        };

        // Extract network information
        let networks = if let Some(network_settings) = &inspect.network_settings {
            if let Some(networks_map) = &network_settings.networks {
                networks_map.iter().map(|(name, network)| {
                    NetworkInfo {
                        network_name: name.clone(),
                        ip_address: network.ip_address.clone().unwrap_or_default(),
                        gateway: network.gateway.clone().unwrap_or_default(),
                        mac_address: network.mac_address.clone().unwrap_or_default(),
                    }
                }).collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Extract resource limits
        let limits = inspect.host_config.as_ref().map(|hc| {
            // Logic to determine CPU limit in cores (e.g., 0.5, 2.0):
            // 1. nano_cpus: Precision CPU limit (1.5 cpus = 1500000000)
            // 2. cpu_quota / cpu_period: Traditional CPU limit
            // Note: cpu_shares is a relative weight, not a hard core limit, so we ignore it.
            
            let cpu_limit = if let Some(nano) = hc.nano_cpus {
                 if nano > 0 {
                     Some(nano as f64 / 1_000_000_000.0)
                 } else {
                     None
                 }
            } else if let (Some(quota), Some(period)) = (hc.cpu_quota, hc.cpu_period) {
                 if quota > 0 && period > 0 {
                     Some(quota as f64 / period as f64)
                 } else {
                     None
                 }
            } else {
                 None
            };

            ResourceLimits {
                memory_limit_bytes: hc.memory,
                cpu_limit,
                pids_limit: hc.pids_limit,
            }
        });

        // Extract command
        let command = inspect.config.as_ref()
            .and_then(|c| c.cmd.clone())
            .unwrap_or_default();

        // Extract working directory
        let working_dir = inspect.config.as_ref()
            .and_then(|c| c.working_dir.clone())
            .unwrap_or_default();

        // Extract environment variables
        let env = inspect.config.as_ref()
            .and_then(|c| c.env.clone())
            .unwrap_or_default();

        // Extract entrypoint
        let entrypoint = inspect.config.as_ref()
            .and_then(|c| c.entrypoint.clone())
            .unwrap_or_default();

        // Extract hostname
        let hostname = inspect.config.as_ref()
            .and_then(|c| c.hostname.clone())
            .unwrap_or_default();

        // Extract user
        let user = inspect.config.as_ref()
            .and_then(|c| c.user.clone())
            .unwrap_or_default();

        // Extract restart policy
        let restart_policy = inspect.host_config.as_ref()
            .and_then(|hc| hc.restart_policy.as_ref())
            .map(|rp| ProtoRestartPolicy {
                name: rp.name.as_ref()
                    .map(|n| format!("{:?}", n).to_lowercase())
                    .unwrap_or_else(|| "no".to_string()),
                max_retry_count: rp.maximum_retry_count
                    .map(|c| c as i32)
                    .unwrap_or(0),
            });

        // Extract network mode
        let network_mode = inspect.host_config.as_ref()
            .and_then(|hc| hc.network_mode.clone())
            .unwrap_or_default();

        // Extract healthcheck configuration
        let healthcheck = inspect.config.as_ref()
            .and_then(|c| c.healthcheck.as_ref())
            .map(|hc| ProtoHealthcheckConfig {
                test: hc.test.clone().unwrap_or_default(),
                interval_ns: hc.interval.unwrap_or(0),
                timeout_ns: hc.timeout.unwrap_or(0),
                retries: hc.retries.map(|r| r as i32).unwrap_or(0),
                start_period_ns: hc.start_period.unwrap_or(0),
            });

        // Extract platform
        let platform = inspect.platform.clone().unwrap_or_default();

        // Extract runtime
        let runtime = inspect.host_config.as_ref()
            .and_then(|hc| hc.runtime.clone())
            .unwrap_or_default();

        Some(ContainerDetails {
            command,
            working_dir,
            env,
            exposed_ports,
            mounts,
            networks,
            limits,
            entrypoint,
            hostname,
            user,
            restart_policy,
            network_mode,
            healthcheck,
            platform,
            runtime,
        })
    }

    /// Optimized filter: avoids string allocation in the hot loop
    fn apply_state_filter(
        containers: Vec<crate::docker::inventory::ContainerInfo>,
        filter: i32,
    ) -> Vec<crate::docker::inventory::ContainerInfo> {
        let filter_enum = ContainerStateFilter::try_from(filter)
            .unwrap_or(ContainerStateFilter::Unspecified);

        if matches!(filter_enum, ContainerStateFilter::All | ContainerStateFilter::Unspecified) {
            return containers;
        }

        // Docker states are usually returned as "running", "exited", etc.
        // We match against the standard string representations.
        let target_state = match filter_enum {
            ContainerStateFilter::Running => "running",
            ContainerStateFilter::Paused => "paused",
            ContainerStateFilter::Exited => "exited",
            ContainerStateFilter::Created => "created",
            _ => return containers,
        };

        containers.into_iter()
            .filter(|c| c.state.eq_ignore_ascii_case(target_state))
            .collect()
    }
}

#[tonic::async_trait]
impl InventoryService for InventoryServiceImpl {
    async fn list_containers(
        &self,
        request: Request<ContainerListRequest>,
    ) -> Result<Response<ContainerListResponse>, Status> {
        let req = request.into_inner();

        // ARCHITECTURE: Read-only cache access
        // The background sync task (background_inventory_sync) continuously updates
        // this cache. This ensures:
        // - Fast response times (pure memory read, no Docker API calls)
        // - DoS protection (Docker is never hammered by concurrent requests)
        // - Data may be up to N seconds stale (configurable via sync interval)
        
        let mut containers: Vec<_> = self.state.inventory
            .iter()
            .map(|entry| entry.value().clone())
            .collect();

        // 1. Apply State Filter
        if let Some(state_filter) = req.state_filter {
            containers = Self::apply_state_filter(containers, state_filter);
        }

        // 2. Apply "Include Stopped" Logic
        // Logic: ONLY filter out non-running if:
        // a) include_stopped is FALSE AND
        // b) We haven't already filtered for a specific state that might be stopped.
        //    (e.g. if user asked for "Exited", don't filter them out now)
        let has_explicit_state_filter = req.state_filter.map_or(false, |sf| {
             let enum_val = ContainerStateFilter::try_from(sf).unwrap_or(ContainerStateFilter::Unspecified);
             !matches!(enum_val, ContainerStateFilter::Unspecified | ContainerStateFilter::All)
        });

        if !req.include_stopped && !has_explicit_state_filter {
            containers.retain(|c| c.state.eq_ignore_ascii_case("running"));
        }

        let total_count = containers.len() as u32;

        // 3. Apply Limit
        if let Some(limit) = req.limit {
            let limit = limit as usize;
            if limit < containers.len() {
                containers.truncate(limit);
            }
        }

        let proto_containers = containers
            .into_iter()
            .map(Self::convert_container_info)
            .collect();

        Ok(Response::new(ContainerListResponse {
            containers: proto_containers,
            total_count,
        }))
    }

    async fn inspect_container(
        &self,
        request: Request<ContainerInspectRequest>,
    ) -> Result<Response<ContainerInspectResponse>, Status> {
        let req = request.into_inner();

        // IMPORTANT: Always fetch fresh data for inspections.
        // Caching here causes stale state bugs (e.g. reporting "Running" when "Exited").
        // The Docker inspect API is fast enough for direct calls.
        
        // Fetch raw inspect response for detailed information
        // Optimization: Single API call instead of two
        let raw_inspect = self.state.docker
            .inspect_container_raw(&req.container_id)
            .await
            .map_err(|e| match e {
                DockerError::ContainerNotFound(msg) => Status::not_found(msg),
                _ => Status::internal(format!("Docker inspect raw failed: {}", e)),
            })?;

        // Derive basic info from raw response locally
        let info = crate::docker::inventory::ContainerInfo::from(raw_inspect.clone());

        // Extract detailed information (ports, mounts, networks, etc.)
        let details = Self::extract_container_details(&raw_inspect);

        // Update cache with the fresh truth
        self.state.inventory.insert(info.id.clone(), info.clone());

        Ok(Response::new(ContainerInspectResponse {
            info: Some(Self::convert_container_info(info)),
            details, 
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::{HostConfig, ContainerConfig, NetworkSettings};
    use crate::docker::inventory::ContainerInfo;
    use std::collections::HashMap;

    fn create_test_container(id: &str, state: &str) -> ContainerInfo {
        ContainerInfo {
            id: id.to_string(),
            name: format!("name-{}", id),
            image: "image".to_string(),
            state: state.to_string(),
            status: "status".to_string(),
            log_driver: None,
            labels: HashMap::new(),
            created_at: 0,
            ports: vec![],
            state_info: None,
        }
    }

    #[test]
    fn test_apply_state_filter() {
        let containers = vec![
            create_test_container("1", "running"),
            create_test_container("2", "exited"),
            create_test_container("3", "paused"),
            create_test_container("4", "created"),
        ];

        // Test Running
        let running = InventoryServiceImpl::apply_state_filter(
            containers.clone(), 
            ContainerStateFilter::Running as i32
        );
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].state, "running");

        // Test Exited
        let exited = InventoryServiceImpl::apply_state_filter(
            containers.clone(), 
            ContainerStateFilter::Exited as i32
        );
        assert_eq!(exited.len(), 1);
        assert_eq!(exited[0].state, "exited");

        // Test All (should return all)
        let all = InventoryServiceImpl::apply_state_filter(
            containers.clone(), 
            ContainerStateFilter::All as i32
        );
        assert_eq!(all.len(), 4);
    }

    #[test]
    fn test_extract_container_details_cpu_limits() {
        // Case 1: nano_cpus (Active)
        let mut hc = HostConfig::default();
        hc.nano_cpus = Some(1_500_000_000); // 1.5 CPUs
        hc.memory = Some(1024);
        
        let inspect = BollardInspectResponse {
            host_config: Some(hc),
            config: Some(ContainerConfig::default()),
            network_settings: Some(NetworkSettings::default()),
            ..Default::default()
        };
        
        let details = InventoryServiceImpl::extract_container_details(&inspect).expect("Should extract details");
        let limits = details.limits.expect("Should have limits");
        assert_eq!(limits.cpu_limit, Some(1.5));

        // Case 2: quota/period (Legacy/Compat)
        let mut hc2 = HostConfig::default();
        hc2.nano_cpus = None; // clear nano
        hc2.cpu_quota = Some(50000);
        hc2.cpu_period = Some(100000); // 0.5 CPUs
        
        let inspect2 = BollardInspectResponse {
            host_config: Some(hc2),
            config: Some(ContainerConfig::default()),
            network_settings: Some(NetworkSettings::default()),
            ..Default::default()
        };

        let details2 = InventoryServiceImpl::extract_container_details(&inspect2).expect("Should extract details");
        let limits2 = details2.limits.expect("Should have limits");
        assert_eq!(limits2.cpu_limit, Some(0.5));

        // Case 3: No limits
        let mut hc3 = HostConfig::default();
        hc3.nano_cpus = None;
        hc3.cpu_quota = None;
        
        let inspect3 = BollardInspectResponse {
            host_config: Some(hc3),
            config: Some(ContainerConfig::default()),
            network_settings: Some(NetworkSettings::default()),
            ..Default::default()
        };

        let details3 = InventoryServiceImpl::extract_container_details(&inspect3).expect("Should extract details");
        let limits3 = details3.limits.expect("Should have limits");
        assert_eq!(limits3.cpu_limit, None);
    }

    #[test]
    fn test_include_stopped_logic() {
        // Validate the boolean logic we implemented in list_containers
        // logic: !req.include_stopped && !has_explicit_state_filter
        
        let check_filter = |include_stopped: bool, state_filter: Option<i32>| -> bool {
             let has_explicit_state_filter = state_filter.map_or(false, |sf| {
                 let enum_val = ContainerStateFilter::try_from(sf).unwrap_or(ContainerStateFilter::Unspecified);
                 !matches!(enum_val, ContainerStateFilter::Unspecified | ContainerStateFilter::All)
            });
            
            !include_stopped && !has_explicit_state_filter
        };

        // Default case: Don't include stopped, no filter -> Should Filter (Return True to "retain only running")
        assert_eq!(check_filter(false, None), true);

        // User asks for "Exited": Don't include stopped (default), but Explicit Filter -> Should NOT Filter (Return False to keep all)
        // (Because apply_state_filter handles the reduction to just "axited" later/earlier)
        assert_eq!(check_filter(false, Some(ContainerStateFilter::Exited as i32)), false);

        // User asks for "Running": Don't include stopped, Explicit Filter -> Should NOT Filter (Return False)
        // (apply_state_filter will handle it)
        assert_eq!(check_filter(false, Some(ContainerStateFilter::Running as i32)), false);
        
        // User explicitly says include_stopped=true -> Should NOT Filter (Return False)
        assert_eq!(check_filter(true, None), false);
    }
}
