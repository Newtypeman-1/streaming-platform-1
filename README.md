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

### 구현 범위

- 휴대폰 브라우저에서 **카메라·마이크로 라이브를 시작하고 종료**합니다. 앱 설치는 필요 없습니다.
  - 전면/후면 카메라를 방송 시작 전에 바꿀 수 있습니다.
- 시청자는 **방송 목록에서 라이브를 골라 시청**합니다. 방송 중인 라이브가 목록 맨 앞에 옵니다.
- **방송 중 · 종료 상태를 구분해 표시**합니다. 목록과 상세에 `LIVE` / `종료` / `녹화` 배지가 붙습니다.
  - 시청 화면은 3초마다 상태를 확인합니다. 방송이 끝나면 "방송이 종료되었습니다"로 바뀝니다.
- 방송은 이렇게 종료됩니다.
  - 송출 화면에서 **방송 종료**를 누를 때
  - 송출 화면을 떠나거나 탭을 닫을 때
  - 네트워크가 끊기거나 브라우저가 강제 종료되어 영상이 끊길 때. 서버가 5초마다 미디어 서버를 확인해 자동으로 종료합니다. 시작 직후 30초는 연결을 기다립니다.
- 로그인과 권한 검사는 없습니다. 대신 라이브를 시작할 때 **송출 키**를 발급합니다. 송출과 종료는 이 키를 가진 화면만 할 수 있습니다. 키는 목록이나 상세 응답에 나오지 않습니다.
- 라이브는 녹화하지 않습니다. 종료된 라이브는 다시 볼 수 없고 종료 기록만 남습니다.

**이번 단계에서 하지 않는 것:** 로그인·권한, 댓글(채팅), 도네이션, 라이브 녹화(다시보기), 시청자 수, 외부 송출 프로그램(OBS) 연동, TURN 서버(다른 네트워크 간 송출)

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

## 1단계 직접 확인 절차 (녹화 방송)

1. 위 순서대로 PostgreSQL, API 서버, 웹을 실행하고 http://localhost:3300 에 접속합니다.
   - 처음에는 "아직 방송이 없습니다"가 보입니다.
2. 오른쪽 위 **녹화 등록**을 눌러 제목을 입력하고 mp4 영상을 선택한 뒤 **등록**을 누릅니다.
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

## 2단계 직접 확인 절차 (휴대폰 라이브)

### 휴대폰 접속 환경

- 휴대폰과 이 컴퓨터가 **같은 와이파이**에 있어야 합니다. 영상은 휴대폰에서 `PUBLIC_HOST:8189`로 바로 갑니다. 다른 네트워크(LTE 등)에서 송출하려면 TURN 서버가 필요합니다. 이번 단계 범위가 아닙니다.
- 브라우저는 **HTTPS나 localhost에서만 카메라·마이크를 허용**합니다. 그래서 휴대폰은 `http://<IP>:3300`이 아니라 Caddy가 여는 **`https://<PUBLIC_HOST>:3443`** 으로 접속합니다.
  - 인증서는 Caddy가 직접 발급한 사설 인증서입니다. 처음 접속할 때 "연결이 비공개로 설정되어 있지 않습니다"(Chrome) 또는 "이 연결은 비공개 연결이 아닙니다"(Safari) 경고가 뜹니다.
    - Android Chrome: **고급 → (주소)(으)로 이동(안전하지 않음)**
    - iPhone Safari: **세부사항 보기 → 이 웹 사이트 방문**
- macOS 방화벽을 켰다면 Docker가 들어오는 연결(3443, 8189)을 받도록 허용해야 합니다.
- 지원 브라우저: Android Chrome, iPhone Safari(iOS 15 이상). 시청은 PC 브라우저도 됩니다.

### 카메라·마이크 권한

- 송출 화면에서 **카메라 켜기**를 누르면 브라우저가 카메라와 마이크 권한을 묻습니다. 둘 다 **허용**합니다.
- 실수로 거부했다면 아래처럼 다시 허용하고 페이지를 새로고침합니다.
  - Android Chrome: 주소창 왼쪽 아이콘 → **권한** → 카메라·마이크 허용
  - iPhone Safari: 주소창의 **가가** → **웹 사이트 설정** → 카메라·마이크 **허용**. 또는 설정 앱 → Safari → 카메라/마이크
- 권한이 없거나 HTTP로 접속했으면 화면에 이유가 표시됩니다. 예: "HTTPS 주소로 접속해야 카메라·마이크를 쓸 수 있습니다."

### 절차

1. 컴퓨터에서 `.env`의 `PUBLIC_HOST`를 LAN IP로 맞추고 다음을 실행합니다.
   ```bash
   docker compose --profile phone up -d
   ```
   ```bash
   cargo run -p server
   ```
   ```bash
   cd web && dx serve --port 3300
   ```
2. **휴대폰(송출)**: `https://<PUBLIC_HOST>:3443` 에 접속합니다. 인증서 경고를 넘기고 오른쪽 위 **라이브 시작**을 누릅니다.
3. **카메라 켜기**를 누르고 권한을 허용합니다. 미리보기가 나오면 필요에 따라 **후면 카메라로**를 누릅니다. 제목을 입력하고 **방송 시작**을 누릅니다.
4. **송출 시작 확인**
   - 휴대폰 화면 왼쪽 위에 `LIVE`, 오른쪽 위에 "송출 연결 중..."이 뜹니다. 1~3초 뒤 **"방송 중"** 으로 바뀌면 송출이 시작된 것입니다.
   - API로도 확인할 수 있습니다. `status`가 `live`이면 방송 중입니다.
     ```bash
     curl -s localhost:8000/api/broadcasts
     ```
   - 미디어 서버에 영상이 들어오는지는 MediaMTX API로 확인합니다. `"name":"live/<id>"`, `"ready":true`가 보이면 됩니다.
     ```bash
     curl -s localhost:9997/v3/paths/list
     ```
5. **다른 화면(시청)**: PC 브라우저에서 http://localhost:3300 을 엽니다. 다른 휴대폰이라면 `https://<PUBLIC_HOST>:3443` 을 엽니다.
   - 목록 맨 앞에 `LIVE` 배지가 붙은 방송이 있습니다. 누르면 1~2초 안에 휴대폰 카메라 영상이 재생됩니다.
   - 자동 재생 정책 때문에 소리 없이 시작합니다. 플레이어의 음소거를 풀면 소리가 들립니다.
   - 휴대폰 송출 화면의 **시청 화면 열기 ↗**로도 같은 페이지를 열 수 있습니다.
6. **송출 종료 확인**: 휴대폰에서 **방송 종료**를 누릅니다.
   - 휴대폰에 "방송이 종료되었습니다"와 종료 시각이 표시됩니다.
   - 시청 화면은 3초 안에 "방송이 종료되었습니다"로 바뀝니다. 배지는 `종료`가 됩니다.
   - 목록에서 그 방송은 `종료` 배지와 함께 라이브들 뒤로 이동합니다. API의 `status`는 `ended`, `endedAt`에 종료 시각이 들어갑니다.
7. (선택) **비정상 종료 확인**: 다시 라이브를 시작한 뒤 휴대폰 브라우저 탭을 닫거나 비행기 모드를 켭니다.
   - 탭을 닫으면 곧바로 종료됩니다.
   - 네트워크가 끊기면 서버가 몇 초 안에 영상 끊김을 감지해 종료합니다. 시작 후 30초 안이라면 30초가 지난 뒤 종료됩니다.

### 휴대폰 없이 확인하기

ffmpeg 7.1 이상은 WHIP 송출을 지원합니다. 휴대폰 대신 테스트 영상을 송출할 수 있습니다.

```bash
curl -s -X POST localhost:8000/api/broadcasts/live -H 'content-type: application/json' -d '{"title":"ffmpeg 테스트"}'
```

응답의 `broadcast.id`와 `streamKey`를 아래 명령에 넣습니다. 그다음 http://localhost:3300 에서 시청합니다.

```bash
ffmpeg -re -f lavfi -i testsrc=size=640x360:rate=30 -f lavfi -i sine=frequency=440 -c:v libx264 -profile:v baseline -pix_fmt yuv420p -bf 0 -g 30 -tune zerolatency -c:a libopus -ar 48000 -ac 2 -authorization <streamKey> -f whip http://localhost:8000/api/broadcasts/<id>/whip
```

`Ctrl+C`로 ffmpeg를 끄면 서버가 영상 끊김을 감지해 종료 상태로 바꿉니다.

### 문제 해결

| 증상 | 확인할 것 |
| --- | --- |
| "HTTPS 주소로 접속해야..." | `http://` 가 아니라 `https://<PUBLIC_HOST>:3443` 으로 접속했는지 |
| `https://...:3443` 접속 불가 | `docker compose --profile phone up -d` 를 실행했는지, `dx serve --port 3300` 이 떠 있는지, 같은 와이파이인지 |
| "송출 연결 중..."에서 멈춘 뒤 실패 | `.env`의 `PUBLIC_HOST`가 현재 LAN IP와 같은지. 바꿨다면 `docker compose up -d` 를 다시 실행 |
| "방송 서버에 연결하지 못했습니다" | MediaMTX 컨테이너가 떠 있는지 (`docker compose ps`) |
