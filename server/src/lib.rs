pub mod api;
pub mod auth;
pub mod auth_api;
pub mod broadcast;
pub mod comment;
pub mod comment_api;
pub mod donation;
pub mod donation_api;
pub mod entity;
pub mod error;
pub mod live;
pub mod live_api;
pub mod media;
pub mod migration;
pub mod payment;
pub mod storage;

use std::path::Path;

use sea_orm::sqlx::{self, Connection};
use sea_orm::{Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;

pub use api::router;
pub use live_api::end_dropped_streams;
pub use media::MediaServer;
pub use payment::PaymentGateway;
use storage::LocalVideoStorage;

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub storage: LocalVideoStorage,
    pub media: MediaServer,
    pub payments: PaymentGateway,
}

impl AppState {
    /// DB에 연결하고 아직 적용되지 않은 마이그레이션을 적용한다.
    pub async fn connect(
        database_url: &str,
        storage_dir: impl AsRef<Path>,
        media: MediaServer,
        payments: PaymentGateway,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let db = Database::connect(database_url).await?;
        migrate(&db).await?;
        let storage = LocalVideoStorage::new(storage_dir)?;
        Ok(Self { db, storage, media, payments })
    }
}

/// 마이그레이션 잠금 번호(PostgreSQL advisory lock). 값 자체에 의미는 없고 모든 인스턴스가 같기만 하면 된다.
const MIGRATION_LOCK: i64 = 0x6e61_6472_69;

/// 인스턴스 여러 개가 한꺼번에 떠도 마이그레이션은 하나씩 적용한다.
/// 잠금은 풀에서 떼어 낸 전용 연결에 걸고, 그 연결을 닫으면 풀린다(중간에 실패해도 남지 않는다).
async fn migrate(db: &DatabaseConnection) -> Result<(), Box<dyn std::error::Error>> {
    let mut lock = db.get_postgres_connection_pool().acquire().await?.detach();
    sqlx::query("SELECT pg_advisory_lock($1)").bind(MIGRATION_LOCK).execute(&mut lock).await?;
    let result = migration::Migrator::up(db, None).await;
    lock.close().await?;
    Ok(result?)
}
