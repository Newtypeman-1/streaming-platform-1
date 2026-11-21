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
    }

    #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
    pub enum Relation {}

    impl ActiveModelBehavior for ActiveModel {}

    impl Model {
        pub fn status(&self) -> shared::BroadcastStatus {
            // DB 에는 이 서버가 쓴 값만 들어간다
            shared::BroadcastStatus::parse(&self.status).expect("알 수 없는 방송 상태")
        }
    }
}
