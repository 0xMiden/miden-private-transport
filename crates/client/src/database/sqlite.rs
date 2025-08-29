use chrono::{DateTime, Utc};
use miden_objects::{
    account::AccountId,
    note::{NoteHeader, NoteId, NoteTag},
    utils::{Deserializable, Serializable},
};
use sqlx::{Row, SqlitePool};

use super::{DatabaseBackend, DatabaseConfig, DatabaseStats, StoredNote};
use crate::Result;

/// `SQLite` implementation of the client database
pub struct SqliteDatabase {
    pool: SqlitePool,
}

impl SqliteDatabase {
    /// Connect to the `SQLite` client database
    pub async fn connect(config: DatabaseConfig) -> Result<Self> {
        if !std::path::Path::new(&config.url).exists() && !config.url.contains(":memory:") {
            std::fs::File::create(&config.url).map_err(crate::Error::Io)?;
        }
        let url = format!("sqlite:{}", config.url);

        let pool = SqlitePool::connect(&url).await?;

        // Create tables if they don't exist
        Self::create_tables(&pool).await?;

        Ok(Self { pool })
    }

    /// Create all necessary tables
    async fn create_tables(pool: &SqlitePool) -> Result<()> {
        // Table for storing fetched note IDs
        sqlx::query(
            r"
            CREATE TABLE IF NOT EXISTS fetched_notes (
                note_id BLOB PRIMARY KEY,
                tag INTEGER NOT NULL,
                fetched_at TEXT NOT NULL
            ) STRICT;
            ",
        )
        .execute(pool)
        .await?;

        // Table for storing notes
        sqlx::query(
            r"
            CREATE TABLE IF NOT EXISTS stored_notes (
                note_id BLOB PRIMARY KEY,
                tag INTEGER NOT NULL,
                header BLOB NOT NULL,
                details BLOB NOT NULL,
                created_at TEXT NOT NULL
            ) STRICT;
            ",
        )
        .execute(pool)
        .await?;

        // Table for storing tag to account ID mappings
        sqlx::query(
            r"
            CREATE TABLE IF NOT EXISTS tag_account_mappings (
                tag INTEGER PRIMARY KEY,
                account_id BLOB NOT NULL,
                created_at TEXT NOT NULL
            ) STRICT;
            ",
        )
        .execute(pool)
        .await?;

        // Create indexes for better performance
        sqlx::query(
            r"
            CREATE INDEX IF NOT EXISTS idx_fetched_notes_tag ON fetched_notes(tag);
            CREATE INDEX IF NOT EXISTS idx_fetched_notes_fetched_at ON fetched_notes(fetched_at);
            CREATE INDEX IF NOT EXISTS idx_stored_notes_tag ON stored_notes(tag);
            CREATE INDEX IF NOT EXISTS idx_stored_notes_created_at ON stored_notes(created_at);
            ",
        )
        .execute(pool)
        .await?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl DatabaseBackend for SqliteDatabase {
    async fn store_note(
        &self,
        header: &NoteHeader,
        details: &[u8],
        created_at: DateTime<Utc>,
    ) -> Result<()> {
        let note_id = header.id();
        let tag = header.metadata().tag();
        let header_bytes = header.to_bytes();

        sqlx::query(
            r"
            INSERT OR REPLACE INTO stored_notes (note_id, tag, header, details, created_at)
            VALUES (?, ?, ?, ?, ?)
            ",
        )
        .bind(&note_id.inner().as_bytes()[..])
        .bind(i64::from(tag.as_u32()))
        .bind(&header_bytes)
        .bind(details)
        .bind(created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_stored_note(&self, note_id: &NoteId) -> Result<Option<StoredNote>> {
        let row = sqlx::query(
            r"
            SELECT tag, header, details, created_at
            FROM stored_notes WHERE note_id = ?
            ",
        )
        .bind(&note_id.inner().as_bytes()[..])
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let header_bytes: Vec<u8> = row.try_get("header")?;
            let details: Vec<u8> = row.try_get("details")?;
            let created_at_str: String = row.try_get("created_at")?;

            let header = NoteHeader::read_from_bytes(&header_bytes).map_err(|e| {
                crate::Error::Database(sqlx::Error::ColumnDecode {
                    index: "header".to_string(),
                    source: Box::new(e),
                })
            })?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| {
                    crate::Error::Database(sqlx::Error::ColumnDecode {
                        index: "created_at".to_string(),
                        source: Box::new(e),
                    })
                })?
                .with_timezone(&Utc);

            Ok(Some(StoredNote { header, details, created_at }))
        } else {
            Ok(None)
        }
    }

    async fn get_stored_notes_for_tag(&self, tag: NoteTag) -> Result<Vec<StoredNote>> {
        let rows = sqlx::query(
            r"
            SELECT note_id, header, details, created_at
            FROM stored_notes WHERE tag = ?
            ORDER BY created_at ASC
            ",
        )
        .bind(i64::from(tag.as_u32()))
        .fetch_all(&self.pool)
        .await?;

        let mut notes = Vec::new();
        for row in rows {
            let header_bytes: Vec<u8> = row.try_get("header")?;
            let details: Vec<u8> = row.try_get("details")?;
            let created_at_str: String = row.try_get("created_at")?;

            let header = NoteHeader::read_from_bytes(&header_bytes).map_err(|e| {
                crate::Error::Database(sqlx::Error::ColumnDecode {
                    index: "header".to_string(),
                    source: Box::new(e),
                })
            })?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| {
                    crate::Error::Database(sqlx::Error::ColumnDecode {
                        index: "created_at".to_string(),
                        source: Box::new(e),
                    })
                })?
                .with_timezone(&Utc);

            notes.push(StoredNote { header, details, created_at });
        }

        Ok(notes)
    }

    async fn record_fetched_note(&self, note_id: &NoteId, tag: NoteTag) -> Result<()> {
        let now = Utc::now();

        sqlx::query(
            r"
            INSERT OR REPLACE INTO fetched_notes (note_id, tag, fetched_at)
            VALUES (?, ?, ?)
            ",
        )
        .bind(&note_id.inner().as_bytes()[..])
        .bind(i64::from(tag.as_u32()))
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn note_fetched(&self, note_id: &NoteId) -> Result<bool> {
        let row = sqlx::query(
            r"
            SELECT 1 FROM fetched_notes WHERE note_id = ?
            ",
        )
        .bind(&note_id.inner().as_bytes()[..])
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.is_some())
    }

    async fn get_fetched_notes_for_tag(&self, tag: NoteTag) -> Result<Vec<NoteId>> {
        let rows = sqlx::query(
            r"
            SELECT note_id FROM fetched_notes WHERE tag = ?
            ORDER BY fetched_at ASC
            ",
        )
        .bind(i64::from(tag.as_u32()))
        .fetch_all(&self.pool)
        .await?;

        let mut note_ids = Vec::new();
        for row in rows {
            let note_id_bytes: Vec<u8> = row.try_get("note_id")?;
            let note_id = NoteId::read_from_bytes(&note_id_bytes).map_err(|e| {
                crate::Error::Database(sqlx::Error::ColumnDecode {
                    index: "note_id".to_string(),
                    source: Box::new(e),
                })
            })?;
            note_ids.push(note_id);
        }

        Ok(note_ids)
    }

    async fn get_stats(&self) -> Result<DatabaseStats> {
        let fetched_notes_count: u64 = sqlx::query_scalar("SELECT COUNT(*) FROM fetched_notes")
            .fetch_one(&self.pool)
            .await?;

        let stored_notes_count: u64 = sqlx::query_scalar("SELECT COUNT(*) FROM stored_notes")
            .fetch_one(&self.pool)
            .await?;

        let unique_tags_count: u64 =
            sqlx::query_scalar("SELECT COUNT(DISTINCT tag) FROM stored_notes")
                .fetch_one(&self.pool)
                .await?;

        Ok(DatabaseStats {
            fetched_notes_count: fetched_notes_count as u64,
            stored_notes_count: stored_notes_count as u64,
            unique_tags_count: unique_tags_count as u64,
        })
    }

    async fn cleanup_old_data(&self, retention_days: u32) -> Result<u64> {
        let cutoff_date = Utc::now() - chrono::Duration::days(i64::from(retention_days));

        let result = sqlx::query(
            r"
            DELETE FROM stored_notes WHERE created_at < ?
            ",
        )
        .bind(cutoff_date.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    async fn store_tag_account_mapping(&self, tag: NoteTag, account_id: &AccountId) -> Result<()> {
        let now = Utc::now();

        sqlx::query(
            r"
            INSERT OR REPLACE INTO tag_account_mappings (tag, account_id, created_at)
            VALUES (?, ?, ?)
            ",
        )
        .bind(i64::from(tag.as_u32()))
        .bind(&account_id.to_bytes()[..])
        .bind(now.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_all_tag_account_mappings(&self) -> Result<Vec<(NoteTag, AccountId)>> {
        let rows = sqlx::query(
            r"
            SELECT tag, account_id FROM tag_account_mappings
            ORDER BY created_at ASC
            ",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut mappings = Vec::new();
        for row in rows {
            let tag: u32 = row.try_get("tag")?;
            let account_id_bytes: Vec<u8> = row.try_get("account_id")?;

            let tag = NoteTag::from(tag);
            let account_id = AccountId::read_from_bytes(&account_id_bytes).map_err(|e| {
                crate::Error::Database(sqlx::Error::ColumnDecode {
                    index: "account_id".to_string(),
                    source: Box::new(e),
                })
            })?;

            mappings.push((tag, account_id));
        }

        Ok(mappings)
    }
}
