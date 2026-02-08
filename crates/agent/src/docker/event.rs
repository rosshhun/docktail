//! Event domain — Docker engine event streaming.

use super::client::{DockerClient, DockerError};
use futures_util::stream::StreamExt;

impl DockerClient {
    /// Stream Docker engine events.
    pub fn stream_events(
        &self,
        type_filters: Vec<String>,
        since: Option<i64>,
        until: Option<i64>,
    ) -> impl futures_util::Stream<Item = Result<bollard::models::EventMessage, DockerError>> + '_
    {
        use bollard::query_parameters::EventsOptionsBuilder;
        use std::collections::HashMap;

        let mut filters = HashMap::new();
        if !type_filters.is_empty() {
            filters.insert(
                "type",
                type_filters
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>(),
            );
        }

        let since_str = since.map(|s| s.to_string());
        let until_str = until.map(|u| u.to_string());

        let mut builder = EventsOptionsBuilder::default();
        builder = builder.filters(&filters);
        if let Some(ref s) = since_str {
            builder = builder.since(s);
        }
        if let Some(ref u) = until_str {
            builder = builder.until(u);
        }
        let options = builder.build();

        self.client
            .events(Some(options))
            .map(|r| r.map_err(DockerError::from))
    }
}
