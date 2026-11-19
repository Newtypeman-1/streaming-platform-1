pub mod api;
pub mod broadcast;
pub mod entity;
pub mod error;
pub mod migration;
pub mod storage;

use std::path::Path;

use sea_orm::{Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;

pub use api::router;
use storage::LocalVideoStorage;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub storage: LocalVideoStorage,
}

impl AppState {
    /// DB에 연결하고 아직 적용되지 않은 마이그레이션을 적용한다.
    pub async fn connect(database_url: &str, storage_dir: impl AsRef<Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let db = Database::connect(database_url).await?;
        migration::Migrator::up(&db, None).await?;
        let storage = LocalVideoStorage::new(storage_dir)?;
        Ok(Self { db, storage })
    }
}
