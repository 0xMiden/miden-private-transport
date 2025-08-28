use std::time::Duration;

use miden_objects::account::AccountId;
use miden_private_transport_client::{
    EncryptionStore, FilesystemEncryptionStore, TransportLayerClient, crypto::SerializableKey,
    database::ClientDatabaseConfig, grpc::GrpcClient, types::mock_account_id,
};
use miden_private_transport_node::{Node, NodeConfig, node::grpc::GrpcServerConfig};
use rand::Rng;
use tokio::{task::JoinHandle, time::sleep};

#[allow(dead_code)]
pub const TAG_LOCALANY: u32 = 0xc000_0000;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub enum EncryptionScheme {
    Aes,
    X25519,
}

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

pub async fn test_client(
    port: u16,
    scheme: EncryptionScheme,
) -> (TransportLayerClient, AccountId, SerializableKey) {
    let timeout_ms = 1000;
    let url = format!("http://127.0.0.1:{port}");

    let grpc_client = Box::new(GrpcClient::connect(url, timeout_ms).await.unwrap());
    let mut rng = rand::rng();
    let num: u32 = rng.random();
    let encryption_store =
        Box::new(FilesystemEncryptionStore::new(format!("/tmp/mptl-keystore-{num:08x}")).unwrap());

    let key = match scheme {
        EncryptionScheme::Aes => SerializableKey::generate_aes(),
        EncryptionScheme::X25519 => SerializableKey::generate_x25519(),
    };
    let account_id = mock_account_id();

    encryption_store.add_key(&account_id, &key).unwrap();

    let db_config = ClientDatabaseConfig::default();
    let client = TransportLayerClient::init(
        grpc_client,
        encryption_store,
        vec![account_id],
        Some(db_config),
    )
    .await
    .unwrap();

    (client, account_id, key.public_key().unwrap())
}
