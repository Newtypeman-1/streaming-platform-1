//! 라이브 방송 도메인 규칙.

use std::collections::HashSet;

use chrono::{DateTime, Duration, Utc};
use shared::BroadcastStatus;

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum LiveError {
    #[error("송출 키가 올바르지 않습니다.")]
    InvalidStreamKey,
    #[error("방송 중이 아닙니다.")]
    NotLive,
}

/// 미디어 서버(MediaMTX) 안에서 이 방송의 영상이 오가는 경로.
pub fn media_path(id: i64) -> String {
    format!("live/{id}")
}

/// 송출 키가 맞는지 확인한다. 녹화 방송처럼 키가 없는 방송은 어떤 키로도 통과하지 못한다.
pub fn verify_stream_key(stored: Option<&str>, given: Option<&str>) -> Result<(), LiveError> {
    match (stored, given) {
        (Some(stored), Some(given)) if stored == given => Ok(()),
        _ => Err(LiveError::InvalidStreamKey),
    }
}

/// 송출·시청은 방송 중일 때만 할 수 있다.
pub fn ensure_live(status: BroadcastStatus) -> Result<(), LiveError> {
    match status {
        BroadcastStatus::Live => Ok(()),
        _ => Err(LiveError::NotLive),
    }
}

/// 방송 중으로 기록돼 있지만 미디어 서버에 영상이 들어오지 않는 방송을 고른다.
/// 시작 직후엔 아직 송출 연결 중일 수 있으므로 `grace` 동안은 봐준다.
pub fn dropped_streams(
    live: &[(i64, DateTime<Utc>)],
    ready_paths: &HashSet<String>,
    now: DateTime<Utc>,
    grace: Duration,
) -> Vec<i64> {
    live.iter()
        .filter(|(id, started_at)| now - *started_at > grace && !ready_paths.contains(&media_path(*id)))
        .map(|(id, _)| *id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 방송마다_미디어_경로가_다르다() {
        assert_eq!(media_path(7), "live/7");
    }

    #[test]
    fn 송출_키가_같아야_통과한다() {
        assert_eq!(verify_stream_key(Some("k1"), Some("k1")), Ok(()));
        assert_eq!(verify_stream_key(Some("k1"), Some("k2")), Err(LiveError::InvalidStreamKey));
        assert_eq!(verify_stream_key(Some("k1"), None), Err(LiveError::InvalidStreamKey));
    }

    #[test]
    fn 키가_없는_방송은_어떤_키로도_통과하지_못한다() {
        assert_eq!(verify_stream_key(None, Some("")), Err(LiveError::InvalidStreamKey));
        assert_eq!(verify_stream_key(None, None), Err(LiveError::InvalidStreamKey));
    }

    #[test]
    fn 방송_중일_때만_송출_시청할_수_있다() {
        assert_eq!(ensure_live(BroadcastStatus::Live), Ok(()));
        assert_eq!(ensure_live(BroadcastStatus::Ended), Err(LiveError::NotLive));
        assert_eq!(ensure_live(BroadcastStatus::Recorded), Err(LiveError::NotLive));
    }

    #[test]
    fn 영상이_끊긴_방송만_고른다() {
        let now = Utc::now();
        let long_ago = now - Duration::minutes(5);
        let live = [(1, long_ago), (2, long_ago)];
        let ready = HashSet::from([media_path(1)]);

        assert_eq!(dropped_streams(&live, &ready, now, Duration::seconds(30)), [2]);
    }

    #[test]
    fn 막_시작한_방송은_영상이_없어도_유예한다() {
        let now = Utc::now();
        let live = [(1, now - Duration::seconds(10)), (2, now - Duration::seconds(31))];

        assert_eq!(dropped_streams(&live, &HashSet::new(), now, Duration::seconds(30)), [2]);
    }
}
