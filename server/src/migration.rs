//! DB 스키마 마이그레이션. 서버가 뜰 때 아직 적용되지 않은 것만 순서대로 적용한다.

use sea_orm_migration::prelude::*;
use sea_orm_migration::schema::*;

pub struct Migrator;

impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20261119_000001_create_broadcast::Migration),
            Box::new(m20261120_000001_add_live::Migration),
            Box::new(m20261121_000001_add_users::Migration),
            Box::new(m20261121_000002_add_broadcast_owner::Migration),
            Box::new(m20261121_000003_add_comment::Migration),
        ]
    }
}

/// 3단계: 댓글. 방송을 지우면 댓글도 함께 지운다.
mod m20261121_000003_add_comment {
    use super::*;

    pub struct Migration;

    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20261121_000003_add_comment"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .create_table(
                    Table::create()
                        .table(Comment::Table)
                        .col(big_pk_auto(Comment::Id))
                        .col(big_integer(Comment::BroadcastId))
                        .col(big_integer(Comment::UserId))
                        .col(string_len(Comment::Body, crate::comment::BODY_MAX_CHARS as u32))
                        .col(timestamp_with_time_zone(Comment::CreatedAt))
                        .col(timestamp_with_time_zone_null(Comment::UpdatedAt))
                        .foreign_key(
                            ForeignKey::create()
                                .from(Comment::Table, Comment::BroadcastId)
                                .to(Broadcast::Table, Broadcast::Id)
                                .on_delete(ForeignKeyAction::Cascade),
                        )
                        .foreign_key(
                            ForeignKey::create()
                                .from(Comment::Table, Comment::UserId)
                                .to(Users::Table, Users::Id)
                                .on_delete(ForeignKeyAction::Cascade),
                        )
                        .to_owned(),
                )
                .await?;
            // 방송별 목록 조회용
            manager
                .create_index(Index::create().name("idx_comment_broadcast").table(Comment::Table).col(Comment::BroadcastId).to_owned())
                .await
        }

        async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager.drop_table(Table::drop().table(Comment::Table).to_owned()).await
        }
    }

    #[derive(DeriveIden)]
    enum Comment {
        Table,
        Id,
        BroadcastId,
        UserId,
        Body,
        CreatedAt,
        UpdatedAt,
    }

    #[derive(DeriveIden)]
    enum Broadcast {
        Table,
        Id,
    }

    #[derive(DeriveIden)]
    enum Users {
        Table,
        Id,
    }
}

/// 3단계: 방송 소유자. 이전 단계에서 만든 방송은 소유자가 없다(NULL).
mod m20261121_000002_add_broadcast_owner {
    use super::*;

    pub struct Migration;

    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20261121_000002_add_broadcast_owner"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .alter_table(
                    Table::alter()
                        .table(Broadcast::Table)
                        .add_column(big_integer_null(Broadcast::UserId))
                        .add_foreign_key(
                            TableForeignKey::new()
                                .name("fk_broadcast_user")
                                .from_tbl(Broadcast::Table)
                                .from_col(Broadcast::UserId)
                                .to_tbl(Users::Table)
                                .to_col(Users::Id)
                                .on_delete(ForeignKeyAction::SetNull),
                        )
                        .to_owned(),
                )
                .await
        }

        async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager.alter_table(Table::alter().table(Broadcast::Table).drop_column(Broadcast::UserId).to_owned()).await
        }
    }

    #[derive(DeriveIden)]
    enum Broadcast {
        Table,
        UserId,
    }

    #[derive(DeriveIden)]
    enum Users {
        Table,
        Id,
    }
}

/// 3단계: 회원과 로그인 세션.
mod m20261121_000001_add_users {
    use super::*;

    pub struct Migration;

    impl MigrationName for Migration {
        fn name(&self) -> &str {
            "m20261121_000001_add_users"
        }
    }

    #[async_trait::async_trait]
    impl MigrationTrait for Migration {
        async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager
                .create_table(
                    Table::create()
                        .table(Users::Table)
                        .col(big_pk_auto(Users::Id))
                        .col(string_len_uniq(Users::Username, crate::auth::USERNAME_MAX_CHARS as u32))
                        .col(string(Users::PasswordHash))
                        .col(timestamp_with_time_zone(Users::CreatedAt))
                        .to_owned(),
                )
                .await?;
            manager
                .create_table(
                    Table::create()
                        .table(Session::Table)
                        .col(big_pk_auto(Session::Id))
                        .col(big_integer(Session::UserId))
                        .col(string_uniq(Session::TokenHash))
                        .col(timestamp_with_time_zone(Session::ExpiresAt))
                        .col(timestamp_with_time_zone(Session::CreatedAt))
                        .foreign_key(
                            ForeignKey::create()
                                .from(Session::Table, Session::UserId)
                                .to(Users::Table, Users::Id)
                                .on_delete(ForeignKeyAction::Cascade),
                        )
                        .to_owned(),
                )
                .await
        }

        async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
            manager.drop_table(Table::drop().table(Session::Table).to_owned()).await?;
            manager.drop_table(Table::drop().table(Users::Table).to_owned()).await
        }
    }

    #[derive(DeriveIden)]
    enum Users {
        Table,
        Id,
        Username,
        PasswordHash,
        CreatedAt,
    }

    #[derive(DeriveIden)]
    enum Session {
        Table,
        Id,
        UserId,
        TokenHash,
        ExpiresAt,
        CreatedAt,
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
