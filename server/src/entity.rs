pub mod broadcast {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "broadcast")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub title: String,
        /// `shared::BroadcastStatus` 의 문자열 (recorded / live / ended)
        pub status: String,
        /// LocalVideoStorage 안에서 영상을 찾는 키. 실제 파일 위치는 저장소가 결정한다. 녹화 방송만 있다.
        pub video_key: Option<String>,
        pub video_content_type: Option<String>,
        /// 라이브 송출 키. 라이브 방송만 있다.
        pub stream_key: Option<String>,
        pub created_at: DateTimeWithTimeZone,
        pub ended_at: Option<DateTimeWithTimeZone>,
        /// 방송한 사람. 3단계(로그인) 이전에 만든 방송은 없다.
        pub user_id: Option<i64>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(belongs_to = "super::user::Entity", from = "Column::UserId", to = "super::user::Column::Id")]
        User,
    }

    impl Related<super::user::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::User.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}

    impl Model {
        pub fn status(&self) -> shared::BroadcastStatus {
            // DB 에는 이 서버가 쓴 값만 들어간다
            shared::BroadcastStatus::parse(&self.status).expect("알 수 없는 방송 상태")
        }
    }
}

pub mod user {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "users")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        #[sea_orm(unique)]
        pub username: String,
        /// Argon2id PHC 문자열. 응답으로 내보내지 않는다.
        pub password_hash: String,
        pub created_at: DateTimeWithTimeZone,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}

    impl From<Model> for shared::UserResponse {
        fn from(user: Model) -> Self {
            Self { id: user.id, username: user.username }
        }
    }
}

pub mod session {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "session")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub user_id: i64,
        /// 세션 토큰의 SHA-256. 원문은 브라우저 쿠키에만 있다.
        #[sea_orm(unique)]
        pub token_hash: String,
        pub expires_at: DateTimeWithTimeZone,
        pub created_at: DateTimeWithTimeZone,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(belongs_to = "super::user::Entity", from = "Column::UserId", to = "super::user::Column::Id")]
        User,
    }

    impl Related<super::user::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::User.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}

pub mod comment {
    use sea_orm::entity::prelude::*;

    #[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
    #[sea_orm(table_name = "comment")]
    pub struct Model {
        #[sea_orm(primary_key)]
        pub id: i64,
        pub broadcast_id: i64,
        pub user_id: i64,
        pub body: String,
        pub created_at: DateTimeWithTimeZone,
        pub updated_at: Option<DateTimeWithTimeZone>,
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {
        #[sea_orm(belongs_to = "super::user::Entity", from = "Column::UserId", to = "super::user::Column::Id")]
        User,
    }

    impl Related<super::user::Entity> for Entity {
        fn to() -> RelationDef {
            Relation::User.def()
        }
    }

    impl ActiveModelBehavior for ActiveModel {}
}
