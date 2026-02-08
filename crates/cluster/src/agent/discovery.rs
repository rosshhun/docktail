use super::{AgentPool, AgentSource};
use crate::config::{AgentConfig, DiscoveryConfig};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tokio::time;
use tracing::{debug, error, info, warn};

/// Agent discovery service that finds agents via Swarm node labels
pub struct AgentDiscovery {
    pool: Arc<AgentPool>,
    config: DiscoveryConfig,
    shutdown_rx: tokio::sync::watch::Receiver<bool>,
}

impl AgentDiscovery {
    pub fn new(
        pool: Arc<AgentPool>,
        config: DiscoveryConfig,
        shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Self {
        Self {
            pool,
            config,
            shutdown_rx,
        }
    }

    /// Start the Swarm-aware discovery loop
    pub async fn start_swarm_discovery(mut self) {
        if !self.config.swarm_discovery {
            info!("Swarm agent discovery is disabled");
            return;
        }

        info!(
            "Starting Swarm agent discovery (label={}, interval={}s, port={})",
            self.config.discovery_label,
            self.config.discovery_interval_secs,
            self.config.agent_port,
        );

        // Parse the discovery label into key=value
        let (label_key, label_value) = match self.config.discovery_label.split_once('=') {
            Some((k, v)) => (k.to_string(), v.to_string()),
            None => {
                error!("Invalid discovery_label format: '{}'. Expected 'key=value'", self.config.discovery_label);
                return;
            }
        };

        let mut interval = time::interval(Duration::from_secs(self.config.discovery_interval_secs));
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        // Track which agents we've discovered so we can detect removals
        let mut discovered_ids: HashSet<String> = HashSet::new();

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    match self.discover_agents_from_swarm(&label_key, &label_value, &mut discovered_ids).await {
                        Ok(count) => {
                            if count > 0 {
                                info!("Swarm discovery: found {} agent(s)", count);
                            } else {
                                debug!("Swarm discovery: no new agents found");
                            }
                        }
                        Err(e) => {
                            warn!("Swarm discovery scan failed: {}", e);
                        }
                    }
                }
                _ = self.shutdown_rx.changed() => {
                    if *self.shutdown_rx.borrow() {
                        info!("Received shutdown signal, stopping Swarm discovery");
                        break;
                    }
                }
            }
        }

        info!("Swarm agent discovery stopped");
    }

    /// Query existing healthy agents for Swarm node list, find nodes with the discovery label
    async fn discover_agents_from_swarm(
        &self,
        label_key: &str,
        label_value: &str,
        discovered_ids: &mut HashSet<String>,
    ) -> Result<usize, String> {
        // Use any healthy agent to query Swarm nodes
        let agents = self.pool.list_agents();
        let healthy_agent = agents.iter().find(|a| a.is_healthy());

        let agent = match healthy_agent {
            Some(a) => a,
            None => {
                debug!("No healthy agents available for Swarm discovery");
                return Ok(0);
            }
        };

        // Query ListNodes via the healthy agent
        let mut client = agent.client.lock().await.clone();
        let response = client
            .list_nodes(super::client::NodeListRequest {})
            .await
            .map_err(|e| format!("Failed to list nodes: {}", e))?;

        let mut new_count = 0;
        let mut current_discovered: HashSet<String> = HashSet::new();

        for node in &response.nodes {
            // Check if node has the discovery label
            let has_label = node.labels.iter().any(|(k, v)| k == label_key && v == label_value);
            if !has_label {
                continue;
            }

            // Derive agent ID from node ID
            let agent_id = format!("discovered-{}", &node.id);
            current_discovered.insert(agent_id.clone());

            // Skip if already in pool
            if self.pool.get_agent(&agent_id).is_some() {
                continue;
            }

            // Determine the agent address from node's hostname/addr
            let node_addr = if !node.addr.is_empty() {
                // addr might include port (e.g. "10.0.0.1:2377"), strip the port
                node.addr.split(':').next().unwrap_or(&node.addr).to_string()
            } else if !node.hostname.is_empty() {
                node.hostname.clone()
            } else {
                warn!("Node {} has no addr or hostname, skipping", node.id);
                continue;
            };

            let address = format!("{}:{}", node_addr, self.config.agent_port);
            let name = if !node.hostname.is_empty() {
                format!("Discovered: {}", node.hostname)
            } else {
                format!("Discovered: {}", node.id)
            };

            // Need TLS credentials for dynamic agents
            let (tls_cert, tls_key, tls_ca) = match (&self.config.tls_cert, &self.config.tls_key, &self.config.tls_ca) {
                (Some(cert), Some(key), Some(ca)) => (cert.clone(), key.clone(), ca.clone()),
                _ => {
                    warn!(
                        "Cannot add discovered agent {}: TLS credentials not configured in [discovery]. \
                         Set discovery.tls_cert, discovery.tls_key, and discovery.tls_ca.",
                        agent_id
                    );
                    continue;
                }
            };

            let mut labels = node.labels.clone();
            labels.insert("discovery.source".to_string(), "swarm".to_string());
            labels.insert("discovery.node_id".to_string(), node.id.clone());
            let role_str = match node.role {
                1 => "manager",
                2 => "worker",
                _ => "unknown",
            };
            labels.insert("discovery.node_role".to_string(), role_str.to_string());

            let agent_config = AgentConfig {
                id: agent_id.clone(),
                name,
                address,
                tls_cert,
                tls_key,
                tls_ca,
                tls_domain: self.config.tls_domain.clone(),
                labels,
            };

            info!("Discovered new agent via Swarm node label: {} (node={})", agent_id, node.id);
            match self.pool.add_dynamic_agent(agent_config, AgentSource::Discovered).await {
                Ok(_) => {
                    new_count += 1;
                    discovered_ids.insert(agent_id);
                }
                Err(e) => {
                    warn!("Failed to add discovered agent {}: {}", agent_id, e);
                }
            }
        }

        // Remove agents that were previously discovered but whose nodes no longer have the label
        let stale_ids: Vec<String> = discovered_ids
            .iter()
            .filter(|id| !current_discovered.contains(*id))
            .cloned()
            .collect();

        for stale_id in &stale_ids {
            if let Some(conn) = self.pool.get_agent(stale_id) {
                if conn.source == AgentSource::Discovered {
                    info!("Removing stale discovered agent: {} (node label removed)", stale_id);
                    self.pool.remove_agent(stale_id);
                    discovered_ids.remove(stale_id);
                }
            }
        }

        Ok(new_count)
    }
}
