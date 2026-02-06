use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use parking_lot::RwLock;
use std::collections::HashMap;

/// Subscription metrics tracker
#[derive(Clone)]
pub struct SubscriptionMetrics {
    inner: Arc<SubscriptionMetricsInner>,
}

struct SubscriptionMetricsInner {
    /// Total number of active subscriptions
    active_subscriptions: AtomicU64,
    
    /// Total subscriptions created (lifetime)
    total_subscriptions_created: AtomicU64,
    
    /// Total messages sent across all subscriptions
    total_messages_sent: AtomicU64,
    
    /// Total bytes sent across all subscriptions
    total_bytes_sent: AtomicU64,
    
    /// Active subscriptions per agent (agent_id -> count)
    subscriptions_per_agent: RwLock<HashMap<String, u64>>,
    
    /// Total failed subscription attempts
    failed_subscriptions: AtomicU64,
}

impl SubscriptionMetrics {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(SubscriptionMetricsInner {
                active_subscriptions: AtomicU64::new(0),
                total_subscriptions_created: AtomicU64::new(0),
                total_messages_sent: AtomicU64::new(0),
                total_bytes_sent: AtomicU64::new(0),
                subscriptions_per_agent: RwLock::new(HashMap::new()),
                failed_subscriptions: AtomicU64::new(0),
            }),
        }
    }
    
    /// Called when a new subscription is created
    pub fn subscription_started(&self, agent_id: &str) {
        self.inner.active_subscriptions.fetch_add(1, Ordering::Relaxed);
        self.inner.total_subscriptions_created.fetch_add(1, Ordering::Relaxed);
        
        let mut per_agent = self.inner.subscriptions_per_agent.write();
        *per_agent.entry(agent_id.to_string()).or_insert(0) += 1;
        
        tracing::debug!(
            agent_id = agent_id,
            active = self.inner.active_subscriptions.load(Ordering::Relaxed),
            "Subscription started"
        );
    }
    
    /// Called when a subscription ends
    pub fn subscription_ended(&self, agent_id: &str) {
        // Use fetch_update for atomic check-and-decrement to prevent underflow.
        // The previous load-then-sub pattern was not atomic and could wrap to u64::MAX
        // under concurrent subscription_started/subscription_ended calls.
        let _ = self.inner.active_subscriptions.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |current| if current > 0 { Some(current - 1) } else { None },
        );
        
        let mut per_agent = self.inner.subscriptions_per_agent.write();
        
        // Use Entry API for efficient garbage collection
        if let std::collections::hash_map::Entry::Occupied(mut entry) = per_agent.entry(agent_id.to_string()) {
            let count = entry.get_mut();
            if *count > 0 {
                *count -= 1;
            }
            // Garbage collection: Remove key if 0 to prevent memory leak over time
            if *count == 0 {
                entry.remove();
            }
        }
        
        tracing::debug!(
            agent_id = agent_id,
            active = self.inner.active_subscriptions.load(Ordering::Relaxed),
            "Subscription ended"
        );
    }
    
    /// Called when a subscription fails to start
    pub fn subscription_failed(&self) {
        self.inner.failed_subscriptions.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Called when a message is sent to a subscriber
    pub fn message_sent(&self, bytes: usize) {
        self.inner.total_messages_sent.fetch_add(1, Ordering::Relaxed);
        self.inner.total_bytes_sent.fetch_add(bytes as u64, Ordering::Relaxed);
    }
    
    /// Get current active subscription count
    pub fn active_count(&self) -> u64 {
        self.inner.active_subscriptions.load(Ordering::Relaxed)
    }
    
    /// Get total subscriptions created (lifetime)
    pub fn total_created(&self) -> u64 {
        self.inner.total_subscriptions_created.load(Ordering::Relaxed)
    }
    
    /// Get total messages sent
    pub fn total_messages(&self) -> u64 {
        self.inner.total_messages_sent.load(Ordering::Relaxed)
    }
    
    /// Get total bytes sent
    pub fn total_bytes(&self) -> u64 {
        self.inner.total_bytes_sent.load(Ordering::Relaxed)
    }
    
    /// Get failed subscription count
    pub fn failed_count(&self) -> u64 {
        self.inner.failed_subscriptions.load(Ordering::Relaxed)
    }
    
    /// Get subscriptions per agent
    pub fn subscriptions_by_agent(&self) -> HashMap<String, u64> {
        self.inner.subscriptions_per_agent.read().clone()
    }
    
    /// Print current metrics summary
    #[allow(dead_code)]
    pub fn print_summary(&self) {
        let active = self.active_count();
        let total = self.total_created();
        let messages = self.total_messages();
        let bytes = self.total_bytes();
        let failed = self.failed_count();
        let per_agent = self.subscriptions_by_agent();
        
        tracing::info!(
            active_subscriptions = active,
            total_created = total,
            total_messages = messages,
            total_bytes_mb = bytes / 1024 / 1024,
            failed_subscriptions = failed,
            "Subscription metrics summary"
        );
        
        for (agent_id, count) in per_agent {
            if count > 0 {
                tracing::info!(
                    agent_id = agent_id,
                    active_subscriptions = count,
                    "Agent subscription count"
                );
            }
        }
    }
}

impl Default for SubscriptionMetrics {
    fn default() -> Self {
        Self::new()
    }
}
