//! Map — pure conversion helpers for container stats.

use std::collections::HashMap;
use tonic::Status;

use crate::proto::{
    ContainerStatsResponse, CpuStats, MemoryStats, NetworkStats, BlockIoStats,
    BlockIoDeviceStats, CpuThrottlingStats,
};

/// Classify a Docker/bollard error into an appropriate gRPC status.
pub fn classify_docker_error(container_id: &str, e: crate::docker::client::DockerError) -> Status {
    crate::docker::error_map::map_docker_error_with_context(
        &format!("stats for container {}", container_id),
        e,
    )
}

/// Convert bollard ContainerStatsResponse to protobuf ContainerStatsResponse
pub fn convert_stats(container_id: &str, stats: bollard::models::ContainerStatsResponse) -> ContainerStatsResponse {
    let timestamp = stats.read
        .as_deref()
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| chrono::Utc::now().timestamp());

    let cpu_percentage = calculate_cpu_percentage(&stats);

    let cpu_stats = if let Some(ref cpu_stats_data) = stats.cpu_stats {
        let total_usage = cpu_stats_data.cpu_usage
            .as_ref()
            .and_then(|u| u.total_usage)
            .unwrap_or(0);
        
        let system_usage = cpu_stats_data.system_cpu_usage.unwrap_or(0);
        let online_cpus = cpu_stats_data.online_cpus.unwrap_or(1) as u32;
        
        let per_cpu_usage = cpu_stats_data.cpu_usage
            .as_ref()
            .and_then(|u| u.percpu_usage.clone())
            .unwrap_or_default();
        
        let throttling = cpu_stats_data.throttling_data.as_ref().map(|t| CpuThrottlingStats {
            throttled_periods: t.throttled_periods.unwrap_or(0),
            total_periods: t.periods.unwrap_or(0),
            throttled_time: t.throttled_time.unwrap_or(0),
        });

        CpuStats {
            cpu_percentage,
            total_usage,
            system_usage,
            online_cpus,
            per_cpu_usage,
            throttling,
        }
    } else {
        CpuStats {
            cpu_percentage: 0.0,
            total_usage: 0,
            system_usage: 0,
            online_cpus: 0,
            per_cpu_usage: vec![],
            throttling: None,
        }
    };

    let memory_stats = if let Some(ref mem_stats) = stats.memory_stats {
        let memory_usage = mem_stats.usage.unwrap_or(0);
        let memory_limit = mem_stats.limit.unwrap_or(0);
        let memory_percentage = if memory_limit > 0 {
            (memory_usage as f64 / memory_limit as f64) * 100.0
        } else {
            0.0
        };

        let cache = mem_stats.stats
            .as_ref()
            .and_then(|s| s.get("cache"))
            .copied()
            .unwrap_or(0);

        let rss = mem_stats.stats
            .as_ref()
            .and_then(|s| s.get("rss"))
            .copied()
            .unwrap_or(0);

        let swap = mem_stats.stats
            .as_ref()
            .and_then(|s| s.get("swap"))
            .copied();

        MemoryStats {
            usage: memory_usage,
            max_usage: mem_stats.max_usage.unwrap_or(0),
            limit: memory_limit,
            percentage: memory_percentage,
            cache,
            rss,
            swap,
        }
    } else {
        MemoryStats {
            usage: 0,
            max_usage: 0,
            limit: 0,
            percentage: 0.0,
            cache: 0,
            rss: 0,
            swap: None,
        }
    };

    let network_stats = stats.networks.map(|networks| {
        networks.into_iter().map(|(interface_name, net)| NetworkStats {
            interface_name,
            rx_bytes: net.rx_bytes.unwrap_or(0),
            rx_packets: net.rx_packets.unwrap_or(0),
            rx_errors: net.rx_errors.unwrap_or(0),
            rx_dropped: net.rx_dropped.unwrap_or(0),
            tx_bytes: net.tx_bytes.unwrap_or(0),
            tx_packets: net.tx_packets.unwrap_or(0),
            tx_errors: net.tx_errors.unwrap_or(0),
            tx_dropped: net.tx_dropped.unwrap_or(0),
        }).collect()
    }).unwrap_or_default();

    let block_io_stats = if let Some(ref blkio_stats) = stats.blkio_stats {
        let mut read_bytes = 0u64;
        let mut write_bytes = 0u64;
        let mut read_ops = 0u64;
        let mut write_ops = 0u64;
        let mut device_map: HashMap<(u64, u64), (u64, u64)> = HashMap::new();

        if let Some(ref io_stats) = blkio_stats.io_service_bytes_recursive {
            for entry in io_stats {
                let value = entry.value.unwrap_or(0);
                let major = entry.major.unwrap_or(0);
                let minor = entry.minor.unwrap_or(0);
                let op = entry.op.as_deref().unwrap_or("");

                match op {
                    "Read" | "read" => {
                        read_bytes += value;
                        device_map.entry((major, minor)).or_default().0 += value;
                    }
                    "Write" | "write" => {
                        write_bytes += value;
                        device_map.entry((major, minor)).or_default().1 += value;
                    }
                    _ => {}
                }
            }
        }

        if let Some(ref io_ops) = blkio_stats.io_serviced_recursive {
            for entry in io_ops {
                let value = entry.value.unwrap_or(0);
                match entry.op.as_deref() {
                    Some("Read") | Some("read") => read_ops += value,
                    Some("Write") | Some("write") => write_ops += value,
                    _ => {}
                }
            }
        }

        let devices: Vec<BlockIoDeviceStats> = device_map
            .into_iter()
            .map(|((major, minor), (dev_read, dev_write))| BlockIoDeviceStats {
                major,
                minor,
                read_bytes: dev_read,
                write_bytes: dev_write,
            })
            .collect();

        BlockIoStats {
            read_bytes,
            write_bytes,
            read_ops,
            write_ops,
            devices,
        }
    } else {
        BlockIoStats {
            read_bytes: 0,
            write_bytes: 0,
            read_ops: 0,
            write_ops: 0,
            devices: vec![],
        }
    };

    let pids_count = stats.pids_stats
        .and_then(|p| p.current);

    ContainerStatsResponse {
        container_id: container_id.to_string(),
        timestamp,
        cpu_stats: Some(cpu_stats),
        memory_stats: Some(memory_stats),
        network_stats,
        block_io_stats: Some(block_io_stats),
        pids_count,
    }
}

/// Calculate CPU percentage from Docker stats
/// Formula: ((total_usage_delta / system_usage_delta) * num_cpus) * 100
pub fn calculate_cpu_percentage(stats: &bollard::models::ContainerStatsResponse) -> f64 {
    let cpu_stats = match &stats.cpu_stats {
        Some(cpu) => cpu,
        None => return 0.0,
    };

    let precpu_stats = match &stats.precpu_stats {
        Some(precpu) => precpu,
        None => return 0.0,
    };

    let cpu_total = cpu_stats.cpu_usage
        .as_ref()
        .and_then(|u| u.total_usage)
        .unwrap_or(0);
    let precpu_total = precpu_stats.cpu_usage
        .as_ref()
        .and_then(|u| u.total_usage)
        .unwrap_or(0);

    let cpu_delta = cpu_total.saturating_sub(precpu_total) as f64;

    let sys_current = cpu_stats.system_cpu_usage.unwrap_or(0);
    let sys_previous = precpu_stats.system_cpu_usage.unwrap_or(0);
    let system_delta = sys_current.saturating_sub(sys_previous) as f64;

    if system_delta > 0.0 && cpu_delta > 0.0 {
        let num_cpus = cpu_stats.online_cpus.unwrap_or(1).max(1) as f64;
        let pct = (cpu_delta / system_delta) * num_cpus * 100.0;
        if pct.is_finite() { pct } else { 0.0 }
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bollard::models::{
        ContainerStatsResponse as BollardStatsResponse,
        ContainerCpuStats, ContainerCpuUsage, ContainerMemoryStats,
        ContainerNetworkStats as BollardNetworkStats, ContainerBlkioStats,
        ContainerBlkioStatEntry, ContainerPidsStats, ContainerThrottlingData,
    };
    use std::collections::HashMap as StdHashMap;

    fn empty_bollard_stats() -> BollardStatsResponse {
        BollardStatsResponse::default()
    }

    fn bollard_stats_with_cpu(
        cpu_total: u64,
        precpu_total: u64,
        sys_total: u64,
        presys_total: u64,
        online_cpus: u32,
    ) -> BollardStatsResponse {
        BollardStatsResponse {
            cpu_stats: Some(ContainerCpuStats {
                cpu_usage: Some(ContainerCpuUsage {
                    total_usage: Some(cpu_total),
                    percpu_usage: Some(vec![cpu_total]),
                    usage_in_kernelmode: None,
                    usage_in_usermode: None,
                }),
                system_cpu_usage: Some(sys_total),
                online_cpus: Some(online_cpus),
                throttling_data: None,
            }),
            precpu_stats: Some(ContainerCpuStats {
                cpu_usage: Some(ContainerCpuUsage {
                    total_usage: Some(precpu_total),
                    percpu_usage: None,
                    usage_in_kernelmode: None,
                    usage_in_usermode: None,
                }),
                system_cpu_usage: Some(presys_total),
                online_cpus: Some(online_cpus),
                throttling_data: None,
            }),
            ..Default::default()
        }
    }

    #[test]
    fn cpu_percentage_normal_case() {
        let stats = bollard_stats_with_cpu(200_000_000, 100_000_000, 2_000_000_000, 1_800_000_000, 1);
        let pct = calculate_cpu_percentage(&stats);
        assert!((pct - 50.0).abs() < 0.01, "Expected ~50%, got {}", pct);
    }

    #[test]
    fn cpu_percentage_multi_core() {
        let stats = bollard_stats_with_cpu(200_000_000, 100_000_000, 2_000_000_000, 1_800_000_000, 4);
        let pct = calculate_cpu_percentage(&stats);
        assert!((pct - 200.0).abs() < 0.01, "Expected ~200%, got {}", pct);
    }

    #[test]
    fn cpu_percentage_no_cpu_stats() {
        let stats = empty_bollard_stats();
        assert_eq!(calculate_cpu_percentage(&stats), 0.0);
    }

    #[test]
    fn cpu_percentage_no_precpu_stats() {
        let mut stats = bollard_stats_with_cpu(100, 0, 100, 0, 1);
        stats.precpu_stats = None;
        assert_eq!(calculate_cpu_percentage(&stats), 0.0);
    }

    #[test]
    fn cpu_percentage_counter_reset() {
        let stats = bollard_stats_with_cpu(50_000_000, 100_000_000, 2_000_000_000, 1_800_000_000, 1);
        let pct = calculate_cpu_percentage(&stats);
        assert_eq!(pct, 0.0, "Counter reset should yield 0%");
    }

    #[test]
    fn cpu_percentage_zero_system_delta() {
        let stats = bollard_stats_with_cpu(200, 100, 1000, 1000, 1);
        assert_eq!(calculate_cpu_percentage(&stats), 0.0);
    }

    #[test]
    fn cpu_percentage_zero_online_cpus() {
        let mut stats = bollard_stats_with_cpu(200, 100, 2000, 1800, 1);
        stats.cpu_stats.as_mut().unwrap().online_cpus = Some(0);
        let pct = calculate_cpu_percentage(&stats);
        assert!((pct - 50.0).abs() < 0.01);
    }

    #[test]
    fn cpu_percentage_one_shot_mode_identical_stats() {
        let stats = bollard_stats_with_cpu(100_000_000, 100_000_000, 2_000_000_000, 2_000_000_000, 1);
        let pct = calculate_cpu_percentage(&stats);
        assert_eq!(pct, 0.0, "Identical stats should yield 0%");
    }

    #[test]
    fn convert_stats_all_none() {
        let stats = empty_bollard_stats();
        let result = convert_stats("test-container", stats);
        assert_eq!(result.container_id, "test-container");
        assert!(result.cpu_stats.is_some());
        assert!(result.memory_stats.is_some());
        assert!(result.block_io_stats.is_some());
        assert!(result.network_stats.is_empty());
        assert!(result.pids_count.is_none());
        let cpu = result.cpu_stats.unwrap();
        assert_eq!(cpu.cpu_percentage, 0.0);
    }

    #[test]
    fn convert_stats_with_memory() {
        let stats = BollardStatsResponse {
            memory_stats: Some(ContainerMemoryStats {
                usage: Some(1024 * 1024 * 100),
                max_usage: Some(1024 * 1024 * 200),
                limit: Some(1024 * 1024 * 512),
                stats: Some({
                    let mut m = StdHashMap::new();
                    m.insert("cache".to_string(), 1024 * 1024 * 10);
                    m.insert("rss".to_string(), 1024 * 1024 * 90);
                    m.insert("swap".to_string(), 0);
                    m
                }),
                failcnt: None,
                commitbytes: None,
                commitpeakbytes: None,
                privateworkingset: None,
            }),
            ..Default::default()
        };
        let result = convert_stats("mem-test", stats);
        let mem = result.memory_stats.unwrap();
        assert_eq!(mem.usage, 1024 * 1024 * 100);
        assert!((mem.percentage - 19.53125).abs() < 0.01);
    }

    #[test]
    fn convert_stats_memory_zero_limit() {
        let stats = BollardStatsResponse {
            memory_stats: Some(ContainerMemoryStats {
                usage: Some(1024),
                limit: Some(0),
                ..Default::default()
            }),
            ..Default::default()
        };
        let result = convert_stats("zero-limit", stats);
        let mem = result.memory_stats.unwrap();
        assert_eq!(mem.percentage, 0.0, "Zero limit should yield 0% not NaN");
    }

    #[test]
    fn convert_stats_with_networks() {
        let mut networks = StdHashMap::new();
        networks.insert("eth0".to_string(), BollardNetworkStats {
            rx_bytes: Some(1000), rx_packets: Some(10), rx_errors: Some(0), rx_dropped: Some(0),
            tx_bytes: Some(2000), tx_packets: Some(20), tx_errors: Some(1), tx_dropped: Some(0),
            endpoint_id: None, instance_id: None,
        });
        networks.insert("lo".to_string(), BollardNetworkStats::default());
        let stats = BollardStatsResponse { networks: Some(networks), ..Default::default() };
        let result = convert_stats("net-test", stats);
        assert_eq!(result.network_stats.len(), 2);
    }

    #[test]
    fn convert_stats_block_io_dedup() {
        let stats = BollardStatsResponse {
            blkio_stats: Some(ContainerBlkioStats {
                io_service_bytes_recursive: Some(vec![
                    ContainerBlkioStatEntry { major: Some(8), minor: Some(0), op: Some("Read".to_string()), value: Some(1000) },
                    ContainerBlkioStatEntry { major: Some(8), minor: Some(0), op: Some("Write".to_string()), value: Some(2000) },
                    ContainerBlkioStatEntry { major: Some(8), minor: Some(16), op: Some("read".to_string()), value: Some(500) },
                ]),
                io_serviced_recursive: Some(vec![
                    ContainerBlkioStatEntry { major: Some(8), minor: Some(0), op: Some("Read".to_string()), value: Some(10) },
                    ContainerBlkioStatEntry { major: Some(8), minor: Some(0), op: Some("write".to_string()), value: Some(20) },
                ]),
                io_queue_recursive: None, io_service_time_recursive: None,
                io_wait_time_recursive: None, io_merged_recursive: None,
                io_time_recursive: None, sectors_recursive: None,
            }),
            ..Default::default()
        };
        let result = convert_stats("blkio-test", stats);
        let bio = result.block_io_stats.unwrap();
        assert_eq!(bio.read_bytes, 1500);
        assert_eq!(bio.write_bytes, 2000);
        assert_eq!(bio.devices.len(), 2, "Duplicate devices should be merged");
    }

    #[test]
    fn convert_stats_cpu_throttling() {
        let stats = BollardStatsResponse {
            cpu_stats: Some(ContainerCpuStats {
                cpu_usage: Some(ContainerCpuUsage::default()),
                system_cpu_usage: None,
                online_cpus: Some(2),
                throttling_data: Some(ContainerThrottlingData {
                    periods: Some(100), throttled_periods: Some(5), throttled_time: Some(1_000_000),
                }),
            }),
            ..Default::default()
        };
        let result = convert_stats("throttle", stats);
        let cpu = result.cpu_stats.unwrap();
        let throttle = cpu.throttling.unwrap();
        assert_eq!(throttle.total_periods, 100);
        assert_eq!(throttle.throttled_periods, 5);
    }

    #[test]
    fn convert_stats_timestamp_from_docker() {
        let stats = BollardStatsResponse { read: Some("2025-06-15T10:30:00.000000000Z".to_string()), ..Default::default() };
        let result = convert_stats("ts-test", stats);
        let expected = chrono::DateTime::parse_from_rfc3339("2025-06-15T10:30:00Z").unwrap().timestamp();
        assert_eq!(result.timestamp, expected);
    }

    #[test]
    fn convert_stats_pids() {
        let stats = BollardStatsResponse { pids_stats: Some(ContainerPidsStats { current: Some(42), limit: Some(1000) }), ..Default::default() };
        let result = convert_stats("pids-test", stats);
        assert_eq!(result.pids_count, Some(42));
    }
}
