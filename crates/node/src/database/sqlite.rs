use chrono::{DateTime, Utc};
use miden_objects::utils::{Deserializable, Serializable};
use sqlx::{Row, SqlitePool};

use crate::{
    Error, Result,
    database::{DatabaseBackend, DatabaseConfig},
    types::{
        AccountId, EncryptionKeyType, NoteHeader, NoteId, NoteTag, StoredEncryptionKey, StoredNote,
    },
};

/// `SQLite` implementation of the database backend
pub struct SQLiteDB {
    pool: SqlitePool,
}

#[async_trait::async_trait]
impl DatabaseBackend for SQLiteDB {
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
                encrypted_data BLOB NOT NULL,
                created_at TEXT NOT NULL,
                received_at TEXT NOT NULL,
                received_by TEXT
            ) STRICT;
            ",
        )
        .execute(&pool)
        .await?;

        sqlx::query(
            r"
            CREATE TABLE IF NOT EXISTS encryption_keys (
                account_id BLOB PRIMARY KEY,
                key_type TEXT NOT NULL,
                key_data BLOB NOT NULL,
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
            CREATE INDEX IF NOT EXISTS idx_notes_received_at ON notes(received_at);
            ",
        )
        .execute(&pool)
        .await?;

        Ok(Self { pool })
    }

    async fn store_note(&self, note: &StoredNote) -> Result<()> {
        let received_by_json = if let Some(ref received_by) = note.received_by {
            serde_json::to_string(received_by)?
        } else {
            "[]".to_string()
        };

        sqlx::query(
            r"
            INSERT INTO notes (id, tag, header, encrypted_data, created_at, received_at, received_by)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ",
        )
        .bind(&note.header.id().inner().as_bytes()[..])
        .bind(i64::from(note.header.metadata().tag().as_u32()))
        .bind(note.header.to_bytes())
        .bind(&note.encrypted_data)
        .bind(note.created_at.to_rfc3339())
        .bind(note.received_at.to_rfc3339())
        .bind(received_by_json)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn fetch_notes(&self, tag: NoteTag, timestamp: DateTime<Utc>) -> Result<Vec<StoredNote>> {
        let query = sqlx::query(
            r"
                SELECT id, tag, header, encrypted_data, created_at, received_at, received_by
                FROM notes
                WHERE tag = ? AND received_at > ?
                ORDER BY received_at ASC
                ",
        )
        .bind(i64::from(tag.as_u32()))
        .bind(timestamp.to_rfc3339());

        let rows = query.fetch_all(&self.pool).await?;
        let mut notes = Vec::new();

        for row in rows {
            let _id_bytes: Vec<u8> = row.try_get("id")?;
            let _tag: i64 = row.try_get("tag")?;
            let header_bytes: Vec<u8> = row.try_get("header")?;
            let encrypted_data: Vec<u8> = row.try_get("encrypted_data")?;
            let created_at_str: String = row.try_get("created_at")?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| {
                    Error::Database(sqlx::Error::ColumnDecode {
                        index: "created_at".to_string(),
                        source: Box::new(e),
                    })
                })?
                .with_timezone(&Utc);

            let received_at_str: String = row.try_get("received_at")?;
            let received_at = DateTime::parse_from_rfc3339(&received_at_str)
                .map_err(|e| {
                    Error::Database(sqlx::Error::ColumnDecode {
                        index: "received_at".to_string(),
                        source: Box::new(e),
                    })
                })?
                .with_timezone(&Utc);

            let received_by_json: String = row.try_get("received_by")?;

            let received_by: Option<Vec<String>> = if received_by_json == "[]" {
                None
            } else {
                Some(serde_json::from_str(&received_by_json)?)
            };

            let header = NoteHeader::read_from_bytes(&header_bytes).map_err(|e| {
                Error::Database(sqlx::Error::ColumnDecode {
                    index: "header".to_string(),
                    source: Box::new(e),
                })
            })?;

            let note = StoredNote {
                header,
                encrypted_data,
                created_at,
                received_at,
                received_by,
            };

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

    async fn store_encryption_key(&self, key: &StoredEncryptionKey) -> Result<()> {
        let key_type_str = match key.key_type {
            EncryptionKeyType::Aes256Gcm => "aes256gcm",
            EncryptionKeyType::X25519Pub => "x25519pub",
            EncryptionKeyType::Other => "other",
        };

        sqlx::query(
            r"
            INSERT OR REPLACE INTO encryption_keys (account_id, key_type, key_data, created_at)
            VALUES (?, ?, ?, ?)
            ",
        )
        .bind(&key.account_id.to_bytes()[..])
        .bind(key_type_str)
        .bind(&key.key_data)
        .bind(key.created_at.to_rfc3339())
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    async fn get_encryption_key(
        &self,
        account_id: &AccountId,
    ) -> Result<Option<StoredEncryptionKey>> {
        let row = sqlx::query(
            r"
            SELECT account_id, key_type, key_data, created_at
            FROM encryption_keys
            WHERE account_id = ?
            ",
        )
        .bind(&account_id.to_bytes()[..])
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            let key_type_str: String = row.try_get("key_type")?;
            let key_type = match key_type_str.as_str() {
                "aes256gcm" => EncryptionKeyType::Aes256Gcm,
                "x25519pub" => EncryptionKeyType::X25519Pub,
                "other" => EncryptionKeyType::Other,
                _ => {
                    return Err(Error::Database(sqlx::Error::ColumnDecode {
                        index: "key_type".to_string(),
                        source: Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Unknown key type: {key_type_str}"),
                        )),
                    }));
                },
            };

            let key_data: Vec<u8> = row.try_get("key_data")?;
            let created_at_str: String = row.try_get("created_at")?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| {
                    Error::Database(sqlx::Error::ColumnDecode {
                        index: "created_at".to_string(),
                        source: Box::new(e),
                    })
                })?
                .with_timezone(&Utc);

            let stored_key = StoredEncryptionKey {
                account_id: *account_id,
                key_type,
                key_data,
                created_at,
            };

            Ok(Some(stored_key))
        } else {
            Ok(None)
        }
    }
}
