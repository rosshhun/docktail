use async_graphql::SimpleObject;

/// Container resource statistics
#[derive(Debug, Clone, SimpleObject)]
pub struct ContainerStats {
    /// Container ID
    pub container_id: String,
    
    /// Timestamp when stats were collected (Unix timestamp)
    pub timestamp: i64,
    
    /// CPU statistics
    pub cpu_stats: CpuStats,
    
    /// Memory statistics
    pub memory_stats: MemoryStats,
    
    /// Network statistics (one per interface)
    pub network_stats: Vec<NetworkStats>,
    
    /// Block I/O statistics
    pub block_io_stats: BlockIoStats,
    
    /// Number of PIDs/processes
    pub pids_count: Option<i64>,
}

/// CPU usage statistics
#[derive(Debug, Clone, SimpleObject)]
pub struct CpuStats {
    /// CPU usage percentage (0-100% per core, can exceed 100% on multi-core)
    pub cpu_percentage: f64,
    
    /// Total CPU time consumed (nanoseconds)
    pub total_usage: i64,
    
    /// CPU time in kernel mode (nanoseconds)
    pub system_usage: i64,
    
    /// Number of CPU cores available to container
    pub online_cpus: i32,
    
    /// Per-CPU usage breakdown (nanoseconds)
    pub per_cpu_usage: Vec<i64>,
    
    /// CPU throttling statistics
    pub throttling: Option<CpuThrottlingStats>,
}

/// CPU throttling statistics
#[derive(Debug, Clone, SimpleObject)]
pub struct CpuThrottlingStats {
    /// Number of periods with throttling active
    pub throttled_periods: i64,
    
    /// Total number of periods
    pub total_periods: i64,
    
    /// Total time throttled (nanoseconds)
    pub throttled_time: i64,
}

/// Memory usage statistics
#[derive(Debug, Clone, SimpleObject)]
pub struct MemoryStats {
    /// Current memory usage (bytes)
    pub usage: i64,
    
    /// Maximum memory usage recorded (bytes)
    pub max_usage: i64,
    
    /// Memory limit (bytes, 0 = unlimited)
    pub limit: i64,
    
    /// Memory usage percentage (0-100)
    pub percentage: f64,
    
    /// Cache memory (bytes)
    pub cache: i64,
    
    /// RSS memory (bytes) - actual physical memory used
    pub rss: i64,
    
    /// Swap usage (bytes)
    pub swap: Option<i64>,
}

/// Network interface statistics
#[derive(Debug, Clone, SimpleObject)]
pub struct NetworkStats {
    /// Network interface name
    pub interface_name: String,
    
    /// Bytes received
    pub rx_bytes: i64,
    
    /// Packets received
    pub rx_packets: i64,
    
    /// Receive errors
    pub rx_errors: i64,
    
    /// Receive dropped packets
    pub rx_dropped: i64,
    
    /// Bytes transmitted
    pub tx_bytes: i64,
    
    /// Packets transmitted
    pub tx_packets: i64,
    
    /// Transmit errors
    pub tx_errors: i64,
    
    /// Transmit dropped packets
    pub tx_dropped: i64,
}

/// Block I/O statistics
#[derive(Debug, Clone, SimpleObject)]
pub struct BlockIoStats {
    /// Total bytes read from disk
    pub read_bytes: i64,
    
    /// Total bytes written to disk
    pub write_bytes: i64,
    
    /// Total read operations
    pub read_ops: i64,
    
    /// Total write operations
    pub write_ops: i64,
    
    /// Per-device statistics
    pub devices: Vec<BlockIoDeviceStats>,
}

/// Per-device block I/O statistics
#[derive(Debug, Clone, SimpleObject)]
pub struct BlockIoDeviceStats {
    /// Device major number
    pub major: i64,
    
    /// Device minor number
    pub minor: i64,
    
    /// Bytes read from this device
    pub read_bytes: i64,
    
    /// Bytes written to this device
    pub write_bytes: i64,
}

// ============================================================================
// Shared conversion from proto ContainerStatsResponse → GraphQL ContainerStats
// ============================================================================

impl ContainerStats {
    /// Convert a proto ContainerStatsResponse into a GraphQL ContainerStats.
    /// This eliminates the ~60-line duplication across schema.rs and subscriptions/mod.rs.
    pub fn from_proto(response: crate::agent::client::ContainerStatsResponse) -> Self {
        Self {
            container_id: response.container_id,
            timestamp: response.timestamp,
            cpu_stats: CpuStats {
                cpu_percentage: response.cpu_stats.as_ref().map(|c| c.cpu_percentage).unwrap_or(0.0),
                total_usage: response.cpu_stats.as_ref().map(|c| c.total_usage as i64).unwrap_or(0),
                system_usage: response.cpu_stats.as_ref().map(|c| c.system_usage as i64).unwrap_or(0),
                online_cpus: response.cpu_stats.as_ref().map(|c| c.online_cpus as i32).unwrap_or(0),
                per_cpu_usage: response.cpu_stats.as_ref()
                    .map(|c| c.per_cpu_usage.iter().map(|&v| v as i64).collect())
                    .unwrap_or_default(),
                throttling: response.cpu_stats.as_ref()
                    .and_then(|c| c.throttling.as_ref())
                    .map(|t| CpuThrottlingStats {
                        throttled_periods: t.throttled_periods as i64,
                        total_periods: t.total_periods as i64,
                        throttled_time: t.throttled_time as i64,
                    }),
            },
            memory_stats: MemoryStats {
                usage: response.memory_stats.as_ref().map(|m| m.usage as i64).unwrap_or(0),
                max_usage: response.memory_stats.as_ref().map(|m| m.max_usage as i64).unwrap_or(0),
                limit: response.memory_stats.as_ref().map(|m| m.limit as i64).unwrap_or(0),
                percentage: response.memory_stats.as_ref().map(|m| m.percentage).unwrap_or(0.0),
                cache: response.memory_stats.as_ref().map(|m| m.cache as i64).unwrap_or(0),
                rss: response.memory_stats.as_ref().map(|m| m.rss as i64).unwrap_or(0),
                swap: response.memory_stats.as_ref().and_then(|m| m.swap).map(|s| s as i64),
            },
            network_stats: response.network_stats.iter().map(|n| NetworkStats {
                interface_name: n.interface_name.clone(),
                rx_bytes: n.rx_bytes as i64,
                rx_packets: n.rx_packets as i64,
                rx_errors: n.rx_errors as i64,
                rx_dropped: n.rx_dropped as i64,
                tx_bytes: n.tx_bytes as i64,
                tx_packets: n.tx_packets as i64,
                tx_errors: n.tx_errors as i64,
                tx_dropped: n.tx_dropped as i64,
            }).collect(),
            block_io_stats: BlockIoStats {
                read_bytes: response.block_io_stats.as_ref().map(|b| b.read_bytes as i64).unwrap_or(0),
                write_bytes: response.block_io_stats.as_ref().map(|b| b.write_bytes as i64).unwrap_or(0),
                read_ops: response.block_io_stats.as_ref().map(|b| b.read_ops as i64).unwrap_or(0),
                write_ops: response.block_io_stats.as_ref().map(|b| b.write_ops as i64).unwrap_or(0),
                devices: response.block_io_stats.as_ref()
                    .map(|b| b.devices.iter().map(|d| BlockIoDeviceStats {
                        major: d.major as i64,
                        minor: d.minor as i64,
                        read_bytes: d.read_bytes as i64,
                        write_bytes: d.write_bytes as i64,
                    }).collect())
                    .unwrap_or_default(),
            },
            pids_count: response.pids_count.map(|p| p as i64),
        }
    }
}
