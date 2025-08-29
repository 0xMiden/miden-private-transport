use std::time::Duration;

use miden_objects::account::AccountId;
use miden_private_transport_client::{
    TransportLayerClient, database::DatabaseConfig, grpc::GrpcClient, types::mock_account_id,
};
use miden_private_transport_node::{Node, NodeConfig, node::grpc::GrpcServerConfig};
use tokio::{task::JoinHandle, time::sleep};

pub const TAG_LOCALANY: u32 = 0xc000_0000;

pub async fn spawn_test_server(port: u16) -> JoinHandle<()> {
    let config = NodeConfig {
        grpc: GrpcServerConfig { port, ..Default::default() },
        ..Default::default()
    };

    let server = Node::init(config).await.unwrap();
    let handle = tokio::spawn(server.entrypoint());
    // Wait for startup
    sleep(Duration::from_millis(100)).await;
    handle
}

pub async fn test_client(port: u16) -> (TransportLayerClient, AccountId) {
    let timeout_ms = 1000;
    let url = format!("http://127.0.0.1:{port}");

    let grpc_client = Box::new(GrpcClient::connect(url, timeout_ms).await.unwrap());

    let account_id = mock_account_id();

    let db_config = DatabaseConfig::default();
    let client = TransportLayerClient::init(grpc_client, vec![account_id], Some(db_config))
        .await
        .unwrap();

    (client, account_id)
}
