//! Volume domain — list, inspect, create, remove.

use super::client::{DockerClient, DockerError};

impl DockerClient {
    /// List all volumes.
    pub async fn list_volumes(&self) -> Result<bollard::models::VolumeListResponse, DockerError> {
        self.client
            .list_volumes(None::<bollard::query_parameters::ListVolumesOptions>)
            .await
            .map_err(DockerError::from)
    }

    /// Inspect a specific volume.
    pub async fn inspect_volume(
        &self,
        name: &str,
    ) -> Result<bollard::models::Volume, DockerError> {
        self.client
            .inspect_volume(name)
            .await
            .map_err(DockerError::from)
    }

    /// Create a new volume.
    pub async fn create_volume(
        &self,
        name: &str,
        driver: Option<&str>,
        labels: std::collections::HashMap<String, String>,
        driver_opts: std::collections::HashMap<String, String>,
    ) -> Result<bollard::models::Volume, DockerError> {
        use bollard::models::VolumeCreateRequest;

        let config = VolumeCreateRequest {
            name: Some(name.to_string()),
            driver: Some(driver.unwrap_or("local").to_string()),
            driver_opts: if driver_opts.is_empty() {
                None
            } else {
                Some(driver_opts)
            },
            labels: Some(labels),
            ..Default::default()
        };

        self.client
            .create_volume(config)
            .await
            .map_err(DockerError::from)
    }

    /// Remove a volume.
    pub async fn remove_volume(&self, name: &str, force: bool) -> Result<(), DockerError> {
        use bollard::query_parameters::RemoveVolumeOptions;

        let options = Some(RemoveVolumeOptions { force });

        self.client
            .remove_volume(name, options)
            .await
            .map_err(DockerError::from)
    }
}
