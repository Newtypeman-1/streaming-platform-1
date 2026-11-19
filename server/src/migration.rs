//! DB 스키마 마이그레이션. 서버가 뜰 때 아직 적용되지 않은 것만 순서대로 적용한다.

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

pub struct Migrator;

impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20261119_000001_create_broadcast::Migration)]
    }
}

mod m20261119_000001_create_broadcast {
    use super::*;

    #[derive(DeriveMigrationName)]
    pub struct Migration;

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .create_table(
                    Table::create()
                        .table(Broadcast::Table)
                        .col(big_pk_auto(Broadcast::Id))
                        // PostgreSQL 의 varchar(n) 은 바이트가 아니라 글자 수 기준이다
                        .col(string_len(Broadcast::Title, crate::broadcast::TITLE_MAX_CHARS as u32))
                        .col(string(Broadcast::VideoKey))
                        .col(string(Broadcast::VideoContentType))
                        .col(timestamp_with_time_zone(Broadcast::CreatedAt))
                        .to_owned(),
                )
                .await
        }

        async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager.drop_table(Table::drop().table(Broadcast::Table).to_owned()).await
        }
    }

    #[derive(DeriveIden)]
    enum Broadcast {
        Table,
        Id,
        Title,
        VideoKey,
        VideoContentType,
        CreatedAt,
    }
}
