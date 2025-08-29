use chrono::{DateTime, Utc};
use miden_objects::utils::{Deserializable, Serializable};
use sqlx::{Row, SqlitePool};

use crate::{
    Error, Result,
    database::{DatabaseBackend, DatabaseConfig},
    types::{NoteHeader, NoteId, NoteTag, StoredNote},
};

/// `SQLite` implementation of the database backend
pub struct SqliteDatabase {
    pool: SqlitePool,
}

#[async_trait::async_trait]
impl DatabaseBackend for SqliteDatabase {
    async fn connect(config: DatabaseConfig) -> Result<Self> {
        if !std::path::Path::new(&config.url).exists() && !config.url.contains(":memory:") {
            std::fs::File::create(&config.url).map_err(crate::Error::Io)?;
        }
        let url = format!("sqlite:{}", config.url);

        let pool = SqlitePool::connect(&url).await?;

        // Create tables if they don't exist
        sqlx::query(
            r"
            CREATE TABLE IF NOT EXISTS notes (
                id BLOB PRIMARY KEY,
                tag INTEGER NOT NULL,
                header BLOB NOT NULL,
                details BLOB NOT NULL,
                created_at TEXT NOT NULL
            ) STRICT;
            ",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r"
            CREATE INDEX IF NOT EXISTS idx_notes_tag ON notes(tag);
            CREATE INDEX IF NOT EXISTS idx_notes_created_at ON notes(created_at);
            ",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }

    async fn store_note(&self, note: &StoredNote) -> Result<()> {
        sqlx::query(
            r"
            INSERT INTO notes (id, tag, header, details, created_at)
            VALUES (?, ?, ?, ?, ?)
            ",
        )
        .bind(&note.header.id().inner().as_bytes()[..])
        .bind(i64::from(note.header.metadata().tag().as_u32()))
        .bind(note.header.to_bytes())
        .bind(&note.details)
        .bind(note.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn fetch_notes(&self, tag: NoteTag, timestamp: DateTime<Utc>) -> Result<Vec<StoredNote>> {
        let query = sqlx::query(
            r"
                SELECT id, tag, header, details, created_at
                FROM notes
                WHERE tag = ? AND created_at > ?
                ORDER BY created_at ASC
                ",
        )
        .bind(i64::from(tag.as_u32()))
        .bind(timestamp.to_rfc3339());

        let rows = query.fetch_all(&self.pool).await?;
        let mut notes = Vec::new();

        for row in rows {
            let header_bytes: Vec<u8> = row.try_get("header")?;
            let details: Vec<u8> = row.try_get("details")?;
            let created_at_str: String = row.try_get("created_at")?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| {
                    Error::Database(sqlx::Error::ColumnDecode {
                        index: "created_at".to_string(),
                        source: Box::new(e),
                    })
                })?
                .with_timezone(&Utc);

            let header = NoteHeader::read_from_bytes(&header_bytes).map_err(|e| {
                Error::Database(sqlx::Error::ColumnDecode {
                    index: "header".to_string(),
                    source: Box::new(e),
                })
            })?;

            let note = StoredNote { header, details, created_at };

            notes.push(note);
        }

        Ok(notes)
    }

    async fn get_stats(&self) -> Result<(u64, u64)> {
        let total_notes: u64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM notes").fetch_one(&self.pool).await?;

        let total_tags: u64 = sqlx::query_scalar("SELECT COUNT(DISTINCT tag) FROM notes")
            .fetch_one(&self.pool)
            .await?;

        Ok((total_notes, total_tags))
    }

    async fn cleanup_old_notes(&self, retention_days: u32) -> Result<u64> {
        let cutoff_date = Utc::now() - chrono::Duration::days(i64::from(retention_days));

        let result = sqlx::query(
            r"
            DELETE FROM notes WHERE created_at < ?
            ",
        )
        .bind(cutoff_date.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    async fn note_exists(&self, note_id: NoteId) -> Result<bool> {
        let count: i64 = sqlx::query_scalar(
            r"
            SELECT COUNT(*) FROM notes WHERE id = ?
            ",
        )
        .bind(&note_id.inner().as_bytes()[..])
        .fetch_one(&self.pool)
        .await?;

        Ok(count > 0)
    }
}
