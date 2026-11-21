# streaming-platform

트위치를 레퍼런스로 한 인터넷 방송 플랫폼입니다. 단계별로 기능을 넓혀 가며, 최종 목표는 웹·Android·iOS를 지원하는 실서비스입니다.

| 구성 | 기술 |
| --- | --- |
| 언어 | Rust (Cargo 워크스페이스) |
| 백엔드 | axum 0.8, SeaORM 2.0, tokio |
| DB | PostgreSQL 17 (Docker Compose) |
| 프론트엔드 | Dioxus 0.7 (웹, WASM) |
| 라이브 | 브라우저 WebRTC (WHIP 송출 / WHEP 시청), MediaMTX 1.21 |
| 테스트 | cargo test, Testcontainers(PostgreSQL) |

```
.
├── server/             axum API 서버 (+ DB 마이그레이션)
├── web/                Dioxus 웹 클라이언트
├── shared/             서버와 웹이 함께 쓰는 API 타입
├── docker-compose.yml  로컬 PostgreSQL, MediaMTX(라이브 중계), Caddy(휴대폰용 HTTPS)
├── mediamtx.yml        MediaMTX 설정
├── Caddyfile           휴대폰용 HTTPS 프록시 설정
├── .env.example        환경변수 예시
└── storage/videos/     업로드된 영상 저장 위치 (git 제외, 실행 시 자동 생성)
```

## 1단계: 인증·인가 없는 녹화방송

### 구현 범위

- 녹화 영상 파일과 제목을 **업로드해서 등록**합니다.
  - 영상은 로컬 디스크에 저장하고, DB에는 방송 정보와 영상 위치(저장 키)를 저장합니다.
- **방송 목록**(최신 등록순)과 **상세 화면**을 제공하고, 상세 화면에서 영상을 재생합니다.
- 영상 응답은 HTTP Range 요청(206 Partial Content)을 지원하므로 재생 중 탐색(seek)이 됩니다.
- 입력 검증
  - 제목은 앞뒤 공백을 제거합니다. 비어 있거나 100자(글자 수 기준)를 넘으면 안 됩니다.
  - 영상 파일이 없거나 비어 있으면 등록할 수 없습니다. `video/*` 타입이 아니어도 마찬가지입니다.
  - 등록에 실패하면 이미 저장한 영상 파일은 지웁니다.
- 서버나 DB를 재시작해도 등록 정보는 남아 있습니다(PostgreSQL 볼륨 + `storage/videos`).

**이번 단계에서 하지 않는 것:** 인증·인가, 라이브 송출, 댓글(채팅), 도네이션, 방송 수정·삭제, 트랜스코딩(HLS 등)

### API

| Method | Path | 설명 |
| --- | --- | --- |
| `POST` | `/api/broadcasts` | 방송 등록 (`multipart/form-data`: `title`, `video`) → `201 Created` |
| `GET` | `/api/broadcasts` | 방송 목록 (최신순) |
| `GET` | `/api/broadcasts/{id}` | 방송 상세 |
| `GET` | `/api/broadcasts/{id}/video` | 영상 스트리밍 (Range 지원) |

응답 예시:

```json
{ "id": 1, "title": "첫 방송", "videoUrl": "/api/broadcasts/1/video", "createdAt": "2026-11-19T01:48:04.295021+00:00" }
```

오류 응답은 `{ "status": 400, "detail": "제목을 입력해주세요." }` 형식입니다. 잘못된 입력에는 400, 없는 방송에는 404를 돌려줍니다.

## 2단계: 휴대폰 라이브 송출과 시청

### 송출 방식

```
휴대폰 브라우저 ──(SDP, HTTPS)──▶ API 서버 ──▶ MediaMTX ◀──(SDP)── API 서버 ◀── 시청자 브라우저
       └──────────── 영상·음성 (WebRTC, UDP/TCP 8189) ─────▶ MediaMTX ───────────▶ 시청자 브라우저
```

- 브라우저 표준 WebRTC로 송출(WHIP)하고 시청(WHEP)합니다. 지연은 1초 안팎입니다.
- 중계는 미디어 서버 [MediaMTX](https://github.com/bluenviron/mediamtx)가 맡습니다. Docker로 실행합니다.
- 접속 정보(SDP)는 항상 API 서버를 거칩니다. API 서버가 송출 키와 방송 상태를 확인한 뒤 MediaMTX에 전달합니다. 그래서 MediaMTX의 신호 포트(8889)와 API 포트(9997)는 `127.0.0.1`에만 열립니다. 외부에는 영상 포트(8189)만 열립니다.

### API (추가)

| Method | Path | 설명 |
| --- | --- | --- |
| `POST` | `/api/broadcasts/live` | 라이브 시작 (`{"title": "..."}`) → `201` `{ broadcast, streamKey }` |
| `POST` | `/api/broadcasts/{id}/whip` | 송출 연결. `Authorization: Bearer <streamKey>`, 본문 SDP offer → `201` SDP answer |
| `POST` | `/api/broadcasts/{id}/whep` | 시청 연결. 본문 SDP offer → `201` SDP answer |
| `POST` | `/api/broadcasts/{id}/end` | 라이브 종료. `Authorization: Bearer <streamKey>` → `200`. 이미 종료된 방송이면 그대로 돌려줍니다 |

방송 응답에 `status`(`recorded` / `live` / `ended`)와 `endedAt`이 추가되었습니다. 라이브 방송은 `videoUrl`이 `null`입니다.

| 상황 | 응답 |
| --- | --- |
| 송출 키가 없거나 틀림 | `401` |
| 종료된 방송에 송출·시청 | `409` "방송 중이 아닙니다." |
| 방송 중이지만 아직 영상이 들어오지 않음 | `409` "아직 방송 영상이 들어오지 않았습니다." (시청 화면이 3초마다 다시 시도) |
| 미디어 서버에 연결할 수 없음 | `502` |

## 실행 방법

### 준비물

- Rust (stable). rustup으로 설치하고 WASM 타깃을 추가합니다.
  ```bash
  rustup target add wasm32-unknown-unknown
  ```
- Dioxus CLI 0.7
  ```bash
  cargo install dioxus-cli@0.7.10
  ```
  더 빠른 방법: `cargo binstall dioxus-cli@0.7.10`
- Docker (Docker Desktop 등)

> Homebrew로 `rustup`을 설치했다면 `cargo`가 PATH에 없을 수 있습니다. 쉘 설정에 아래 줄을 추가합니다.
> `export PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH"`

### 1) 환경변수 설정

```bash
cp .env.example .env
```

`.env`의 `change-me`를 원하는 비밀번호로 바꿉니다. `POSTGRES_PASSWORD`와 `DATABASE_URL` 안의 비밀번호를 **같은 값으로** 바꿔야 합니다. `.env`는 git에 올라가지 않습니다.

| 변수 | 용도 | 예시 |
| --- | --- | --- |
| `POSTGRES_DB` | 생성할 DB 이름 | `streaming` |
| `POSTGRES_USER` / `POSTGRES_PASSWORD` | DB 계정 | `streaming` / `change-me` |
| `POSTGRES_PORT` | 호스트에 열 PostgreSQL 포트 | `5432` |
| `DATABASE_URL` | 서버가 접속할 DB 주소 | `postgres://streaming:change-me@localhost:5432/streaming` |
| `SERVER_PORT` | API 서버 포트 | `8000` |
| `VIDEO_STORAGE_DIR` | 영상 저장 디렉터리 (상대경로는 실행 위치 기준) | `storage/videos` |
| `PUBLIC_HOST` | 휴대폰이 접속할 이 컴퓨터의 LAN IP. MediaMTX 영상 주소와 HTTPS 프록시가 씁니다 | `192.168.0.10` |
| `MEDIAMTX_URL` | API 서버가 접속할 MediaMTX WebRTC 주소 | `http://localhost:8889` |
| `MEDIAMTX_API_URL` | API 서버가 접속할 MediaMTX Control API 주소 | `http://localhost:9997` |

서버는 실행 위치부터 상위 폴더로 올라가며 `.env`를 찾아 읽습니다. 그래서 따로 `export`하지 않아도 됩니다.

LAN IP는 macOS에서 아래 명령으로 확인합니다. 와이파이를 바꾸면 IP도 바뀝니다. 그때는 `.env`를 고치고 `docker compose up -d`를 다시 실행합니다.

```bash
ipconfig getifaddr en0
```

> 이미 사용 중인 포트가 있으면 `.env`에서 `POSTGRES_PORT`, `DATABASE_URL`, `SERVER_PORT`를 바꿉니다. `SERVER_PORT`를 바꿨다면 `web/Dioxus.toml`의 `[[web.proxy]] backend` 주소도 같은 포트로 맞춥니다.

### 2) PostgreSQL · MediaMTX 실행

```bash
docker compose up -d
```

휴대폰으로 송출하려면 HTTPS 프록시도 함께 켭니다. 이유는 아래 "휴대폰 접속 환경"에 있습니다.

```bash
docker compose --profile phone up -d
```

### 3) API 서버 실행 (기본 http://localhost:8000)

프로젝트 루트에서 실행합니다. DB 테이블은 서버가 뜰 때 마이그레이션으로 자동 생성됩니다.

```bash
cargo run -p server
```

### 4) 웹 실행 (http://localhost:3300)

`dx serve`의 기본 포트(8080)는 다른 프로그램과 자주 겹칩니다. 3000번은 Node.js 개발 서버와 겹칩니다. 그래서 3300번을 씁니다.

```bash
cd web && dx serve --port 3300
```

개발 서버가 `/api` 요청을 API 서버로 프록시하므로 CORS 설정은 필요 없습니다.

### 테스트

```bash
cargo test -p server
```

Testcontainers가 PostgreSQL 컨테이너를 띄우므로 Docker가 실행 중이어야 합니다. 라이브 테스트(`server/tests/live.rs`)는 가짜 MediaMTX를 띄우므로 MediaMTX 컨테이너는 필요 없습니다.

## 직접 확인 절차

1. 위 순서대로 PostgreSQL, API 서버, 웹을 실행하고 http://localhost:3300 에 접속합니다.
   - 처음에는 "아직 등록된 방송이 없습니다"가 보입니다.
2. 오른쪽 위 **방송 등록**을 눌러 제목을 입력하고 mp4 영상을 선택한 뒤 **등록**을 누릅니다.
   - 테스트용 영상이 없다면 ffmpeg로 만들 수 있습니다.
     ```bash
     ffmpeg -f lavfi -i testsrc=duration=8:size=640x360:rate=30 -pix_fmt yuv420p sample.mp4
     ```
3. 등록하면 상세 화면으로 이동하고 영상이 재생됩니다. 재생 바를 움직여 탐색이 되는지 확인합니다.
4. 왼쪽 위 로고를 눌러 목록으로 갑니다.
   - 방금 등록한 방송이 맨 위에 있어야 합니다.
   - 카드를 누르면 상세 화면으로 가야 합니다.
5. **재시작 확인**
   - API 서버를 `Ctrl+C`로 종료합니다.
   - DB도 재시작합니다.
     ```bash
     docker compose stop && docker compose start
     ```
   - 다시 `cargo run -p server`로 서버를 실행합니다.
   - 목록에 방송이 그대로 있고 상세 화면에서 재생되면 성공입니다.
6. (선택) 잘못된 입력을 확인합니다.
   - 등록 화면에서 제목을 공백만 넣고 등록하면 "제목을 입력해주세요."가 표시됩니다.
   - 실패한 요청의 영상은 `storage/videos`에 남지 않습니다.

> 데이터를 완전히 지우려면 `docker compose --profile phone down -v`로 DB 볼륨을 삭제하고 `storage/` 폴더를 지웁니다.
