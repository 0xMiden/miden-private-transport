pub mod sqlite;

use chrono::{DateTime, Utc};
use miden_objects::{
    account::AccountId,
    note::{NoteHeader, NoteId, NoteTag},
};

use crate::Result;

/// Trait for client database operations
#[async_trait::async_trait]
pub trait DatabaseBackend: Send + Sync {
    /// Store a note
    async fn store_note(
        &self,
        header: &NoteHeader,
        details: &[u8],
        created_at: DateTime<Utc>,
    ) -> Result<()>;

    /// Get a stored note by ID
    async fn get_stored_note(&self, note_id: &NoteId) -> Result<Option<StoredNote>>;

    /// Get all stored notes with provided tag
    async fn get_stored_notes_for_tag(&self, tag: NoteTag) -> Result<Vec<StoredNote>>;

    /// Record that a note has been fetched
    async fn record_fetched_note(&self, note_id: &NoteId, tag: NoteTag) -> Result<()>;

    /// Check if a note has been fetched before
    async fn note_fetched(&self, note_id: &NoteId) -> Result<bool>;

    /// Get all fetched note IDs for a specific tag
    async fn get_fetched_notes_for_tag(&self, tag: NoteTag) -> Result<Vec<NoteId>>;

    /// Store a tag to account ID mapping
    async fn store_tag_account_mapping(&self, tag: NoteTag, account_id: &AccountId) -> Result<()>;

    /// Get all tag to account ID mappings
    async fn get_all_tag_account_mappings(&self) -> Result<Vec<(NoteTag, AccountId)>>;

    /// Get database statistics
    async fn get_stats(&self) -> Result<DatabaseStats>;

    /// Clean up old data based on retention policy
    async fn cleanup_old_data(&self, retention_days: u32) -> Result<u64>;
}

/// Client database configuration
#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_note_size: usize,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: "sqlite::memory:".to_string(),
            max_note_size: 1024 * 1024, // 1MB default
        }
    }
}

/// Client database for storing persistent state
pub struct Database {
    backend: Box<dyn DatabaseBackend>,
}

impl Database {
    /// Create a new client database with the specified backend
    pub fn new(backend: Box<dyn DatabaseBackend>) -> Self {
        Self { backend }
    }

    /// Create a new SQLite-based client database
    pub async fn new_sqlite(config: DatabaseConfig) -> Result<Self> {
        let backend = sqlite::SqliteDatabase::connect(config).await?;
        Ok(Self::new(Box::new(backend)))
    }

    /// Store a tag to account ID mapping
    pub async fn store_tag_account_mapping(
        &self,
        tag: NoteTag,
        account_id: &AccountId,
    ) -> Result<()> {
        self.backend.store_tag_account_mapping(tag, account_id).await
    }

    /// Get all tag to account ID mappings
    pub async fn get_all_tag_account_mappings(&self) -> Result<Vec<(NoteTag, AccountId)>> {
        self.backend.get_all_tag_account_mappings().await
    }

    /// Store an encrypted note
    pub async fn store_note(
        &self,
        header: &NoteHeader,
        encrypted_data: &[u8],
        created_at: DateTime<Utc>,
    ) -> Result<()> {
        self.backend.store_note(header, encrypted_data, created_at).await
    }

    /// Get an stored note by ID
    pub async fn get_stored_note(&self, note_id: &NoteId) -> Result<Option<StoredNote>> {
        self.backend.get_stored_note(note_id).await
    }

    /// Get all stored notes for a tag
    pub async fn get_stored_notes_for_tag(&self, tag: NoteTag) -> Result<Vec<StoredNote>> {
        self.backend.get_stored_notes_for_tag(tag).await
    }

    /// Record that a note has been fetched
    pub async fn record_fetched_note(&self, note_id: &NoteId, tag: NoteTag) -> Result<()> {
        self.backend.record_fetched_note(note_id, tag).await
    }

    /// Check if a note has been fetched before
    pub async fn note_fetched(&self, note_id: &NoteId) -> Result<bool> {
        self.backend.note_fetched(note_id).await
    }

    /// Get all fetched note IDs for a specific tag
    pub async fn get_fetched_notes_for_tag(&self, tag: NoteTag) -> Result<Vec<NoteId>> {
        self.backend.get_fetched_notes_for_tag(tag).await
    }

    /// Get database statistics
    pub async fn get_stats(&self) -> Result<DatabaseStats> {
        self.backend.get_stats().await
    }

    /// Clean up old data based on retention policy
    pub async fn cleanup_old_data(&self, retention_days: u32) -> Result<u64> {
        self.backend.cleanup_old_data(retention_days).await
    }
}

/// Encrypted note stored in the client database
#[derive(Debug, Clone)]
pub struct StoredNote {
    pub header: NoteHeader,
    pub details: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

/// Client database statistics
#[derive(Debug, Clone)]
pub struct DatabaseStats {
    /// Downloaded notes
    pub fetched_notes_count: u64,
    /// Stored (kept) notes
    pub stored_notes_count: u64,
    /// Stored tags
    pub unique_tags_count: u64,
}

#[cfg(test)]
mod tests {
    use miden_objects::{note::NoteDetails, utils::Serializable};

    use super::*;
    use crate::types::mock_note_p2id;

    #[tokio::test]
    async fn test_client_database_operations() {
        let config = DatabaseConfig::default();

        let db = Database::new_sqlite(config).await.unwrap();

        let note = mock_note_p2id();
        let note_id = note.id();
        let tag = note.metadata().tag();
        let header = *note.header();
        let details = NoteDetails::from(note).to_bytes();

        db.record_fetched_note(&note_id, tag).await.unwrap();

        let created_at = Utc::now();
        db.store_note(&header, &details, created_at).await.unwrap();

        let stored_note = db.get_stored_note(&note_id).await.unwrap();
        assert!(stored_note.is_some());

        let stored_note = stored_note.unwrap();
        assert_eq!(stored_note.header.id(), note_id);
        assert_eq!(stored_note.details, details);

        // Test statistics
        let stats = db.get_stats().await.unwrap();
        assert_eq!(stats.fetched_notes_count, 1);
        assert_eq!(stats.stored_notes_count, 1);
        assert_eq!(stats.unique_tags_count, 1);
    }
}
