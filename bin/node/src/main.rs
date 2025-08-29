use clap::Parser;
use miden_private_transport_node::{
    Node, NodeConfig, Result,
    database::DatabaseConfig,
    logging::{TracingConfig, setup_tracing},
    node::grpc::GrpcServerConfig,
};
use tracing::info;

#[derive(Parser)]
#[command(name = "miden-private-transport-node")]
#[command(about = "Miden Transport Node - Canonical transport layer for private notes")]
struct Args {
    /// Host to bind to
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// Port to bind to
    #[arg(long, default_value = "8080")]
    port: u16,

    /// Database URL
    #[arg(long, default_value = "sqlite::memory:")]
    database_url: String,

    /// Maximum note size in bytes
    #[arg(long, default_value = "1048576")]
    max_note_size: usize,

    /// Retention period in days
    #[arg(long, default_value = "30")]
    retention_days: u32,

    /// Rate limit per minute
    #[arg(long, default_value = "100")]
    rate_limit_per_minute: u32,

    /// Request timeout in seconds
    #[arg(long, default_value = "30")]
    request_timeout_seconds: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse command line arguments
    let args = Args::parse();

    // Setup tracing
    let tracing_cfg = TracingConfig::from_env();
    setup_tracing(tracing_cfg.clone())?;

    info!("Starting Miden Transport Node...");
    info!("Host: {}", args.host);
    info!("Port: {}", args.port);
    info!("Database: {}", args.database_url);
    info!("Max note size: {} bytes", args.max_note_size);
    info!("Retention days: {}", args.retention_days);
    info!("Rate limit: {} requests/minute", args.rate_limit_per_minute);
    info!("Request timeout: {} seconds", args.request_timeout_seconds);
    info!(
        "Telemetry: OpenTelemetry={}, JSON={}",
        tracing_cfg.otel.is_enabled(),
        tracing_cfg.json_format
    );

    // Create Node config
    let config = NodeConfig {
        grpc: GrpcServerConfig {
            host: args.host,
            port: args.port,
            max_note_size: args.max_note_size,
        },
        database: DatabaseConfig {
            url: args.database_url,
            retention_days: args.retention_days,
            rate_limit_per_minute: args.rate_limit_per_minute,
            request_timeout_seconds: args.request_timeout_seconds,
            max_note_size: args.max_note_size,
        },
    };

    // Run Node
    let node = Node::init(config).await?;
    node.entrypoint().await;

    Ok(())
}
