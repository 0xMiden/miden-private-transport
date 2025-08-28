pub mod crypto;
pub mod database;
pub mod error;
pub mod grpc;
pub mod logging;
pub mod types;

use std::collections::HashMap;

use miden_objects::{
    account::AccountId,
    utils::{Deserializable, Serializable},
};

use self::{
    crypto::{EncryptionKey, SerializableKey},
    database::{ClientDatabase, ClientDatabaseConfig},
    types::{Note, NoteDetails, NoteHeader, NoteId, NoteInfo, NoteStatus, NoteTag},
};
pub use self::{
    error::{Error, Result},
    grpc::GrpcClient,
};

/// The main transport client trait for sending and receiving encrypted notes
#[async_trait::async_trait]
pub trait TransportClient: Send + Sync {
    /// Send a note with encrypted details
    async fn send_note(
        &mut self,
        header: NoteHeader,
        encrypted_note: Vec<u8>,
    ) -> Result<(NoteId, NoteStatus)>;

    /// Fetch all notes for a given tag
    async fn fetch_notes(&mut self, tag: NoteTag) -> Result<Vec<NoteInfo>>;

    /// Associate an encryption key with an account ID
    async fn register_key(&mut self, account_id: AccountId, key: SerializableKey) -> Result<()>;

    /// Fetch the key associated with an account ID
    async fn fetch_key(&mut self, account_id: AccountId) -> Result<Option<SerializableKey>>;
}

/// Encryption store trait for managing encryption keys
pub trait EncryptionStore: Send + Sync {
    /// Decrypt a message using the stored key for the given account ID
    fn decrypt(&self, msg: &[u8], id: &AccountId) -> Result<Vec<u8>>;

    /// Encrypt data for a recipient using their stored key
    fn encrypt(&self, data: &[u8], id: &AccountId) -> Result<Vec<u8>>;

    /// Add a key for an account ID
    fn add_key(&self, id: &AccountId, key: &SerializableKey) -> Result<()>;

    /// Get a key for an account ID
    fn get_key(&self, id: &AccountId) -> Result<Option<SerializableKey>>;
}

/// Filesystem-based encryption store
pub struct FilesystemEncryptionStore {
    key_dir: std::path::PathBuf,
}

impl FilesystemEncryptionStore {
    pub fn new<P: AsRef<std::path::Path>>(key_dir: P) -> Result<Self> {
        let key_dir = key_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&key_dir)?;
        Ok(Self { key_dir })
    }
}

impl EncryptionStore for FilesystemEncryptionStore {
    fn decrypt(&self, msg: &[u8], id: &AccountId) -> Result<Vec<u8>> {
        let key = self.get_key(id)?.ok_or_else(|| {
            Error::Decryption(format!(
                "Decryption key not found for Account ID {:02x?}",
                id.to_bytes()
            ))
        })?;

        if !key.can_decrypt() {
            return Err(Error::Decryption("Key cannot be used for decryption".to_string()));
        }

        key.decrypt(msg)
            .ok_or_else(|| Error::Decryption("Key does not support decryption".to_string()))?
    }

    fn encrypt(&self, data: &[u8], id: &AccountId) -> Result<Vec<u8>> {
        let key = self.get_key(id)?.ok_or_else(|| {
            Error::Encryption(format!(
                "Encryption key not found for Account ID {:02x?}",
                id.to_bytes()
            ))
        })?;

        // For encryption, we might need the public key component
        let encryption_key = if key.can_encrypt() {
            key
        } else if let Some(public_key) = key.public_key() {
            public_key
        } else {
            return Err(Error::Encryption("Key cannot be used for encryption".to_string()));
        };

        encryption_key.encrypt(data)
    }

    fn add_key(&self, id: &AccountId, key: &SerializableKey) -> Result<()> {
        let id_hex = hex::encode(id.to_bytes());
        let key_path = self.key_dir.join(format!("{id_hex}.key"));
        let key_json = serde_json::to_string(key)?;
        std::fs::write(key_path, key_json)?;
        Ok(())
    }

    fn get_key(&self, id: &AccountId) -> Result<Option<SerializableKey>> {
        let id_hex = hex::encode(id.to_bytes());
        let key_path = self.key_dir.join(format!("{id_hex}.key"));

        if key_path.exists() {
            let key_json = std::fs::read_to_string(key_path)?;
            let key: SerializableKey = serde_json::from_str(&key_json)?;
            Ok(Some(key))
        } else {
            Ok(None)
        }
    }
}

/// Client for interacting with the transport layer
pub struct TransportLayerClient {
    transport_client: Box<dyn TransportClient>,
    encryption_store: Box<dyn EncryptionStore>,
    /// Owned account IDs
    account_ids: Vec<AccountId>,
    /// Mapping between owned account IDs and note tags
    tag_accid_map: HashMap<NoteTag, AccountId>,
    /// Client database for persistent state
    database: ClientDatabase,
}

impl TransportLayerClient {
    pub async fn init(
        transport_client: Box<dyn TransportClient>,
        encryption_store: Box<dyn EncryptionStore>,
        account_ids: Vec<AccountId>,
        database_config: Option<ClientDatabaseConfig>,
    ) -> Result<Self> {
        let database = ClientDatabase::new_sqlite(database_config.unwrap_or_default()).await?;

        // Start with default tag mappings for owned account IDs
        let mut tag_accid_map: HashMap<NoteTag, AccountId> =
            account_ids.iter().map(|id| (NoteTag::from_account_id(*id), *id)).collect();

        // Load existing tag mappings from database
        if let Ok(existing_mappings) = database.get_all_tag_account_mappings().await {
            for (tag, account_id) in existing_mappings {
                tag_accid_map.insert(tag, account_id);
            }
        }

        Ok(Self {
            transport_client,
            encryption_store,
            account_ids,
            tag_accid_map,
            database,
        })
    }

    /// Send a note to a recipient
    pub async fn send_note(&mut self, note: Note, id: &AccountId) -> Result<(NoteId, NoteStatus)> {
        let header = *note.header();
        let details: NoteDetails = note.into();
        let details_bytes = details.to_bytes();
        let encrypted = self.encryption_store.encrypt(&details_bytes, id)?;
        self.transport_client.send_note(header, encrypted).await
    }

    /// Fetch and decrypt notes for a tag
    pub async fn fetch_notes(&mut self, tag: NoteTag) -> Result<Vec<(NoteHeader, NoteDetails)>> {
        let infos = self.transport_client.fetch_notes(tag).await?;
        let mut decrypted_notes = Vec::new();
        let id = self.get_accid_for_tag(tag).ok_or_else(|| {
            Error::InvalidTag(format!("Account ID not found for tag {}", tag.as_u32()))
        })?;

        for info in infos {
            // Check if we've already fetched this note
            if !self.database.note_fetched(&info.header.id()).await? {
                // Try to decrypt the note
                if let Ok(decrypted) = self.encryption_store.decrypt(&info.encrypted_data, id) {
                    // Mark note as fetched
                    self.database.record_fetched_note(&info.header.id(), tag).await?;

                    let details = NoteDetails::read_from_bytes(&decrypted).map_err(|e| {
                        Error::Decryption(format!("Failed to deserialize decrypted details: {e}"))
                    })?;
                    decrypted_notes.push((info.header, details));

                    // Store the encrypted note
                    self.database
                        .store_encrypted_note(
                            &info.header.id(),
                            tag,
                            &info.header,
                            &info.encrypted_data,
                            info.created_at,
                        )
                        .await?;
                } else {
                    // Skip notes that can't be decrypted
                }
            }
        }

        Ok(decrypted_notes)
    }

    /// Register a key to an account ID
    pub async fn register_key(
        &mut self,
        account_id: AccountId,
        key: SerializableKey,
    ) -> Result<()> {
        self.transport_client.register_key(account_id, key).await
    }

    /// Fetch the key associated with an account ID
    pub async fn fetch_key(&mut self, account_id: AccountId) -> Result<()> {
        let key = self.transport_client.fetch_key(account_id).await?.ok_or_else(|| {
            Error::Generic(anyhow::anyhow!("Encryption key unknown for {account_id}"))
        })?;
        self.encryption_store.add_key(&account_id, &key)?;
        self.database.store_key(&account_id, &key).await?;

        Ok(())
    }

    /// Adds a key associated with an account ID to the encryption store and database
    ///
    /// The key can be either of the ego client, or another network participant.
    pub async fn add_key(&mut self, key: &SerializableKey, account_id: &AccountId) -> Result<()> {
        self.encryption_store.add_key(account_id, key)?;
        self.database.store_key(account_id, key).await?;

        Ok(())
    }

    /// Adds an ego account ID
    pub fn add_account_id(&mut self, account_id: &AccountId) {
        self.account_ids.push(*account_id);
    }

    /// Get a key for an account ID from the database
    pub async fn get_key(&self, account_id: &AccountId) -> Result<Option<SerializableKey>> {
        self.database.get_key(account_id).await
    }

    /// Get all stored keys from the database
    pub async fn get_all_keys(&self) -> Result<Vec<(AccountId, SerializableKey)>> {
        self.database.get_all_keys().await
    }

    /// Check if a note has been fetched before
    pub async fn note_fetched(&self, note_id: &NoteId) -> Result<bool> {
        self.database.note_fetched(note_id).await
    }

    /// Get all fetched note IDs for a specific tag
    pub async fn get_fetched_notes_for_tag(&self, tag: NoteTag) -> Result<Vec<NoteId>> {
        self.database.get_fetched_notes_for_tag(tag).await
    }

    /// Get an encrypted note from the database
    pub async fn get_encrypted_note(
        &self,
        note_id: &NoteId,
    ) -> Result<Option<database::EncryptedNote>> {
        self.database.get_encrypted_note(note_id).await
    }

    /// Get all encrypted notes for a specific tag
    pub async fn get_encrypted_notes_for_tag(
        &self,
        tag: NoteTag,
    ) -> Result<Vec<database::EncryptedNote>> {
        self.database.get_encrypted_notes_for_tag(tag).await
    }

    /// Get database statistics
    pub async fn get_database_stats(&self) -> Result<database::ClientDatabaseStats> {
        self.database.get_stats().await
    }

    /// Clean up old data based on retention policy
    pub async fn cleanup_old_data(&self, retention_days: u32) -> Result<u64> {
        self.database.cleanup_old_data(retention_days).await
    }

    /// Registers a tag to an account ID
    ///
    /// If the account ID is not provided, the tag is registered under all owned account IDs.
    pub async fn register_tag(
        &mut self,
        tag: NoteTag,
        account_id: Option<AccountId>,
    ) -> Result<()> {
        let account_ids = if let Some(account_id) = account_id {
            &vec![account_id]
        } else {
            &self.account_ids
        };
        for id in account_ids {
            self.tag_accid_map.insert(tag, *id);
            // Persist the mapping to database
            self.database.store_tag_account_mapping(tag, id).await?;
        }
        Ok(())
    }

    fn get_accid_for_tag(&self, tag: NoteTag) -> Option<&AccountId> {
        self.tag_accid_map.get(&tag)
    }
}
