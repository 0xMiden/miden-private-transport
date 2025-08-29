use std::sync::Arc;

use tokio::time::{Duration, sleep};
use tracing::{error, info};

use super::{Database, DatabaseConfig};
use crate::Result;

enum State {
    Stopped,
    Running,
}

/// Perform periodic maintenance of the database
pub struct DatabaseMaintenance {
    database: Arc<Database>,
    config: DatabaseConfig,
    state: State,
}

impl DatabaseMaintenance {
    pub fn new(database: Arc<Database>, config: DatabaseConfig) -> Self {
        Self { database, config, state: State::Stopped }
    }

    pub async fn entrypoint(mut self) {
        self.state = State::Running;
        while self.is_active() {
            if let Err(e) = self.step().await {
                error!("Database maintenance error: {e}");
            }
        }
    }

    async fn step(&mut self) -> Result<()> {
        self.database.cleanup_old_notes(self.config.retention_days).await?;
        info!("Cleaned up old notes");

        sleep(Duration::from_secs(600)).await;

        Ok(())
    }

    fn is_active(&self) -> bool {
        matches!(self.state, State::Running)
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serial_test::serial;

    use super::*;
    use crate::types::{StoredNote, test_note_header};

    fn note_at(age: Duration) -> StoredNote {
        StoredNote {
            header: test_note_header(),
            details: vec![1, 2, 3, 4],
            created_at: Utc::now() - age,
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_cleanup_old_notes_no_retention() {
        let config = DatabaseConfig { retention_days: 0, ..Default::default() };

        let db = Arc::new(Database::connect(config.clone()).await.unwrap());
        db.store_note(&note_at(Duration::from_secs(30))).await.unwrap();

        let maintenance = DatabaseMaintenance::new(db.clone(), config);
        tokio::spawn(maintenance.entrypoint());
        sleep(Duration::from_secs(2)).await;

        let (total_notes, _) = db.get_stats().await.unwrap();
        assert_eq!(total_notes, 0);
    }

    #[tokio::test]
    #[serial]
    async fn test_cleanup_old_notes_retention() {
        let config = DatabaseConfig { retention_days: 7, ..Default::default() };

        let db = Arc::new(Database::connect(config.clone()).await.unwrap());
        db.store_note(&note_at(Duration::from_secs(30))).await.unwrap();

        let maintenance = DatabaseMaintenance::new(db.clone(), config);
        tokio::spawn(maintenance.entrypoint());
        sleep(Duration::from_secs(2)).await;

        let (total_notes, _) = db.get_stats().await.unwrap();
        assert_eq!(total_notes, 1);
    }

    #[tokio::test]
    #[serial]
    async fn test_cleanup_old_notes_mixed_ages() {
        let config = DatabaseConfig { retention_days: 1, ..Default::default() };

        let db = Arc::new(Database::connect(config.clone()).await.unwrap());
        db.store_note(&note_at(Duration::from_secs(30))).await.unwrap();
        db.store_note(&note_at(Duration::from_secs(3600 * 26))).await.unwrap();

        let maintenance = DatabaseMaintenance::new(db.clone(), config);
        tokio::spawn(maintenance.entrypoint());
        sleep(Duration::from_secs(2)).await;

        let (total_notes, _) = db.get_stats().await.unwrap();
        assert_eq!(total_notes, 1);
    }
}
