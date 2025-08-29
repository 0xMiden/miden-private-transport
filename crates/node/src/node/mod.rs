use std::sync::Arc;

use tracing::{error, info};

use self::grpc::{GrpcServer, GrpcServerConfig};
use crate::{
    Result,
    database::{Database, DatabaseConfig, DatabaseMaintenance},
    metrics::Metrics,
};

pub mod grpc;

/// Miden Private Transport Node
pub struct Node {
    /// Serve client requests
    grpc: GrpcServer,
    /// Database maintenance
    maintenance: DatabaseMaintenance,
    /// Metrics
    _metrics: Metrics,

    // To be used in other services, .e.g. P2P
    _database: Arc<Database>,
}

#[derive(Debug, Default, Clone)]
pub struct NodeConfig {
    pub grpc: GrpcServerConfig,
    pub database: DatabaseConfig,
}

impl Node {
    pub async fn init(config: NodeConfig) -> Result<Self> {
        let metrics = Metrics::default();
        let database =
            Arc::new(Database::connect(config.database.clone(), metrics.db.clone()).await?);

        let grpc = GrpcServer::new(database.clone(), config.grpc, metrics.grpc.clone());
        let maintenance =
            DatabaseMaintenance::new(database.clone(), config.database, metrics.db.clone());

        Ok(Self {
            grpc,
            maintenance,
            _metrics: metrics,
            _database: database,
        })
    }

    pub async fn entrypoint(self) {
        info!("Starting Miden Transport Node");
        tokio::spawn(self.maintenance.entrypoint());

        if let Err(e) = self.grpc.serve().await {
            error!("Server error: {e}");
        }
    }
}
