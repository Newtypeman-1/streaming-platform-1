# 작업내역

트위치를 레퍼런스로 한 인터넷 방송 플랫폼의 커밋별 작업 기록입니다.
번호는 `단계-순서`이고, 같은 이름의 태그가 커밋에 붙어 있습니다.

실행 방법은 [README.md](README.md)를 보세요.

## 기술 스택

| 구분 | 사용 |
|---|---|
| 언어 | Rust (Cargo 워크스페이스) |
| 백엔드 | axum, SeaORM |
| DB | PostgreSQL 17 (Docker) |
| 프론트엔드 | Dioxus (웹, WASM) |
| 라이브 | WebRTC (WHIP 송출 / WHEP 시청), MediaMTX (Docker) |
| 테스트 | cargo test, Testcontainers |

## 1단계: 녹화 영상 업로드와 재생

### 001-01 프로젝트 뼈대, Cargo 워크스페이스와 PostgreSQL Docker
- [링크](https://github.com/Newtypeman-1/streaming-platform-1/commit/001-01)
- 작업: server(axum + SeaORM), web(Dioxus), shared(공용 API 타입) 워크스페이스 구성, docker-compose로 PostgreSQL 구동
- 이유: 서버와 웹이 같은 타입을 쓰는 구조와 DB 실행 환경을 먼저 고정
- 결과: 접속 정보는 .env로 분리(.env.example 제공), .env·업로드 폴더·빌드 결과물은 git 제외

### 001-02 방송 제목 규칙과 로컬 영상 저장소
- [링크](https://github.com/Newtypeman-1/streaming-platform-1/commit/001-02)
- 작업: 제목 검증(공백 제거, 빈 값 금지, 100자 이하), video/* 타입만 허용, UUID 파일명으로 저장하는 LocalVideoStorage
- 이유: 업로드 API 전에 입력 규칙과 저장 위치를 먼저 정함. 사용자 파일명과 저장소 밖 경로(../)는 쓰지 않음
- 결과: 테스트 11개 통과. 제목 길이는 바이트가 아니라 글자 수 기준

### 001-03 방송 REST API, 등록·목록·상세·영상 재생
- [링크](https://github.com/Newtypeman-1/streaming-platform-1/commit/001-03)
- 작업: POST/GET /api/broadcasts, GET /api/broadcasts/{id}, GET /api/broadcasts/{id}/video(Range 206 지원), SeaORM 마이그레이션
- 이유: 업로드는 디스크로 바로 스트리밍(최대 2GB)하고, 실패하면 저장한 파일을 지워 고아 파일을 남기지 않음
- 결과: 통합 테스트 11개 통과(Testcontainers PostgreSQL), 재시작 후 조회·재생 검증

### 001-04 Dioxus 웹 클라이언트, 목록·상세(재생)·등록 화면
- [링크](https://github.com/Newtypeman-1/streaming-platform-1/commit/001-04)
- 작업: `/` 목록, `/broadcasts/:id` 상세 재생, `/new` 업로드 화면
- 이유: 브라우저 File을 FormData로 그대로 보내 WASM 메모리에 올리지 않음, 개발 서버 프록시로 CORS 설정 생략
- 결과: 테스트 영상 업로드 후 자동 재생, 공백 제목 오류 표시, DB 재시작 후에도 목록 유지와 탐색 재생 직접 확인

### 001-05 README, 실행 방법과 직접 확인 절차
- [링크](https://github.com/Newtypeman-1/streaming-platform-1/commit/001-05)
- 작업: 실행 방법, 환경변수, 1단계 범위, 직접 확인 절차 정리
- 이유: 처음 받은 사람도 같은 순서로 실행하고 확인할 수 있게 함
- 결과: README.md 추가

### 001-06 API 서버 기본 포트 8080 → 8000
- [링크](https://github.com/Newtypeman-1/streaming-platform-1/commit/001-06)
- 작업: 서버 기본값, .env.example, Dioxus 프록시 주소, README 포트 변경
- 이유: 8080은 다른 로컬 프로그램과 dx serve 기본 포트와 자주 겹침
- 결과: 서버·환경변수 예시·프록시·문서의 포트를 8000으로 통일

### 001-07 작업내역 문서 WORKLOG.md 추가
- [링크](https://github.com/Newtypeman-1/streaming-platform-1/commit/001-07)
- 작업: 커밋별 번호·링크·작업·이유·결과를 적는 WORKLOG.md 추가
- 이유: README는 실행 안내로 두고, 커밋 단위 기록은 별도 문서로 공개
- 결과: 001-01~001-07 항목 작성

## 2단계: 휴대폰 라이브 송출과 시청

### 002-01 웹 개발 서버 포트 3000 → 3300
- [링크](https://github.com/Newtypeman-1/streaming-platform-1/commit/002-01)
- 작업: README의 웹 실행 포트와 접속 주소를 3300으로 변경
- 이유: 3000번은 Node.js 개발 서버가 쓰고 있어 겹침
- 결과: README의 `dx serve --port`와 접속 주소가 3300으로 통일됨. 3300에서 웹이 뜨고 `/api` 프록시가 동작하는 것 확인

### 002-02 라이브 중계 서버 MediaMTX와 휴대폰용 HTTPS(Caddy) 구성
- [링크](https://github.com/Newtypeman-1/streaming-platform-1/commit/002-02)
- 작업: docker-compose에 MediaMTX 1.21.4와 Caddy(`phone` 프로필) 추가, `mediamtx.yml`·`Caddyfile` 작성, `.env.example`에 `PUBLIC_HOST`·`MEDIAMTX_URL`·`MEDIAMTX_API_URL` 추가, README 환경변수·실행 순서 갱신
- 이유: 휴대폰 브라우저 WebRTC 송출을 중계할 미디어 서버가 필요함. 브라우저는 HTTPS(또는 localhost)에서만 카메라를 허용하므로 휴대폰용 HTTPS 주소도 필요함. MediaMTX 신호(8889)·API(9997) 포트는 127.0.0.1에만 열어 API 서버만 쓰게 함
- 결과: MediaMTX 기동 후 ffmpeg WHIP 송출 시 API에서 `ready: true` 확인. `https://<LAN IP>:3443` 접속 200 확인(IP 접속은 SNI가 없어 `default_sni` 지정이 필요했음)
