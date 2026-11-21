use std::env;
use std::time::Duration;

use server::{AppState, MediaServer, PaymentGateway, end_dropped_streams, router, with_instance_name};
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

    let toss_api_url = env::var("TOSS_API_URL").unwrap_or_else(|_| "https://api.tosspayments.com".into());
    let toss_secret_key = env::var("TOSS_SECRET_KEY").unwrap_or_default();
    let toss_client_key = env::var("TOSS_CLIENT_KEY").unwrap_or_default();
    if toss_secret_key.is_empty() || toss_client_key.is_empty() {
        tracing::warn!("TOSS_SECRET_KEY·TOSS_CLIENT_KEY 가 없어 후원 결제를 할 수 없습니다");
    }

    let mut payments = PaymentGateway::new(&toss_api_url, &toss_secret_key, &toss_client_key);
    // 결제 승인 실패 재현용 (테스트 키에서만 동작)
    if let Ok(code) = env::var("TOSS_TEST_CODE")
        && !code.is_empty()
    {
        tracing::warn!("결제 승인 요청에 테스트 코드 {code} 를 붙입니다");
        payments = payments.with_test_code(&code);
    }

    let state = AppState::connect(
        &database_url,
        &storage_dir,
        MediaServer::new(&media_url, &media_api_url),
        payments,
    )
    .await?;

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

    // 컨테이너에서는 HOSTNAME 이 컨테이너(Pod) 이름이다
    let instance = env::var("INSTANCE_NAME").or_else(|_| env::var("HOSTNAME")).unwrap_or_else(|_| "local".into());
    let app = with_instance_name(router(state), &instance).layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("listening on http://localhost:{port} (instance: {instance}, videos: {storage_dir}, media: {media_url})");
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
    Ok(())
}

/// 스케일 인 때 오케스트레이터가 보내는 SIGTERM(또는 Ctrl+C)을 받으면 새 연결은 받지 않고,
/// 처리 중인 요청은 끝까지 응답한 뒤 종료한다.
async fn shutdown_signal() {
    let ctrl_c = async { tokio::signal::ctrl_c().await.ok() };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! {
        _ = ctrl_c => {}
        _ = term => {}
    }
    tracing::info!("종료 신호를 받아 처리 중인 요청을 마치고 종료합니다");
}
