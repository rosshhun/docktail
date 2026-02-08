pub mod logs;
pub mod inventory;
pub mod health;
pub mod stats;
pub mod multiline;
pub mod background;
pub mod control;
pub mod swarm;
pub mod shell;

pub mod proto {
    tonic::include_proto!("docktail.agent");
}

pub use proto::{
    log_service_server::LogServiceServer,
    inventory_service_server::InventoryServiceServer,
    health_service_server::HealthServiceServer,
    stats_service_server::StatsServiceServer,
    control_service_server::ControlServiceServer,
    swarm_service_server::SwarmServiceServer,
    shell_service_server::ShellServiceServer,
};

pub use logs::LogServiceImpl;
pub use inventory::InventoryServiceImpl;
pub use health::HealthServiceImpl;
pub use stats::StatsServiceImpl;
pub use control::ControlServiceImpl;
pub use swarm::SwarmServiceImpl;
pub use shell::ShellServiceImpl;
