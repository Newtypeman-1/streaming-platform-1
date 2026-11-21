//! DB 스키마 마이그레이션. 서버가 뜰 때 아직 적용되지 않은 것만 순서대로 적용한다.

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

pub struct Migrator;

impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20261119_000001_create_broadcast::Migration),
            Box::new(m20261120_000001_add_live::Migration),
        ]
    }
}

/// 2단계: 라이브 방송. 기존 녹화 방송은 status = 'recorded' 가 된다.
mod m20261120_000001_add_live {
    use super::*;

    pub struct Migration;

    // DeriveMigrationName 은 파일 이름("migration")을 쓰므로 한 파일에 두 개를 둘 수 없다.
    // 첫 마이그레이션은 이미 그 이름으로 적용된 DB 가 있어 그대로 두고, 이후부터는 이름을 직접 정한다.
    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20261120_000001_add_live"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .alter_table(
                    Table::alter()
                        .table(Broadcast::Table)
                        .add_column(string_len(Broadcast::Status, 16).default("recorded"))
                        .modify_column(ColumnDef::new(Broadcast::VideoKey).string().null())
                        .modify_column(ColumnDef::new(Broadcast::VideoContentType).string().null())
                        .add_column(string_null(Broadcast::StreamKey))
                        .add_column(timestamp_with_time_zone_null(Broadcast::EndedAt))
                        .to_owned(),
                )
                .await
        }

        async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .exec_stmt(Query::delete().from_table(Broadcast::Table).cond_where(Expr::col(Broadcast::VideoKey).is_null()).to_owned())
                .await?;
            manager
                .alter_table(
                    Table::alter()
                        .table(Broadcast::Table)
                        .drop_column(Broadcast::Status)
                        .modify_column(ColumnDef::new(Broadcast::VideoKey).string().not_null())
                        .modify_column(ColumnDef::new(Broadcast::VideoContentType).string().not_null())
                        .drop_column(Broadcast::StreamKey)
                        .drop_column(Broadcast::EndedAt)
                        .to_owned(),
                )
                .await
        }
    }

    #[derive(DeriveIden)]
    enum Broadcast {
        Table,
        Status,
        VideoKey,
        VideoContentType,
        StreamKey,
        EndedAt,
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
