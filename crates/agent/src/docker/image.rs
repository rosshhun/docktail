//! Image domain — list, inspect, pull, remove.

use super::client::{DockerClient, DockerError};
use futures_util::stream::StreamExt;

impl DockerClient {
    /// List all images on the Docker host.
    pub async fn list_images(&self) -> Result<Vec<bollard::models::ImageSummary>, DockerError> {
        use bollard::query_parameters::ListImagesOptions;

        let options = Some(ListImagesOptions {
            all: false,
            ..Default::default()
        });

        self.client.list_images(options).await.map_err(DockerError::from)
    }

    /// Inspect a specific image by ID or tag.
    pub async fn inspect_image(
        &self,
        image_id: &str,
    ) -> Result<bollard::models::ImageInspect, DockerError> {
        self.client
            .inspect_image(image_id)
            .await
            .map_err(DockerError::from)
    }

    /// Pull an image from a registry. Returns when the pull is complete.
    pub async fn pull_image(
        &self,
        image: &str,
        tag: &str,
        registry_auth: Option<&str>,
    ) -> Result<(), DockerError> {
        use bollard::auth::DockerCredentials;
        use bollard::query_parameters::CreateImageOptions;

        let options = Some(CreateImageOptions {
            from_image: Some(image.to_string()),
            tag: Some(tag.to_string()),
            ..Default::default()
        });

        let credentials = registry_auth.map(|auth| DockerCredentials {
            auth: Some(auth.to_string()),
            ..Default::default()
        });

        let mut stream = self.client.create_image(options, None, credentials);

        while let Some(result) = stream.next().await {
            match result {
                Ok(info) => {
                    tracing::debug!(status = ?info.status, "Image pull progress");
                }
                Err(e) => return Err(DockerError::from(e)),
            }
        }

        Ok(())
    }

    /// Remove an image by ID or tag.
    pub async fn remove_image(
        &self,
        image_id: &str,
        force: bool,
        no_prune: bool,
    ) -> Result<(), DockerError> {
        use bollard::query_parameters::RemoveImageOptions;

        let options = Some(RemoveImageOptions {
            force,
            noprune: no_prune,
            ..Default::default()
        });

        self.client
            .remove_image(image_id, options, None)
            .await
            .map_err(DockerError::from)?;

        Ok(())
    }
}
