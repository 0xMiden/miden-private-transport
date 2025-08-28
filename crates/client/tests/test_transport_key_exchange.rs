mod common;

use self::common::*;

#[tokio::test]
async fn test_transport_key_exchange_aes() -> Result<(), Box<dyn std::error::Error>> {
    let port = 9630;
    let handle = spawn_test_server(port).await;

    let (mut client0, _accid0, _) = test_client(port, EncryptionScheme::Aes).await;
    let (mut client1, accid1, pubkey1) = test_client(port, EncryptionScheme::Aes).await;

    // Register, fetch
    client0.register_key(accid1, pubkey1.clone()).await?;
    client1.fetch_key(accid1).await?;

    // Verify key was stored
    let stored_key = client1.get_key(&accid1).await?;
    assert!(stored_key.is_some());

    handle.abort();
    Ok(())
}

#[tokio::test]
async fn test_transport_key_exchange_x25519() -> Result<(), Box<dyn std::error::Error>> {
    let port = 9631;
    let handle = spawn_test_server(port).await;

    let (mut client0, _accid0, _) = test_client(port, EncryptionScheme::X25519).await;
    let (mut client1, accid1, pubkey1) = test_client(port, EncryptionScheme::X25519).await;

    // Register, fetch
    client0.register_key(accid1, pubkey1.clone()).await?;
    client1.fetch_key(accid1).await?;

    // Verify key was stored
    let stored_key = client1.get_key(&accid1).await?;
    assert!(stored_key.is_some());

    handle.abort();
    Ok(())
}
