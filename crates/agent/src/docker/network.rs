//! Network domain — list, inspect, create, remove, connect, disconnect.

use super::client::{DockerClient, DockerError};

impl DockerClient {
    /// List all networks.
    pub async fn list_networks(&self) -> Result<Vec<bollard::models::Network>, DockerError> {
        self.client
            .list_networks(None::<bollard::query_parameters::ListNetworksOptions>)
            .await
            .map_err(DockerError::from)
    }

    /// Inspect a specific network.
    pub async fn inspect_network(
        &self,
        network_id: &str,
    ) -> Result<bollard::models::NetworkInspect, DockerError> {
        self.client
            .inspect_network(
                network_id,
                None::<bollard::query_parameters::InspectNetworkOptions>,
            )
            .await
            .map_err(DockerError::from)
    }

    /// Create a new network.
    pub async fn create_network(
        &self,
        name: &str,
        driver: Option<&str>,
        labels: std::collections::HashMap<String, String>,
        internal: bool,
        attachable: bool,
        enable_ipv6: bool,
        options: std::collections::HashMap<String, String>,
        ipam: Option<bollard::models::Ipam>,
    ) -> Result<bollard::models::NetworkCreateResponse, DockerError> {
        use bollard::models::NetworkCreateRequest;

        let config = NetworkCreateRequest {
            name: name.to_string(),
            driver: Some(driver.unwrap_or("bridge").to_string()),
            internal: if internal { Some(true) } else { None },
            attachable: if attachable { Some(true) } else { None },
            enable_ipv6: if enable_ipv6 { Some(true) } else { None },
            options: if options.is_empty() {
                None
            } else {
                Some(options)
            },
            ipam,
            labels: Some(labels),
            ..Default::default()
        };

        self.client
            .create_network(config)
            .await
            .map_err(DockerError::from)
    }

    /// Remove a network.
    pub async fn remove_network(&self, network_id: &str) -> Result<(), DockerError> {
        self.client
            .remove_network(network_id)
            .await
            .map_err(DockerError::from)
    }

    /// Connect a container to a network.
    pub async fn network_connect(
        &self,
        network_id: &str,
        container_id: &str,
    ) -> Result<(), DockerError> {
        use bollard::models::NetworkConnectRequest;

        let config = NetworkConnectRequest {
            container: container_id.to_string(),
            ..Default::default()
        };

        self.client
            .connect_network(network_id, config)
            .await
            .map_err(DockerError::from)
    }

    /// Disconnect a container from a network.
    pub async fn network_disconnect(
        &self,
        network_id: &str,
        container_id: &str,
        force: bool,
    ) -> Result<(), DockerError> {
        use bollard::models::NetworkDisconnectRequest;

        let config = NetworkDisconnectRequest {
            container: container_id.to_string(),
            force: Some(force),
        };

        self.client
            .disconnect_network(network_id, config)
            .await
            .map_err(DockerError::from)
    }
}
