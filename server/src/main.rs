use std::env;

use server::{AppState, router};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

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

    let state = AppState::connect(&database_url, &storage_dir).await?;
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("listening on http://localhost:{port} (videos: {storage_dir})");
    axum::serve(listener, router(state).layer(TraceLayer::new_for_http())).await?;
    Ok(())
}
