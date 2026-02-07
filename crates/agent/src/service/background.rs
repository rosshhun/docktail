use std::collections::HashSet;
use std::time::Duration;
use tokio::time::{self, MissedTickBehavior};
use tracing::{info, warn, error};
use crate::state::SharedState;
use crate::docker::inventory::ContainerInfo;
use dashmap::DashMap;

fn perform_mark_and_sweep(inventory: &DashMap<String, ContainerInfo>, containers: Vec<ContainerInfo>) {
    let active_ids: HashSet<String> = containers
        .iter()
        .map(|c| c.id.clone())
        .collect();
    
    for container in containers {
        inventory.insert(container.id.clone(), container);
    }
    
    inventory.retain(|id, _| active_ids.contains(id));
}

/// Background task that synchronizes the container inventory cache
/// 
/// This task runs continuously in the background, fetching fresh container data
/// from Docker at regular intervals and updating the shared cache. This architecture
/// decouples user requests from Docker API calls, providing two key benefits:
/// 
/// 1. **DoS Protection**: No matter how many users refresh the UI, Docker is only
///    called once per interval (e.g., every 2 seconds)
/// 2. **Memory Leak Prevention**: Dead containers are removed from cache using
///    the "Mark and Sweep" pattern
///
/// ## Safety Guarantees
/// 
/// - **No UI Flickering**: Uses UPSERT + RETAIN instead of CLEAR to ensure the cache
///   is never empty during updates
/// - **Timeout Protection**: Docker calls are wrapped in timeouts to prevent hangs
/// - **Graceful Degradation**: On error, the old cache is preserved (stale > empty)
pub async fn background_inventory_sync(state: SharedState, interval_secs: u64) {
    info!("Starting background inventory sync task (interval: {}s)", interval_secs);
    
    let mut interval = time::interval(Duration::from_secs(interval_secs));
    interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
    
    let mut sync_count: u64 = 0;
    let mut consecutive_failures: u32 = 0;
    
    loop {
        interval.tick().await;
        sync_count = sync_count.saturating_add(1);
        
        // Wrap the Docker call in a timeout to prevent hangs
        let list_future = state.docker.list_containers();
        let timeout_duration = Duration::from_secs(5);
        
        match time::timeout(timeout_duration, list_future).await {
            Ok(Ok(containers)) => {
                // Success: Docker returned valid data
                consecutive_failures = 0;
                
                perform_mark_and_sweep(&state.inventory, containers);
                
                let cache_size = state.inventory.len();
                
                // Log periodically (every 30 syncs = ~1 minute at 2s interval)
                if sync_count % 30 == 0 {
                    info!("Inventory sync #{}: {} containers in cache", sync_count, cache_size);
                }
            }
            Ok(Err(e)) => {
                // Docker returned an error
                consecutive_failures = consecutive_failures.saturating_add(1);
                error!("Docker list_containers failed (attempt {}): {}", consecutive_failures, e);
                
                // Keep old cache - stale data is better than no data
                if consecutive_failures >= 3 {
                    warn!("Docker API has failed {} times consecutively - check daemon health", 
                        consecutive_failures);
                }
            }
            Err(_) => {
                // Timeout: Docker is unresponsive
                consecutive_failures = consecutive_failures.saturating_add(1);
                warn!("Docker daemon timeout after {:?} (attempt {})", 
                    timeout_duration, consecutive_failures);
                
                // Keep old cache - stale data is better than no data
                if consecutive_failures >= 3 {
                    error!("Docker daemon appears to be hung - {} consecutive timeouts", 
                        consecutive_failures);
                }
            }
        }
        
        // Health reporting: Update global metrics
        state.metrics.set_docker_failures(consecutive_failures as u64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dashmap::DashMap;
    use std::collections::HashMap;

    fn create_container(id: &str, name: &str) -> ContainerInfo {
        ContainerInfo {
            id: id.to_string(),
            name: name.to_string(),
            image: "nginx:latest".to_string(),
            state: "running".to_string(),
            status: "Up 1 minute".to_string(),
            log_driver: Some("json-file".to_string()),
            labels: HashMap::new(),
            created_at: 1000,
            ports: vec![],
            state_info: None,
        }
    }

    #[test]
    fn test_mark_and_sweep_initial_population() {
        let inventory = DashMap::new();
        let containers = vec![
            create_container("1", "app-1"),
            create_container("2", "app-2"),
        ];

        perform_mark_and_sweep(&inventory, containers);

        assert_eq!(inventory.len(), 2);
        assert!(inventory.contains_key("1"));
        assert!(inventory.contains_key("2"));
    }

    #[test]
    fn test_mark_and_sweep_updates_existing() {
        let inventory = DashMap::new();
        let c1 = create_container("1", "app-1-old");
        inventory.insert(c1.id.clone(), c1);

        let c1_new = create_container("1", "app-1-new"); // Changed name
        let containers = vec![c1_new];

        perform_mark_and_sweep(&inventory, containers);

        assert_eq!(inventory.len(), 1);
        let entry = inventory.get("1").unwrap();
        assert_eq!(entry.name, "app-1-new");
    }

    #[test]
    fn test_mark_and_sweep_removes_dead_containers() {
        let inventory = DashMap::new();
        
        inventory.insert("1".to_string(), create_container("1", "keep-me"));
        inventory.insert("2".to_string(), create_container("2", "remove-me"));
        inventory.insert("3".to_string(), create_container("3", "keep-me-too"));

        let containers = vec![
            create_container("1", "keep-me"),
            create_container("3", "keep-me-too"),
        ];

        perform_mark_and_sweep(&inventory, containers);

        assert_eq!(inventory.len(), 2);
        assert!(inventory.contains_key("1"));
        assert!(!inventory.contains_key("2")); // Should be gone
        assert!(inventory.contains_key("3"));
    }

    #[test]
    fn test_mark_and_sweep_handles_empty_update() {
        let inventory = DashMap::new();
        inventory.insert("1".to_string(), create_container("1", "gone"));

        perform_mark_and_sweep(&inventory, vec![]);

        assert!(inventory.is_empty());
    }

    #[test]
    fn test_mark_and_sweep_no_flicker_empty_to_full() {
        let inventory = DashMap::new();
        
        let containers = vec![create_container("1", "new")];
        perform_mark_and_sweep(&inventory, containers);

        assert_eq!(inventory.len(), 1);
        assert!(inventory.contains_key("1"));
    }
}
