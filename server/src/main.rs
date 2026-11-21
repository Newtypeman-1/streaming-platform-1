use std::env;
use std::time::Duration;

use server::{AppState, MediaServer, end_dropped_streams, router};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

/// 라이브 시작 후 송출이 연결될 때까지 기다려 주는 시간
const LIVE_CONNECT_GRACE: chrono::Duration = chrono::Duration::seconds(30);
/// 끊긴 라이브를 확인하는 주기
const DROPPED_STREAM_CHECK: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 현재 디렉터리부터 상위로 올라가며 .env 를 찾는다 (없으면 무시)
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=info".into()))
        .init();

    let database_url = env::var("DATABASE_URL").map_err(|_| "DATABASE_URL 환경변수가 필요합니다")?;
    let port: u16 = env::var("SERVER_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8000);
    let storage_dir = env::var("VIDEO_STORAGE_DIR").unwrap_or_else(|_| "storage/videos".into());
    let media_url = env::var("MEDIAMTX_URL").unwrap_or_else(|_| "http://localhost:8889".into());
    let media_api_url = env::var("MEDIAMTX_API_URL").unwrap_or_else(|_| "http://localhost:9997".into());

    let state = AppState::connect(&database_url, &storage_dir, MediaServer::new(&media_url, &media_api_url)).await?;

    let sweeper_state = state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(DROPPED_STREAM_CHECK);
        loop {
            interval.tick().await;
            match end_dropped_streams(&sweeper_state, LIVE_CONNECT_GRACE).await {
                Ok(ended) if !ended.is_empty() => tracing::info!("송출이 끊긴 라이브 종료: {ended:?}"),
                Ok(_) => {}
                Err(e) => tracing::warn!("끊긴 라이브 확인 실패: {e:?}"),
            }
        }
    });

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("listening on http://localhost:{port} (videos: {storage_dir}, media: {media_url})");
    axum::serve(listener, router(state).layer(TraceLayer::new_for_http())).await?;
    Ok(())
}
