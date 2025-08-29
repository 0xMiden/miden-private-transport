mod maintenance;
mod sqlite;

use chrono::{DateTime, Utc};

pub use self::maintenance::DatabaseMaintenance;
use self::sqlite::SQLiteDB;
use crate::{
    Result,
    metrics::MetricsDatabase,
    types::{NoteId, NoteTag, StoredNote},
};

/// Database operations
#[async_trait::async_trait]
pub trait DatabaseBackend: Send + Sync {
    /// Connect to the database
    async fn connect(config: DatabaseConfig, metrics: MetricsDatabase) -> Result<Self>
    where
        Self: Sized;

    /// Store a new note
    async fn store_note(&self, note: &StoredNote) -> Result<()>;

    /// Fetch notes by tag
    async fn fetch_notes(&self, tag: NoteTag, timestamp: DateTime<Utc>) -> Result<Vec<StoredNote>>;

    /// Get statistics about the database
    async fn get_stats(&self) -> Result<(u64, u64)>;

    /// Clean up old notes based on retention policy
    async fn cleanup_old_notes(&self, retention_days: u32) -> Result<u64>;

    /// Check if a note exists
    async fn note_exists(&self, note_id: NoteId) -> Result<bool>;
}

/// Database manager for the transport layer
pub struct Database {
    backend: Box<dyn DatabaseBackend>,
}

#[derive(Debug, Clone)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_note_size: usize,
    pub retention_days: u32,
    pub rate_limit_per_minute: u32,
    pub request_timeout_seconds: u64,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: "sqlite::memory:".to_string(),
            retention_days: 30,
            rate_limit_per_minute: 100,
            request_timeout_seconds: 10,
            max_note_size: 1024 * 1024,
        }
    }
}

impl Database {
    /// Connect to a database (with `SQLite` backend)
    pub async fn connect(config: DatabaseConfig, metrics: MetricsDatabase) -> Result<Self> {
        let backend = SQLiteDB::connect(config, metrics).await?;
        Ok(Self { backend: Box::new(backend) })
    }

    /// Store a new note
    pub async fn store_note(&self, note: &StoredNote) -> Result<()> {
        self.backend.store_note(note).await
    }

    /// Fetch notes by tag, optionally filtered by block number
    pub async fn fetch_notes(
        &self,
        tag: NoteTag,
        timestamp: DateTime<Utc>,
    ) -> Result<Vec<StoredNote>> {
        self.backend.fetch_notes(tag, timestamp).await
    }

    /// Get statistics about the database
    pub async fn get_stats(&self) -> Result<(u64, u64)> {
        self.backend.get_stats().await
    }

    /// Clean up old notes based on retention policy
    pub async fn cleanup_old_notes(&self, retention_days: u32) -> Result<u64> {
        self.backend.cleanup_old_notes(retention_days).await
    }

    /// Check if a note exists
    pub async fn note_exists(&self, note_id: NoteId) -> Result<bool> {
        self.backend.note_exists(note_id).await
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::{
        metrics::Metrics,
        types::{TEST_TAG, test_note_header},
    };

    #[tokio::test]
    async fn test_sqlite_database() {
        let db = Database::connect(DatabaseConfig::default(), Metrics::default().db)
            .await
            .unwrap();
        let start = Utc::now();

        let note = StoredNote {
            header: test_note_header(),
            encrypted_data: vec![1, 2, 3, 4],
            created_at: Utc::now(),
            received_at: Utc::now(),
            received_by: None,
        };

        db.store_note(&note).await.unwrap();

        let fetched_notes = db.fetch_notes(TEST_TAG.into(), start).await.unwrap();
        assert_eq!(fetched_notes.len(), 1);
        assert_eq!(fetched_notes[0].header.id(), note.header.id());

        // Test note exists
        assert!(db.note_exists(note.header.id()).await.unwrap());

        // Test stats
        let (total_notes, total_tags) = db.get_stats().await.unwrap();
        assert_eq!(total_notes, 1);
        assert_eq!(total_tags, 1);
    }

    #[tokio::test]
    async fn test_fetch_notes_timestamp_filtering() {
        let db = Database::connect(DatabaseConfig::default(), Metrics::default().db)
            .await
            .unwrap();

        // Create a note with a specific received_at time
        let received_time = Utc::now();
        let note = StoredNote {
            header: test_note_header(),
            encrypted_data: vec![1, 2, 3, 4],
            created_at: received_time,
            received_at: received_time,
            received_by: None,
        };

        db.store_note(&note).await.unwrap();

        // Fetch notes with timestamp before the note was received - should return the note
        let before_timestamp = received_time - chrono::Duration::seconds(1);
        let fetched_notes = db.fetch_notes(TEST_TAG.into(), before_timestamp).await.unwrap();
        assert_eq!(fetched_notes.len(), 1);
        assert_eq!(fetched_notes[0].header.id(), note.header.id());

        // Fetch notes with timestamp after the note was received - should return empty
        let after_timestamp = received_time + chrono::Duration::seconds(1);
        let fetched_notes = db.fetch_notes(TEST_TAG.into(), after_timestamp).await.unwrap();
        assert_eq!(fetched_notes.len(), 0);
    }
}
