# streaming-platform

트위치를 레퍼런스로 한 인터넷 방송 플랫폼입니다. 단계별로 기능을 넓혀 가며, 최종 목표는 웹·Android·iOS를 지원하는 실서비스입니다.

| 구성 | 기술 |
| --- | --- |
| 언어 | Rust (Cargo 워크스페이스) |
| 백엔드 | axum 0.8, SeaORM 2.0, tokio |
| DB | PostgreSQL 17 (Docker Compose) |
| 프론트엔드 | Dioxus 0.7 (웹, WASM) |
| 라이브 | 브라우저 WebRTC (WHIP 송출 / WHEP 시청), MediaMTX 1.21 |
| 결제 | 토스페이먼츠 주문서형 결제(SDK v2)·결제 API (테스트 키) |
| 배포(로컬) | Docker 이미지, kind(Kubernetes), HPA + metrics-server |
| 배포(클라우드) | OCI Always Free VM(Ubuntu, arm64), Docker Compose, Caddy(HTTPS, Let's Encrypt) |
| 데스크톱 | Dioxus 0.7 desktop (WebView), dx bundle(dmg·NSIS), GitHub Actions |
| 부하 | k6 (Docker) |
| 테스트 | cargo test, Testcontainers(PostgreSQL) |

```
.
├── server/             axum API 서버 (+ DB 마이그레이션)
├── web/                Dioxus 웹 클라이언트
├── desktop/            Dioxus 데스크톱 클라이언트 (Windows·macOS)
├── shared/             서버와 클라이언트가 함께 쓰는 API 타입
├── docker-compose.yml  로컬 PostgreSQL, MediaMTX(라이브 중계), Caddy(휴대폰용 HTTPS)
├── mediamtx.yml        MediaMTX 설정
├── Caddyfile           휴대폰용 HTTPS 프록시 설정
├── Dockerfile          API 서버 컨테이너 이미지
├── deploy/             로컬 Kubernetes 클러스터(kind)·HPA 설정, 부하·확인 스크립트
│   └── oci/            OCI VM 배포(compose·Caddy·VM 준비·배포 스크립트), 다운로드 페이지
├── .github/workflows/  데스크톱 설치 파일 빌드·Release
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

## 3단계: 로그인·권한과 댓글

### 구현 범위

- **회원가입·로그인·로그아웃**
  - 아이디는 영문 소문자·숫자·밑줄(`_`) 3~20자입니다. 대문자로 입력하면 소문자로 바꿉니다. 비밀번호는 8~64자입니다.
  - 비밀번호는 Argon2id 해시로만 저장합니다. 응답에는 비밀번호도 해시도 나오지 않습니다.
  - 가입하면 바로 로그인됩니다. 로그인 상태는 30일 유지됩니다.
  - 로그인하면 서버가 세션 토큰을 `HttpOnly; SameSite=Lax` 쿠키로 줍니다. JS에서 읽을 수 없고, 다른 사이트에서 보낸 POST에는 실리지 않습니다. DB에는 토큰의 SHA-256만 저장합니다.
  - 로그아웃하면 그 세션을 DB에서 지웁니다. 같은 쿠키를 다시 보내도 거절됩니다. 다른 기기의 로그인은 유지됩니다.
  - 아이디가 없을 때와 비밀번호가 틀렸을 때 같은 메시지를 줍니다.
- **방송 소유자와 권한**
  - 녹화 방송 등록과 라이브 시작은 로그인해야 합니다. 등록·시작한 사용자가 그 방송의 소유자가 됩니다. 방송 응답의 `owner`에 소유자가 나옵니다.
  - 방송 제목 수정·삭제와 라이브 종료는 **소유자만** 할 수 있습니다. 서버가 요청마다 로그인 쿠키로 사용자를 확인하고 방송의 소유자와 비교합니다. 화면에서 버튼을 숨기는 것과 별개로, API를 직접 호출해도 거절됩니다.
  - 라이브 송출(WHIP)은 2단계처럼 송출 키로 확인합니다. 송출 키는 라이브를 시작한 소유자에게만 한 번 발급되므로 다른 사용자는 송출할 수 없습니다. 브라우저가 아닌 송출 프로그램(ffmpeg 등)도 쓸 수 있도록 쿠키 대신 키를 씁니다.
  - 라이브 종료는 2단계의 송출 키 대신 **소유자 로그인**으로 바뀌었습니다. 송출 키만으로는 종료할 수 없습니다.
  - 방송 중인 라이브는 삭제할 수 없습니다(`409`). 먼저 종료해야 합니다. 녹화 방송을 삭제하면 영상 파일도 지웁니다.
  - 2단계 이전에 만든 방송은 소유자가 없습니다(`owner: null`). 이런 방송은 아무도 수정·삭제·종료할 수 없습니다.
- **댓글**
  - 로그인한 사용자는 녹화·라이브·종료된 방송 어디에나 댓글을 쓸 수 있습니다. 내용은 앞뒤 공백을 빼고 1~500자입니다.
  - 방송별 댓글 목록은 누구나 볼 수 있습니다. 오래된 순으로 나옵니다. 각 댓글에 작성자가 나옵니다.
  - 댓글 수정·삭제는 **작성자만** 할 수 있습니다. 방송 소유자라도 남의 댓글은 고치거나 지울 수 없습니다. 수정하면 `updatedAt`이 남습니다.
  - 방송을 지우면 그 방송의 댓글도 함께 지워집니다.
  - 라이브 중 댓글은 실시간으로 밀어주지 않고, 시청 화면이 3초마다 목록을 다시 불러옵니다. 실시간 채팅(WebSocket)은 이번 단계 범위가 아닙니다.

- **웹 화면**
  - 오른쪽 위에서 로그인·회원가입·로그아웃합니다. 로그인하면 아이디가 표시됩니다.
  - 녹화 등록·라이브 시작 화면은 로그인하지 않으면 "로그인이 필요합니다"를 보여 줍니다.
  - 상세 화면에서 방송 소유자에게만 **제목 수정**·**삭제** 버튼이, 댓글 작성자에게만 그 댓글의 **수정**·**삭제** 버튼이 보입니다. 버튼을 숨기는 것은 편의일 뿐이고, 권한은 서버가 검사합니다.

**이번 단계에서 하지 않는 것:** 비밀번호 찾기·변경, 회원 탈퇴, 이메일 인증, 소셜 로그인, 로그인 시도 횟수 제한, 방송 소유자의 댓글 관리(남의 댓글 삭제·차단), 실시간 채팅(WebSocket), 비공개 방송, 모바일 앱용 토큰 인증

### 공개 범위

| 기능 | 비로그인 | 로그인 (다른 사용자) | 로그인 (소유자·작성자) |
| --- | --- | --- | --- |
| 방송 목록·상세 조회, 녹화 영상 재생, 라이브 시청 | O | O | O |
| 녹화 방송 등록, 라이브 시작 | X `401` | O | O |
| 방송 제목 수정·삭제, 라이브 종료 | X `401` | X `403` | O |
| 댓글 목록 조회 | O | O | O |
| 댓글 작성 | X `401` | O | O |
| 댓글 수정·삭제 | X `401` | X `403` (방송 소유자도) | O (작성자) |
| 라이브 송출(WHIP) | 송출 키가 있어야 함 (`401`) | 송출 키가 있어야 함 (`401`) | O (발급받은 송출 키) |

- 목록·상세·시청은 **누구나** 볼 수 있습니다. 트위치처럼 방송은 공개가 기본이라 로그인하지 않아도 둘러볼 수 있게 했습니다. 비공개 방송은 이번 단계 범위가 아닙니다.
- 로그인하지 않은 요청은 `401` "로그인이 필요합니다.", 로그인했지만 권한이 없으면 `403`을 돌려줍니다. 권한 검사는 없는 방송인지(`404`) 확인한 다음에 합니다.
- JSON 본문이 아니거나 필드가 맞지 않는 요청도 다른 오류와 같은 형식으로 `400` "요청 본문(JSON)이 올바르지 않습니다."를 돌려줍니다.

### API (추가)

| Method | Path | 로그인 | 설명 |
| --- | --- | --- | --- |
| `POST` | `/api/auth/signup` | - | 가입 (`{"username", "password"}`) → `201` 사용자 정보 + 로그인 쿠키. 아이디 중복 `409`, 규칙 위반 `400` |
| `POST` | `/api/auth/login` | - | 로그인 → `200` 사용자 정보 + 로그인 쿠키. 틀리면 `401` |
| `POST` | `/api/auth/logout` | - | 로그아웃 → `204`, 쿠키 삭제 |
| `GET` | `/api/auth/me` | 필요 | 내 정보 `{ "id", "username" }`. 로그인하지 않았으면 `401` |
| `PATCH` | `/api/broadcasts/{id}` | 소유자 | 제목 수정 (`{"title": "..."}`) → `200` |
| `DELETE` | `/api/broadcasts/{id}` | 소유자 | 방송 삭제 → `204`. 방송 중이면 `409` |
| `GET` | `/api/broadcasts/{id}/comments` | - | 댓글 목록 (오래된 순) |
| `POST` | `/api/broadcasts/{id}/comments` | 필요 | 댓글 작성 (`{"body": "..."}`) → `201` |
| `PATCH` | `/api/comments/{id}` | 작성자 | 댓글 수정 (`{"body": "..."}`) → `200` |
| `DELETE` | `/api/comments/{id}` | 작성자 | 댓글 삭제 → `204` |

바뀐 API:

| Method | Path | 3단계에서 바뀐 점 |
| --- | --- | --- |
| `POST` | `/api/broadcasts` | 로그인 필요 |
| `POST` | `/api/broadcasts/live` | 로그인 필요 |
| `POST` | `/api/broadcasts/{id}/end` | 송출 키 대신 소유자 로그인으로 확인 |

방송 응답에 `owner`(`{ "id", "username" }` 또는 `null`)가 추가되었습니다.

댓글 응답 예시:

```json
{ "id": 1, "broadcastId": 3, "author": { "id": 2, "username": "viewer" }, "body": "재밌어요", "createdAt": "2026-11-21T05:10:00+00:00", "updatedAt": null }
```

## 4단계: 방송자 후원(도네이션)

### 구현 범위

- **후원 주문**
  - 로그인한 사용자가 방송 상세 화면에서 금액을 골라 그 방송의 방송자(소유자)에게 후원합니다. 금액은 1,000원 ~ 1,000,000원입니다.
  - 받는 사람은 화면이 아니라 서버가 방송 소유자로 정합니다. 소유자가 없는 옛 방송과 자기 방송에는 후원할 수 없습니다(`400`).
  - 서버는 주문번호를 만들어 금액·받는 사람과 함께 **결제 대기**(`pending`)로 저장한 뒤 주문서를 그릴 값을 돌려줍니다.
- **결제(테스트 모드)**
  - 토스페이먼츠 주문서형 결제(SDK v2)를 씁니다. 주문 금액으로 결제수단·약관 UI를 방송 상세 화면 안에 그리고, **결제하기**를 누르면 고른 결제수단의 결제창이 열립니다. 테스트 키(`test_`)를 쓰므로 실제 돈은 빠져나가지 않습니다.
- **서버의 결제 승인 확인**
  - 결제창 인증이 끝나면 화면이 결제창이 넘겨 준 `paymentKey`·`orderId`·`amount`를 서버에 보냅니다. 화면은 아직 "완료"를 보여 주지 않습니다.
  - 서버는 ① 후원한 본인인지 ② 금액이 주문과 같은지 ③ 주문한 방송이 남아 있고 소유자가 주문 때 받는 사람과 같은지 확인한 뒤, 시크릿 키로 결제 서비스에 **승인을 요청**합니다.
  - 승인 응답의 주문번호·금액·상태(`DONE`)가 주문과 같을 때만 후원을 **확정**(`done`)합니다. 화면은 이 응답을 받은 뒤에만 "후원 완료"를 보여 줍니다.
  - 결제 서비스가 거절하면 결제를 한 번 조회해, 이미 승인된 결제(앞선 승인 응답만 끊긴 경우)면 같은 대조를 거쳐 확정합니다. 승인되지 않았으면 사유와 함께 `400`, 실패(`failed`)로 기록합니다. 승인 요청에는 주문번호를 멱등키(`Idempotency-Key`)로 실어, 다시 요청해도 결제 서비스가 같은 결제를 두 번 승인하지 않습니다. 결제 서비스에 연결하지 못하면 승인됐는지 알 수 없으므로 `502`를 주고 결제 대기로 둡니다(아래 승인 통지로 확정될 수 있음).
  - 결제창을 닫거나 주문서에서 **다른 금액 선택**을 누르면 **취소**(`canceled`), 결제창에서 실패하면 **실패**(`failed`)로 기록합니다.
- **승인 통지(웹훅)와 중복 적립 방지**
  - 결제 서비스가 보내는 결제 상태 변경 통지(`PAYMENT_STATUS_CHANGED`)를 받습니다. 승인 후 화면이 닫히거나 승인 응답이 끊겨도 통지로 확정됩니다.
  - 통지에는 서명이 없어 누구나 보낼 수 있으므로, 내용은 믿지 않고 `paymentKey`로 결제 서비스에 **직접 조회**한 결과(`DONE`·주문번호·금액)로 확정합니다.
  - 후원은 주문 하나가 DB 한 행이고, 확정은 "결제 대기인 주문만 `done`으로 바꾸는" 조건부 갱신입니다. 같은 통지가 여러 번 오거나 승인 확인과 통지가 함께 와도 한 번만 확정되고, 합계는 확정된 행의 금액을 더하므로 두 번 적립되지 않습니다. 결제 키(`paymentKey`)는 DB에서 유일해 한 결제가 두 주문에 쓰일 수도 없습니다.
  - 결제 서비스는 `200`을 받을 때까지 통지를 다시 보내므로, 이미 처리했거나 이 서버의 주문이 아닌 통지에도 `200`을 줍니다.
- **방송별 후원 내역과 합계**
  - 확정된 후원만 최근 순으로 후원자·금액·시각과 합계·건수를 보여 줍니다. 결제 대기·실패·취소는 세지 않습니다. 누구나 볼 수 있습니다.
  - 방송을 지워도 후원 기록은 남습니다(방송 연결만 끊김).
- **웹 화면**
  - 방송 상세 화면에 후원 합계·내역과 금액 선택(1,000 / 5,000 / 10,000 / 50,000원 또는 직접 입력), **후원하기** 버튼이 있습니다. 후원하기를 누르면 그 자리에 주문서(결제수단·약관·**결제하기**)가 열립니다. 비로그인이면 로그인 안내, 방송 소유자에게는 후원 버튼이 보이지 않습니다.
  - 결제창이 돌아오는 `/donations/success`(승인 확인 후 "후원 완료"), `/donations/fail`(취소·실패 표시) 화면이 있습니다.

**이번 단계에서 하지 않는 것:** 결제 취소·환불, 방송자 정산·출금, 후원 메시지와 방송 화면 알림, 가상계좌(입금 대기 뒤 확정) 결제, 실시간 후원 갱신, 결제 서비스 장애로 결제 대기에 남은 주문의 주기적 재확인

### 결제 흐름

```
브라우저                         API 서버                          토스페이먼츠
 │ POST /api/broadcasts/{id}/donations ─▶ 주문 저장(pending)
 │ ◀── orderId·amount·clientKey
 │ 주문서(clientKey, amount) → 결제하기(orderId) ────────────────▶ 결제수단 인증
 │ ◀──────────────────────── successUrl?paymentKey&orderId&amount
 │ POST /api/donations/confirm ─────────▶ 본인·금액·받는 방송 확인
 │                                        승인 요청(시크릿 키) ──▶ /v1/payments/confirm
 │                                        응답 대조(주문번호·금액·DONE) → done
 │ ◀── 확정된 주문 → "후원 완료"
 │                                        ◀── 결제 상태 통지(웹훅) ── 
 │                                        조회(시크릿 키) ────────▶ /v1/payments/{paymentKey}
 │                                        결제 대기면 done, 이미 done 이면 그대로
```

### 공개 범위

| 기능 | 비로그인 | 로그인 (다른 사용자) | 로그인 (후원한 본인) |
| --- | --- | --- | --- |
| 방송별 후원 내역·합계 조회 | O | O | O |
| 후원 주문 | X `401` | O (자기 방송은 `400`) | O |
| 결제 승인 확인, 실패·취소 기록 | X `401` | X `403` | O |
| 결제 상태 통지(웹훅) | 누구나 보낼 수 있지만 서버가 결제 서비스에 조회해 확인 | | |

### API (추가)

| Method | Path | 로그인 | 설명 |
| --- | --- | --- | --- |
| `GET` | `/api/broadcasts/{id}/donations` | - | 후원 내역·합계 `{ "total", "count", "donations": [{ "id", "donor", "amount", "approvedAt" }] }`. 확정된 후원만, 최근 순 |
| `POST` | `/api/broadcasts/{id}/donations` | 필요 | 후원 주문 (`{"amount": 5000}`) → `201` 주문. 금액 범위 밖·소유자 없는 방송·자기 방송 `400` |
| `POST` | `/api/donations/confirm` | 후원한 본인 | 결제 승인 확인 (`{"paymentKey", "orderId", "amount"}`) → `200` 확정된 주문. 금액 불일치·결제 서비스 거절 `400`, 끝난 주문·방송 없어짐 `409`, 결제 서비스 연결 실패 `502`. 이미 확정된 주문은 승인을 다시 요청하지 않고 같은 결과 |
| `POST` | `/api/donations/{orderId}/fail` | 후원한 본인 | 결제창 실패·취소 기록 (`{"code", "message"}`) → `200`. `PAY_PROCESS_CANCELED`(모바일 결제창 닫기)·`USER_CANCEL`(PC 결제창 닫기)은 취소, 나머지는 실패. 확정된 주문은 `409` |
| `POST` | `/api/payments/webhook` | - | 결제 서비스의 결제 상태 변경 통지. 조회 결과로 확정 → `200`. 결제 서비스에 없는 결제 `400` |

주문 응답 예시:

```json
{ "orderId": "nadri-670d220b34f84a7d9f64804b68d14386", "broadcastId": 12, "recipient": { "id": 4, "username": "owner4" }, "amount": 5000, "orderName": "나드리 owner4 님 후원", "status": "pending", "clientKey": "test_gck_...", "failure": null }
```

`status`는 `pending`(결제 대기) · `done`(확정) · `failed`(실패) · `canceled`(취소)입니다.

## 5단계: 여러 인스턴스와 자동 스케일링

### 구현 범위

- **컨테이너 이미지**: 저장소 루트의 `Dockerfile`로 API 서버 이미지를 만듭니다(`docker build -t nadri-server:local .`). 빌드 도구 없이 실행 파일과 인증서만 담고, 일반 사용자로 실행합니다.
- **여러 인스턴스 실행**: 로컬 Kubernetes 클러스터(kind, 노드 3개)에서 서버를 Deployment로 띄웁니다. 기본 2개이고 인스턴스는 노드에 고르게 나뉩니다.
- **요청 분산**: Service(NodePort)가 준비된 인스턴스로 요청을 나눠 보냅니다. kind가 이 Service를 이 컴퓨터의 `localhost:8000`으로 열어 주므로 웹 개발 서버의 `/api` 프록시를 바꾸지 않아도 됩니다.
- **자동 스케일 아웃·인**: HPA가 CPU 사용률(요청량 100m 대비)을 보고 **2~8개** 사이에서 인스턴스 수를 조절합니다. 평균 50%를 넘으면 바로 늘리고(15초마다 최대 2배), 1분 동안 계속 낮으면 30초마다 최대 2개씩 줄입니다.
- **상태 확인 API**: `GET /api/health`가 DB에 닿으면 `200`, 아니면 `503`입니다. Kubernetes가 이것으로 요청을 보낼 인스턴스(readiness)와 다시 띄울 인스턴스(liveness)를 고릅니다.
- **스케일 인 중 요청 보호**: 인스턴스는 종료 신호(SIGTERM)를 받으면 새 연결을 받지 않고 처리 중인 요청에 응답한 뒤 끝납니다. Service 목록에서 빠지는 동안 들어오는 요청도 받도록 종료 전에 5초 기다립니다.
- **동시 기동**: 빈 DB에 인스턴스 여러 개가 한꺼번에 떠도 DB 잠금(advisory lock)으로 마이그레이션을 하나씩 적용하므로 기동에 실패하지 않습니다.
- **응답한 인스턴스 표시**: 모든 응답에 `X-Instance` 헤더로 처리한 인스턴스(Pod) 이름이 붙습니다.
- **부하·확인 스크립트**: `deploy/load.sh`(k6), `deploy/watch.sh`(인스턴스 수·CPU), `deploy/check.sh`(로그인·목록·댓글·영상 유지 확인)

**이번 단계에서 하지 않는 것:** 클라우드 배포, 웹 화면(WASM)의 컨테이너화, PostgreSQL·MediaMTX 자체의 다중화, 요청 수 기반 스케일링, 영상의 오브젝트 스토리지 이전, 실시간(WebSocket) 기능

### 구성

```
브라우저 / 웹 개발 서버(3300, /api 프록시)          k6 부하(Docker, kind 네트워크)
        │ localhost:8000                                   │ nadri-control-plane:30800
        ▼                                                  ▼
┌──────────────────────── kind 클러스터 (노드 3개) ────────────────────────┐
│  Service nadri-server (NodePort 30800) ── 준비된 Pod 으로 요청 분산       │
│     ├─ Pod nadri-server-…  ┐                                              │
│     ├─ Pod nadri-server-…  ├─ HPA: CPU 50% 기준 2~8개 (metrics-server)    │
│     └─ … (최대 8)           ┘                                              │
│  모든 노드의 /nadri/videos ── 저장소의 storage/videos (공유 디렉터리)       │
└──────────────────────────────────────────────────────────────────────────┘
        │ host.docker.internal
        ▼
 docker compose: PostgreSQL(5432) · MediaMTX(8889 신호, 9997 API, 8189 영상)
```

### 상태를 두는 곳

인스턴스는 상태를 갖지 않습니다. 어느 인스턴스가 요청을 받아도, 인스턴스가 늘거나 사라져도 결과가 같습니다.

| 상태 | 저장 위치 |
| --- | --- |
| 로그인 세션 | PostgreSQL `session` (쿠키에는 토큰만, DB에는 그 해시) |
| 방송 목록·라이브 상태·송출 키 | PostgreSQL `broadcast` |
| 댓글 | PostgreSQL `comment` |
| 후원 주문·결제 상태 | PostgreSQL `donation` |
| 업로드 영상 파일 | 모든 노드가 함께 보는 디렉터리(`storage/videos` → Pod 의 `/data/videos`) |
| 라이브 영상 중계 | MediaMTX (모든 인스턴스가 같은 MediaMTX에 신호를 보냄) |

끊긴 라이브를 종료하는 확인 작업은 인스턴스마다 5초마다 돕니다. 여러 인스턴스가 함께 돌아도 결과는 "송출이 없는 라이브를 `ended`로" 같은 갱신이라 겹쳐도 문제가 없습니다.

### API (추가)

| Method | Path | 로그인 | 설명 |
| --- | --- | --- | --- |
| `GET` | `/api/health` | - | 상태 확인. DB에 닿으면 `200`(본문 없음), 닿지 않거나 2초 안에 응답이 없으면 `503` |

모든 응답에 `X-Instance: <인스턴스 이름>` 헤더가 붙습니다.

## 6단계: 클라우드 배포와 데스크톱 클라이언트

### 구현 범위

- **클라우드 배포**: OCI Always Free VM 한 대에 PostgreSQL · MediaMTX · API 서버 · Caddy를 Docker Compose로 띄웁니다(`deploy/oci/`). 방송·계정·댓글·후원 기능은 그대로입니다.
- **도메인과 HTTPS**: 도메인의 A 레코드가 VM 공인 IP를 가리키고, Caddy가 Let's Encrypt 인증서를 자동으로 받아 갱신합니다. HTTP로 들어오면 HTTPS로 넘깁니다. 웹 앱, API(`/api`), 다운로드 페이지(`/download/`)를 모두 같은 도메인의 HTTPS로 제공합니다.
- **배포 설정**: 서비스 주소(도메인·공인 IP)와 비밀 값(DB 비밀번호, 결제 키)은 VM의 `/opt/nadri/.env`에서만 읽습니다. 저장소에는 `deploy/oci/.env.example`만 있습니다.
- **상태 확인**: `GET /api/health`(DB 연결 확인, 5단계)로 배포 직후 공개 주소에서 확인합니다(`deploy.sh` 마지막 단계).
- **HTTPS 쿠키**: HTTPS 프록시를 거친 요청(`X-Forwarded-Proto: https`)이면 로그인 쿠키에 `Secure`를 붙입니다. 로컬 개발 서버(`http://localhost`)에서는 붙이지 않습니다.
- **데스크톱 클라이언트**: Windows·macOS 앱(`desktop/`). 방송 목록을 보고 녹화 방송과 라이브를 재생합니다. 서버 호출은 앱(Rust)이 하므로 서버에 CORS를 열지 않습니다.
- **설치 파일**: GitHub Actions가 macOS arm64·x64(`.dmg`)와 Windows x64(NSIS `-setup.exe`)를 빌드해 `desktop-v*` 태그의 Release에 올립니다.
- **다운로드 페이지**: `/download/`에 OS별 링크, 지원 아키텍처, 설치·실행 방법을 보여 줍니다(`deploy/oci/download/index.html`).

**이번 단계에서 하지 않는 것:** 데스크톱 앱의 로그인·댓글·후원·방송하기(웹에서 함), 코드 서명·공증(Apple Developer ID, Windows 인증서), 앱 자동 업데이트, VM 다중화·자동 스케일링(5단계 구성은 로컬 전용), DB 백업 자동화, Linux 데스크톱 패키지

### 공개 주소

| 용도 | 주소 |
| --- | --- |
| 웹 앱 | https://nadri.me |
| API 상태 확인 | https://nadri.me/api/health |
| 데스크톱 다운로드 | https://nadri.me/download/ |
| 설치 파일 Release | https://github.com/Newtypeman-1/streaming-platform-1/releases/tag/desktop-v0.1.0 |

### 도메인

| 항목 | 내용 |
| --- | --- |
| 도메인 | `nadri.me` |
| 등록 여부 | 등록함 (2026-11-22 등록, 2027-11-22 만료) |
| 등록 기관 | Cloudflare Registrar (DNS도 Cloudflare) |
| 최초 등록 비용 | US$16.56 (1년) |
| 갱신 비용 | US$16.56 / 1년 (매년 같음) |

### OCI VM

| 항목 | 내용 |
| --- | --- |
| 홈 리전 | 일본 동부(도쿄), `ap-tokyo-1` |
| 확인한 잔여 한도 | Compute A1 코어·메모리: 서비스 한도 "동적", 사용량 0 (계정에 다른 A1 인스턴스 없음) |
| Shape | `VM.Standard.E2.1.Micro` (AMD, x86_64, 1/8 OCPU, 메모리 1GB) — Always Free |
| 이미지 | Canonical Ubuntu 24.04 (x86_64) |
| 공인 IP | `129.203.181.147` |
| 부트 볼륨 | 기본 크기(약 47GB) |

**A1이 아닌 이유:** 우선 후보는 `VM.Standard.A1.Flex` 2 OCPU·12GB였습니다. 하지만 도쿄 리전에서 A1 생성이 재고 부족(Out of capacity)으로 계속 실패해서, Always Free 범위의 다른 Shape인 `VM.Standard.E2.1.Micro`로 임시 배포했습니다.

**E2.1.Micro를 쓰기 위해 바꾼 것과 제약:**
- 메모리가 1GB라 VM 준비 때 스왑 2GB를 만듭니다. 배포 후 컨테이너 메모리는 서버 17MB, PostgreSQL 27MB, MediaMTX 31MB, Caddy 47MB 정도입니다.
- CPU가 x86_64라 `deploy.sh`가 VM 아키텍처를 읽어 서버 이미지를 `linux/amd64`로 빌드합니다. 이 컴퓨터(arm64)에서는 에뮬레이션으로 빌드하므로 첫 빌드에 약 8분 걸렸습니다.
- 1/8 OCPU라 동시 시청자가 많거나 큰 영상을 올리면 느립니다. 기능 시연에는 충분합니다.

**A1으로 옮기려면:** A1 재고가 풀리면 같은 절차로 A1 VM을 만들고 DNS A 레코드만 새 IP로 바꿉니다. `deploy.sh`가 arm64를 알아서 고릅니다.

**Ubuntu를 고른 이유:**
- VM 준비 스크립트가 Ubuntu 기준입니다(`apt`의 `docker.io`·`docker-compose-v2`, `netfilter-persistent`, OCI Ubuntu 이미지의 기본 iptables 규칙).
- 24.04는 LTS라 2029년까지 보안 업데이트가 나옵니다.
- 서비스는 모두 컨테이너로 돌아서 호스트 OS는 Docker만 띄우면 됩니다.

### 구성

```
사용자 브라우저 · 데스크톱 앱
   │ https://<도메인> (443)                         │ WebRTC 영상 (8189 UDP/TCP)
   ▼                                                ▼
┌──────────────── OCI VM (Ubuntu, Docker Compose) ────────────────┐
│ caddy  :80 → :443 이동, :443 HTTPS(Let's Encrypt)                │
│   ├─ /api/*        → server:8000 (API 서버)                     │
│   ├─ /download/    → 다운로드 페이지                              │
│   ├─ /downloads/*  → 데스크톱 설치 파일                           │
│   └─ 그 밖          → 웹 앱 정적 파일 (하위 경로는 index.html)     │
│ server   ── postgres (내부망만)                                   │
│          └─ mediamtx 신호·제어 (내부망만) ── 영상은 8189로 직접     │
└────────────────────────────────────────────────────────────────┘
```

밖으로 여는 포트는 22(SSH), 80, 443(TCP·UDP), 8189(UDP·TCP)뿐입니다. DB, MediaMTX 제어 API, 서버 포트는 Compose 내부 네트워크에만 있습니다.

### DNS·TLS 구성

1. 도메인 DNS에 A 레코드를 추가합니다: 이름 `@`(루트), 값 VM 공인 IP. Cloudflare DNS라면 프록시를 끄고(**DNS only**, 회색 구름) 씁니다. WebRTC 영상(UDP)은 Cloudflare 프록시를 통과하지 못하고, 인증서는 VM의 Caddy가 직접 받기 때문입니다.
2. 전파를 확인합니다: `dig +short <도메인> @1.1.1.1` 이 VM 공인 IP를 돌려주면 됩니다.
3. VM에서 Caddy가 처음 뜰 때 Let's Encrypt HTTP-01 방식으로 인증서를 받습니다(80·443이 열려 있어야 함). 갱신도 Caddy가 자동으로 합니다. 인증서는 `caddy-data` 볼륨에 남아 재시작해도 다시 받지 않습니다.

### 배포 환경 설정 (`/opt/nadri/.env`)

| 변수 | 설명 |
| --- | --- |
| `NADRI_DOMAIN` | 서비스 도메인. Caddy가 이 이름으로 인증서를 받습니다 |
| `PUBLIC_IP` | VM 공인 IP. MediaMTX가 WebRTC 접속 주소(ICE 후보)로 알려 줍니다 |
| `POSTGRES_DB` · `POSTGRES_USER` · `POSTGRES_PASSWORD` | DB 계정. 비밀번호는 URL에 들어가므로 영문·숫자(`openssl rand -hex 24`) |
| `TOSS_CLIENT_KEY` · `TOSS_SECRET_KEY` | 토스페이먼츠 테스트 키. 비우면 후원만 동작하지 않습니다 |
| `SERVER_TAG` | 실행할 서버 이미지 태그. `deploy.sh`가 커밋 해시로 바꿉니다 |

### 배포 절차 (재현)

1. **VM 만들기**: OCI 콘솔 → 컴퓨트 → 인스턴스 → 인스턴스 생성.
   - Shape는 `VM.Standard.A1.Flex`(2 OCPU, 12GB)이고, 재고가 없으면 `VM.Standard.E2.1.Micro`를 씁니다.
   - 이미지는 Canonical Ubuntu 24.04입니다.
   - 공용 서브넷에 공용 IPv4를 자동 할당하고, SSH 키 쌍을 만들어 개인 키를 저장합니다.
2. **보안 목록**: VCN → Security List → Ingress Rules에 `0.0.0.0/0` TCP 80, TCP 443, UDP 443, UDP 8189, TCP 8189 추가.
3. **SSH 접속 이름**: `~/.ssh/config`에 `Host nadri`(HostName=공인 IP, User=ubuntu, IdentityFile=키)를 적습니다.
4. **VM 준비**(VM 안 iptables 허용, Docker 설치, 스왑 2GB):
   ```bash
   ssh nadri 'bash -s' < deploy/oci/setup-vm.sh
   ```
5. **환경 설정**: 예시를 올리고 VM에서 값을 채웁니다.
   ```bash
   scp deploy/oci/.env.example nadri:/opt/nadri/.env
   ```
   ```bash
   ssh -t nadri 'chmod 600 /opt/nadri/.env && nano /opt/nadri/.env'
   ```
6. **DNS**: 위 "DNS·TLS 구성"대로 A 레코드를 만들고 전파를 확인합니다.
7. **데스크톱 설치 파일**: 태그를 올리면 Actions가 빌드해 Release를 만듭니다(도메인이 `nadri.me`가 아니면 먼저 저장소 변수 `NADRI_SERVER_URL`을 `https://<도메인>`으로 설정).
   ```bash
   git tag desktop-v0.1.0 && git push origin desktop-v0.1.0
   ```
8. **배포**(서버 이미지·웹 빌드 → VM 업로드 → `docker compose up -d` → 공개 주소 상태 확인):
   ```bash
   VM=nadri DESKTOP_RELEASE=desktop-v0.1.0 deploy/oci/deploy.sh
   ```
   다시 배포할 때 설치 파일을 바꾸지 않으면 `DESKTOP_RELEASE`는 빼도 됩니다.

### 데스크톱 클라이언트 개발

```bash
NADRI_SERVER_URL=http://localhost:8000 cargo run -p desktop
```

```bash
cargo test -p desktop
```

접속할 서버는 실행할 때의 `NADRI_SERVER_URL` → 빌드할 때의 `NADRI_SERVER_URL` → `https://nadri.me` 순으로 정합니다. 설치 파일은 `desktop/`에서 `dx bundle --platform desktop --release`로 이 컴퓨터용을 만들 수 있습니다(`target/dx/nadri/bundle/`).

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
| `TOSS_CLIENT_KEY` | 토스페이먼츠 주문서형, 결제창형 연동 **테스트 클라이언트 키** (브라우저에 공개되는 값) | `test_gck_...` |
| `TOSS_SECRET_KEY` | 토스페이먼츠 주문서형, 결제창형 연동 **테스트 시크릿 키** (서버만 사용, 외부 노출 금지) | `test_gsk_...` |
| `TOSS_API_URL` | 결제 API 주소 | `https://api.tosspayments.com` |
| `TOSS_TEST_CODE` | (선택) 결제 승인 실패 재현용 오류 코드. 평소엔 비워 둡니다 | `INVALID_REJECT_CARD` |
| `INSTANCE_NAME` | (선택) 응답의 `X-Instance` 헤더에 넣을 인스턴스 이름. 없으면 `HOSTNAME`(컨테이너에서는 Pod 이름), 그것도 없으면 `local` | `nadri-1` |

서버는 실행 위치부터 상위 폴더로 올라가며 `.env`를 찾아 읽습니다. 그래서 따로 `export`하지 않아도 됩니다.

**토스페이먼츠 테스트 키 발급**: [토스페이먼츠 개발자센터](https://developers.tosspayments.com)에 가입하고 **API 키** 메뉴에서 **주문서형, 결제창형 연동 키**의 테스트 클라이언트 키(`test_gck_`)와 시크릿 키(`test_gsk_`)를 `.env`에 넣습니다. API 개별 연동 키(`test_ck_`·`test_sk_`)는 주문서형 결제에서 쓸 수 없습니다. 테스트 키로는 실제 결제가 일어나지 않습니다. 키 값은 저장소에 올리지 마세요. 키가 없어도 서버는 뜨지만 후원 결제는 할 수 없습니다.

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

### 5) 여러 인스턴스로 실행 (5단계, 3) 대신)

3)의 `cargo run -p server` 대신 서버를 로컬 Kubernetes 클러스터에 여러 인스턴스로 띄웁니다. 클러스터가 `localhost:8000`을 쓰므로 `cargo run`으로 띄운 서버는 먼저 끕니다. 2)의 PostgreSQL·MediaMTX는 그대로 켜 둡니다.

준비물: Docker Desktop, [kind](https://kind.sigs.k8s.io), kubectl

```bash
brew install kind kubectl
```

```bash
./deploy/up.sh
```

`up.sh`가 하는 일:

1. kind 클러스터 `nadri`(컨트롤 플레인 1 + 워커 2)를 만들고 모든 노드에 `storage/videos`를 연결합니다. 이미 있으면 그대로 씁니다.
2. HPA가 CPU 사용률을 읽도록 metrics-server를 설치합니다.
3. 서버 이미지를 빌드해 클러스터 노드에 넣습니다. 첫 빌드는 의존성 컴파일 때문에 몇 분 걸립니다.
4. `.env`의 값으로 Secret `nadri-env`를 만듭니다. 클러스터 안에서는 `DATABASE_URL`·`MEDIAMTX_URL`·`MEDIAMTX_API_URL`의 `localhost`를 `host.docker.internal`로 바꿔 이 컴퓨터의 PostgreSQL·MediaMTX에 접속합니다. 따로 넣을 환경변수는 없습니다.
5. `deploy/k8s/server.yaml`(Deployment·Service·HPA)을 적용하고 인스턴스가 준비될 때까지 기다립니다.

코드를 바꾼 뒤 다시 실행하면 이미지를 새로 빌드해 인스턴스를 차례로 교체합니다. 웹은 4)와 같이 `dx serve`로 띄우면 클러스터의 서버를 씁니다.

클러스터를 지우려면 아래를 실행합니다. DB와 `storage/videos`의 데이터는 남습니다.

```bash
./deploy/down.sh
```

### 테스트

```bash
cargo test -p server
```

Testcontainers가 PostgreSQL 컨테이너를 띄우므로 Docker가 실행 중이어야 합니다. 라이브 테스트(`server/tests/live.rs`)는 가짜 MediaMTX를, 후원 테스트(`server/tests/donations.rs`)는 가짜 결제 API를 띄우므로 MediaMTX 컨테이너나 토스페이먼츠 키는 필요 없습니다.

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

3단계부터는 로그인해야 라이브를 시작할 수 있습니다. 가입(또는 `/api/auth/login`)하면서 받은 쿠키를 `cookies.txt`에 저장해 씁니다.

```bash
curl -s -c cookies.txt localhost:8000/api/auth/signup -H 'content-type: application/json' -d '{"username":"streamer","password":"password123"}'
```

```bash
curl -s -b cookies.txt -X POST localhost:8000/api/broadcasts/live -H 'content-type: application/json' -d '{"title":"ffmpeg 테스트"}'
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

## 3단계 직접 확인 절차 (로그인·권한·댓글)

> 3단계에서 새로 필요한 환경변수는 없습니다. 위 "실행 방법"대로 실행하면 서버가 뜰 때 회원·세션·댓글 테이블과 방송 소유자 컬럼이 마이그레이션으로 추가됩니다. 2단계까지 만든 방송은 소유자가 없는 상태(`owner: null`)로 남습니다.

비로그인 사용자, 방송 소유자, 다른 사용자 세 역할로 확인합니다. 화면 확인(A)과 API 직접 호출 확인(B)을 모두 합니다. B는 화면에 버튼이 없어도 서버가 거절하는지 보는 절차입니다.

### A. 화면에서 확인

브라우저 두 개를 씁니다. 예: 일반 창은 **소유자**, 시크릿 창은 **다른 사용자**. 로그인 쿠키가 창마다 따로 저장됩니다.

1. PostgreSQL, API 서버, 웹을 실행하고 http://localhost:3300 에 접속합니다.
2. **비로그인**
   - 목록과 기존 방송 상세(영상 재생)는 그대로 볼 수 있습니다.
   - **녹화 등록**이나 **라이브 시작**을 누르면 "로그인이 필요합니다."가 나옵니다.
   - 상세 화면 아래 댓글 목록은 보이지만 입력창 대신 "로그인하면 댓글을 쓸 수 있습니다."가 나옵니다. 제목 수정·삭제 버튼도 없습니다.
3. **소유자 가입**: 일반 창에서 오른쪽 위 **로그인 → 회원가입**으로 `owner1` / `password123` 가입합니다. 오른쪽 위에 `owner1`이 표시됩니다.
   - 규칙 확인: 아이디 `ab`나 8자 미만 비밀번호는 오류 메시지가 나옵니다. 같은 아이디로 다시 가입하면 "이미 사용 중인 아이디입니다."가 나옵니다.
4. **소유자가 녹화 방송 등록**: **녹화 등록**으로 영상을 올립니다. 상세 화면에 `owner1 · 시각 시작`이 표시되고, 제목 옆에 **제목 수정**·**삭제** 버튼이 있습니다.
   - **제목 수정**으로 제목을 바꾸고 저장하면 바로 반영됩니다. 새로고침해도 유지됩니다.
   - 댓글을 하나 씁니다. 내 댓글에만 **수정**·**삭제**가 보입니다.
5. **다른 사용자**: 시크릿 창에서 `viewer1` / `password123`으로 가입하고 같은 방송 상세로 갑니다.
   - **제목 수정**·**삭제** 버튼이 없습니다.
   - 댓글을 쓸 수 있습니다. 내 댓글에만 **수정**·**삭제**가 보이고, `owner1`의 댓글에는 없습니다.
   - 내 댓글을 수정하면 시각 옆에 "(수정됨)"이 붙습니다.
6. **소유자 화면에서 확인**: 일반 창을 새로고침하면 `viewer1`의 댓글이 보입니다. 소유자에게도 그 댓글의 수정·삭제 버튼은 없습니다.
7. **라이브**: 일반 창(소유자)에서 2단계 절차대로 라이브를 시작합니다. 시크릿 창(다른 사용자)에서 그 라이브를 시청하며 댓글을 쓰면, 소유자의 시청 화면에 3초 안에 나타납니다.
   - 라이브 중에는 상세 화면에 **삭제** 버튼이 없습니다. 송출 화면에서 **방송 종료**하면 종료됩니다.
8. **삭제**: 소유자가 녹화 방송 상세에서 **삭제**를 누르고 확인하면 목록으로 돌아가고, 그 방송과 댓글이 사라집니다. `storage/videos`에서 영상 파일도 지워집니다.
9. **로그아웃**: 오른쪽 위 **로그아웃**을 누르면 로그인 버튼으로 바뀌고, 다시 녹화 등록 화면은 "로그인이 필요합니다."가 됩니다.

### B. API로 직접 호출해 서버의 권한 검사 확인

화면에서 버튼을 숨기는 것만으로 막고 있지 않은지 확인합니다. `curl`로 세 역할의 요청을 직접 보냅니다. 쿠키는 `-c`로 저장하고 `-b`로 보냅니다. 웹 개발 서버(3300)의 프록시를 거쳐도 같은 결과입니다.

```bash
API=http://localhost:8000
J='content-type: application/json'
# 아이디가 이미 있으면 바꿔서 실행합니다
curl -s -c owner.txt $API/api/auth/signup -H "$J" -d '{"username":"owner2","password":"password123"}'
curl -s -c other.txt $API/api/auth/signup -H "$J" -d '{"username":"other2","password":"password123"}'
```

녹화 방송을 소유자로 하나 만들고 `id`를 적어 둡니다(영상은 1단계의 ffmpeg 명령으로 만든 `sample.mp4`).

```bash
curl -s -o /dev/null -w '%{http_code}\n' $API/api/broadcasts -F title=비로그인 -F 'video=@sample.mp4;type=video/mp4'   # 401
curl -s -b owner.txt $API/api/broadcasts -F title=소유자방송 -F 'video=@sample.mp4;type=video/mp4'                    # 201, owner 가 owner2
ID=<위 응답의 id>
```

각 요청의 상태 코드를 확인합니다. `code`는 상태 코드만 출력하는 함수입니다.

```bash
code() { curl -s -o /dev/null -w '%{http_code}\n' "$@"; }

# 방송 제목 수정: 비로그인 401, 다른 사용자 403, 소유자 200
code -X PATCH $API/api/broadcasts/$ID -H "$J" -d '{"title":"바꿈"}'
code -b other.txt -X PATCH $API/api/broadcasts/$ID -H "$J" -d '{"title":"바꿈"}'
code -b owner.txt -X PATCH $API/api/broadcasts/$ID -H "$J" -d '{"title":"바꿈"}'

# 댓글 작성: 비로그인 401, 다른 사용자 201, 소유자 201
code -X POST $API/api/broadcasts/$ID/comments -H "$J" -d '{"body":"익명"}'
curl -s -b other.txt -X POST $API/api/broadcasts/$ID/comments -H "$J" -d '{"body":"시청자 댓글"}'
code -b owner.txt -X POST $API/api/broadcasts/$ID/comments -H "$J" -d '{"body":"방송자 댓글"}'
CID=<시청자 댓글 응답의 id>

# 댓글 목록: 비로그인도 200
code $API/api/broadcasts/$ID/comments

# 남의 댓글 수정·삭제: 비로그인 401, 방송 소유자라도 403, 작성자 200 / 204
code -X PATCH $API/api/comments/$CID -H "$J" -d '{"body":"수정"}'
code -b owner.txt -X PATCH $API/api/comments/$CID -H "$J" -d '{"body":"수정"}'
code -b other.txt -X PATCH $API/api/comments/$CID -H "$J" -d '{"body":"수정"}'
code -b owner.txt -X DELETE $API/api/comments/$CID
code -b other.txt -X DELETE $API/api/comments/$CID

# 라이브 시작: 비로그인 401, 로그인 201
code -X POST $API/api/broadcasts/live -H "$J" -d '{"title":"라이브"}'
curl -s -b owner.txt -X POST $API/api/broadcasts/live -H "$J" -d '{"title":"라이브"}'
LID=<응답의 broadcast.id>
KEY=<응답의 streamKey>

# 라이브 송출: 송출 키가 없으면 401
code -b other.txt -X POST $API/api/broadcasts/$LID/whip -H 'content-type: application/sdp' -d 'v=0'

# 라이브 종료: 비로그인 401, 다른 사용자 403, 송출 키만으로 401, 방송 중 삭제 409, 소유자 200
code -X POST $API/api/broadcasts/$LID/end
code -b other.txt -X POST $API/api/broadcasts/$LID/end
code -H "Authorization: Bearer $KEY" -X POST $API/api/broadcasts/$LID/end
code -b owner.txt -X DELETE $API/api/broadcasts/$LID
code -b owner.txt -X POST $API/api/broadcasts/$LID/end

# 방송 삭제: 비로그인 401, 다른 사용자 403, 소유자 204
code -X DELETE $API/api/broadcasts/$ID
code -b other.txt -X DELETE $API/api/broadcasts/$ID
code -b owner.txt -X DELETE $API/api/broadcasts/$ID

# 로그아웃 204, 그 뒤 같은 쿠키로 내 정보 401
code -b owner.txt -X POST $API/api/auth/logout
code -b owner.txt $API/api/auth/me
```

확인이 끝나면 `rm owner.txt other.txt`로 쿠키 파일을 지웁니다.

### 참고

- 로그인 쿠키에는 `Secure` 속성을 붙이지 않았습니다. 개발 환경의 `http://localhost`에서도 쓰기 위해서입니다. 실서비스 배포(HTTPS) 단계에서 붙입니다.
- 로그인 쿠키는 `SameSite=Lax`라 다른 사이트에서 보낸 POST·PATCH·DELETE에는 실리지 않습니다(CSRF 방지).

## 4단계 직접 확인 절차 (후원·테스트 결제)

> 4단계에서는 토스페이먼츠 테스트 키(`TOSS_CLIENT_KEY`, `TOSS_SECRET_KEY`)가 필요합니다. 위 "토스페이먼츠 테스트 키 발급"대로 `.env`에 넣고 API 서버를 다시 실행합니다. 서버가 뜰 때 `donation` 테이블이 마이그레이션으로 추가됩니다.

후원 상태는 DB에서 직접 확인할 수 있습니다. 아래 명령을 "상태 확인"이라고 부릅니다.

```bash
docker exec streaming-platform-postgres psql -U streaming -d streaming \
  -c "select id, order_id, amount, status, payment_key, failure, approved_at from donation order by id desc limit 5"
```

**준비**: 일반 창에서 `owner1`(방송자)로 로그인해 녹화 방송을 등록하거나 라이브를 시작했다가 종료합니다. 시크릿 창에서 `viewer1`(후원자)로 로그인해 그 방송 상세로 갑니다.

### A. 테스트 결제 성공

1. `viewer1` 창의 방송 상세에서 **5,000원**을 고르고 **owner1 님에게 후원하기**를 누릅니다.
2. 그 자리에 주문서(결제수단·약관)가 열립니다. 결제수단(예: **퀵계좌이체**)을 고르고 **결제하기**를 누른 뒤 결제창 안내대로 진행합니다. 테스트 결제라 실제 돈은 빠져나가지 않습니다.
3. "결제 승인을 확인하는 중..." 뒤에 **후원 완료** — "owner1 님에게 5,000원을 후원했습니다."가 나옵니다. 서버가 결제 서비스 승인까지 확인한 뒤에만 이 화면이 나옵니다.
4. **방송으로 돌아가기**를 누르면 "후원 5,000원 · 1건"과 내역에 `viewer1 5,000원`이 보입니다. `owner1` 창과 비로그인 창에서도 같은 합계가 보입니다. `owner1` 창에는 후원 버튼이 없습니다.
5. 상태 확인: 그 주문이 `done`이고 `payment_key`, `approved_at`이 있습니다. 토스페이먼츠 개발자센터의 테스트 결제 내역에도 같은 주문번호가 있습니다.
6. 성공 화면 주소를 새로고침해도 "후원 완료"가 다시 나오고 합계는 그대로 5,000원입니다(승인을 다시 요청하지 않음).

### B. 테스트 결제 취소

1. **후원하기** → **결제하기**를 누르고 결제창을 닫습니다(X 또는 취소). 주문서에서 **다른 금액 선택**을 눌러도 취소로 기록됩니다.
2. "결제를 취소했습니다."가 나오고(환경에 따라 취소 화면으로 이동) 합계는 그대로입니다.
3. 상태 확인: 그 주문이 `canceled`입니다.

### C. 테스트 결제 실패

1. **결제 서비스가 승인을 거절하는 경우**: `.env`에 `TOSS_TEST_CODE=INVALID_REJECT_CARD`를 넣고 API 서버를 다시 실행합니다. 서버 로그에 "결제 승인 요청에 테스트 코드 ... 를 붙입니다"가 나옵니다.
   - 후원하고 결제창을 끝까지 진행하면 **후원을 확정하지 못했습니다** — "결제가 승인되지 않았습니다: ..."가 나옵니다. 합계는 그대로이고, 상태 확인에서 `failed`와 거절 사유가 보입니다.
   - 확인이 끝나면 `TOSS_TEST_CODE=`로 비우고 서버를 다시 실행합니다.
2. **화면의 성공 표시만 흉내 내는 경우**: 결제창을 거치지 않고 성공 화면 주소를 직접 엽니다. 서버가 결제 서비스에 승인을 요청하고 결제 서비스가 거절하므로 확정되지 않습니다.
   ```bash
   API=http://localhost:8000
   J='content-type: application/json'
   curl -s -c viewer.txt $API/api/auth/login -H "$J" -d '{"username":"viewer1","password":"password123"}'
   curl -s -b viewer.txt $API/api/broadcasts/<방송 id>/donations -H "$J" -d '{"amount":5000}'   # 응답의 orderId
   ```
   브라우저(`viewer1` 창)에서 `http://localhost:3300/donations/success?paymentKey=fake&orderId=<orderId>&amount=5000`을 열면 "후원을 확정하지 못했습니다"가 나오고, 상태 확인에서 `failed`입니다.
3. **금액을 바꾼 경우**: 새 주문을 만들고 같은 주소에서 `amount=100`으로 바꿔 열면 "결제 금액이 주문과 다릅니다."가 나옵니다. 서버는 결제 서비스에 승인을 요청하지 않고 `failed`로 기록합니다.
4. **결제창에서 실패한 경우**: 결제창은 실패하면 `failUrl`로 돌아옵니다. 같은 상황을 새 주문으로 `http://localhost:3300/donations/fail?order=<orderId>&code=PAY_PROCESS_ABORTED&message=결제 실패`를 열어 확인하면 "후원에 실패했습니다"가 나오고 `failed`로 기록됩니다.

### D. 승인 통지(웹훅)가 반복될 때

로컬 서버(`localhost`)에는 토스페이먼츠가 통지를 보낼 수 없으므로, 결제 서비스가 보내는 것과 같은 형식의 통지를 `curl`로 보냅니다. 서버는 통지 내용을 믿지 않고 `paymentKey`로 결제 서비스에 조회해 확인합니다.

1. A에서 확정된 주문의 `order_id`와 `payment_key`를 상태 확인으로 찾습니다(A의 성공 화면 주소에도 있습니다).
2. 같은 통지를 세 번 보냅니다. 세 번 모두 `200`입니다.
   ```bash
   ORDER=<order_id>; PAY=<payment_key>
   for i in 1 2 3; do
     curl -s -o /dev/null -w '%{http_code}\n' http://localhost:8000/api/payments/webhook -H 'content-type: application/json' \
       -d "{\"eventType\":\"PAYMENT_STATUS_CHANGED\",\"createdAt\":\"2026-11-21T12:00:00.000000\",\"data\":{\"paymentKey\":\"$PAY\",\"orderId\":\"$ORDER\",\"status\":\"DONE\"}}"
   done
   curl -s http://localhost:8000/api/broadcasts/<방송 id>/donations
   ```
3. 합계·건수가 그대로(5,000원 · 1건)이고, 상태 확인에서 그 주문의 `approved_at`도 바뀌지 않습니다.
4. 결제 서비스에 없는 결제로 통지를 보내면(`"paymentKey":"forged"`) `400`이고 아무것도 확정되지 않습니다. 이 서버에 없는 주문번호로 보내면 `200`이고 무시합니다(결제 서비스가 다시 보내지 않도록).
5. 배포한 서버에서는 개발자센터 **웹훅** 메뉴에 `https://<도메인>/api/payments/webhook`을 `PAYMENT_STATUS_CHANGED` 이벤트로 등록하면 결제 서비스가 직접 통지를 보냅니다.

확인이 끝나면 `rm viewer.txt`로 쿠키 파일을 지웁니다.

## 5단계 직접 확인 절차 (여러 인스턴스·자동 스케일링)

터미널 세 개를 씁니다. 먼저 2)의 PostgreSQL·MediaMTX와 5)의 클러스터를 띄웁니다.

### A. 요청이 여러 인스턴스로 나뉘는지

```bash
for i in $(seq 1 10); do curl -s -o /dev/null -D - http://localhost:8000/api/health | grep -i x-instance; done | sort | uniq -c
```

두 인스턴스 이름이 섞여 나옵니다(예: 5번·5번).

### B. 확인용 로그인·방송·댓글 만들기

웹(http://localhost:3300)에서 회원가입하고 영상 방송을 하나 등록한 뒤 댓글을 하나 씁니다. 브라우저 개발자 도구 → 애플리케이션 → 쿠키에서 `nadri_session` 값을 복사하고, 방송 상세 주소의 id를 확인합니다. `curl`로 할 수도 있습니다.

```bash
curl -s -c cookie.txt -o /dev/null -H 'content-type: application/json' \
  -d '{"username":"scale5","password":"<비밀번호 8자 이상>"}' http://localhost:8000/api/auth/signup
curl -s -b cookie.txt -F 'title=5단계 스케일 확인' -F 'video=@<영상 파일>.mp4;type=video/mp4' http://localhost:8000/api/broadcasts
curl -s -b cookie.txt -H 'content-type: application/json' -d '{"body":"스케일 아웃 전에 쓴 댓글"}' \
  http://localhost:8000/api/broadcasts/<방송 id>/comments
grep nadri_session cookie.txt | awk '{print "nadri_session="$7}'   # check.sh 에 넘길 쿠키
```

### C. 부하를 걸고 인스턴스 수 변화 보기

1. 터미널 1: 인스턴스 수와 CPU 사용률을 10초마다 찍습니다.
   ```bash
   ./deploy/watch.sh
   ```
2. 터미널 2: 로그인·목록·댓글·영상이 그대로인지 10초마다 확인합니다. 괄호 안은 응답한 인스턴스입니다.
   ```bash
   ./deploy/check.sh 'nadri_session=<쿠키 값>' <방송 id>
   ```
3. 터미널 3: 부하를 겁니다(가상 사용자 50명, 4분). 끝나면 요청 수·실패율 요약이 나옵니다. 도중에 멈추려면 Ctrl+C.
   ```bash
   DURATION=4m ./deploy/load.sh
   ```
4. 터미널 1에서 CPU 사용률이 50%를 넘으면 `DESIRED`·`READY`가 늘어 최대 8개가 됩니다. 부하가 끝나고 1분쯤 지나면 2개까지 줄어듭니다. `kubectl get hpa nadri-server -w`나 `kubectl get pods -l app=nadri-server -w`로도 볼 수 있습니다.
5. 그동안 터미널 2의 모든 줄이 `me=200:<아이디>`, 같은 방송 수, 같은 댓글 수, `video=206`이고 괄호 안 인스턴스 이름이 바뀌어 가는지 봅니다. 스케일 인으로 사라진 인스턴스가 받았던 로그인도 다른 인스턴스에서 그대로입니다.
6. 웹에서 새로고침해도 로그인 상태·방송 목록·댓글이 그대로입니다.

### 확인한 결과 (2026-11-22, MacBook · Docker Desktop)

`deploy/watch.sh` 출력. 부하 시작 전에는 2개였습니다. 첫 시도는 부하를 이 컴퓨터의 8000 포트로 보내다가 Docker Desktop 포트 전달 단계에서 연결이 시간 초과돼 중단했는데, 그때 3개로 늘어 있었습니다. 아래는 그 뒤 `deploy/load.sh`(가상 사용자 50명, 4분)로 다시 건 부하입니다(01:48:10 시작, 01:52:40 종료).

```
TIME     HPA-CPU      DESIRED   READY    PODS
01:47:37 2%/50%       2         2        Running:2
01:47:47 57%/50%      3         3        Running:3      ← 첫 시도
01:48:08 1%/50%       3         3        Running:3
01:48:19 50%/50%      3         3        Running:3      ← 부하 시작
01:48:39 290%/50%     6         6        Running:6
01:48:50 363%/50%     6         6        Running:6
01:49:00 316%/50%     8         6        ContainerCreating:2 Running:6
01:49:11 316%/50%     8         8        Running:8      ← 최대
01:49:21 265%/50%     8         8        Running:8
   … (최대 8개로 200% 안팎 유지)
01:52:28 186%/50%     8         8        Running:8
01:52:38 44%/50%      8         8        Running:8      ← 부하 종료
01:52:48 4%/50%       8         8        Running:8
01:53:29 1%/50%       8         8        Running:8
01:53:39 1%/50%       6         6        Running:6      ← 스케일 인
01:54:10 1%/50%       4         4        Running:4
01:54:31 1%/50%       2         2        Running:2 Terminating:2
01:54:51 1%/50%       2         2        Running:2      ← 최소
```

`deploy/load.sh` 요약: 요청 557,886건(초당 약 2,300건), 실패 0건(0.00%), 응답 시간 p95 21.7ms. 스케일 인으로 인스턴스가 종료되는 동안에도 실패한 요청은 없었습니다.

`deploy/check.sh` 출력(일부). 같은 시간 동안 60회 모두 로그인 유지·방송 11개·댓글 1개·영상 `206`이었고, 응답한 인스턴스는 8개였습니다.

```
01:47:42  me=200:scale5(nadri-server-5b79584578-s4lbs)  broadcasts=200:11(nadri-server-5b79584578-s4lbs)  comments=200:1(nadri-server-5b79584578-s4lbs)  video=206 nadri-server-5b79584578-s4lbs
01:48:44  me=200:scale5(nadri-server-5b79584578-7x6pm)  broadcasts=200:11(nadri-server-5b79584578-9nvmr)  comments=200:1(nadri-server-5b79584578-z65dg)  video=206 nadri-server-5b79584578-9nvmr
01:49:35  me=200:scale5(nadri-server-5b79584578-dfrtb)  broadcasts=200:11(nadri-server-5b79584578-dfrtb)  comments=200:1(nadri-server-5b79584578-9nvmr)  video=206 nadri-server-5b79584578-9nvmr
01:54:38  me=200:scale5(nadri-server-5b79584578-7x6pm)  broadcasts=200:11(nadri-server-5b79584578-7x6pm)  comments=200:1(nadri-server-5b79584578-7x6pm)  video=206 nadri-server-5b79584578-s4lbs
01:54:59  me=200:scale5(nadri-server-5b79584578-7x6pm)  broadcasts=200:11(nadri-server-5b79584578-s4lbs)  comments=200:1(nadri-server-5b79584578-s4lbs)  video=206 nadri-server-5b79584578-7x6pm
```

- 로그인 세션은 `…447lw` 인스턴스에서 만들어졌고, 댓글은 `…s4lbs` 인스턴스가 받았습니다. 둘 다 부하 중에 새로 뜬 인스턴스(`…9nvmr`, `…dfrtb` 등)와 스케일 인 뒤 남은 인스턴스에서 그대로 조회됐습니다. `…447lw`는 스케일 인으로 사라졌습니다.
- 클러스터에서 송출 없이 시작한 라이브(`POST /api/broadcasts/live`)는 다른 인스턴스에서 조회해도 `live`였고, 31초 뒤 `ended`가 됐습니다(송출이 연결되지 않은 라이브를 끝내는 확인이 클러스터에서도 MediaMTX에 닿음).

### 문제 해결

- **`up.sh`에서 포트 8000 사용 중**: `cargo run -p server`로 띄운 서버를 끄고 `./deploy/down.sh` 후 다시 실행합니다.
- **Pod 이 `Running`인데 `READY 0/1`**: DB에 닿지 않는 상태입니다. `docker compose ps`로 PostgreSQL이 떠 있는지, `kubectl logs deploy/nadri-server`로 오류를 확인합니다.
- **`watch.sh`의 CPU가 `?`**: metrics-server가 지표를 모으는 중입니다. 1분쯤 기다립니다.
- **부하를 `localhost:8000`으로 보내면 `dial: i/o timeout`**: 새 연결이 몰리면 Docker Desktop의 포트 전달이 따라가지 못합니다. `load.sh`처럼 kind 네트워크에서 보냅니다.

## 6단계 직접 확인 절차 (클라우드 배포·데스크톱 클라이언트)

외부 네트워크(예: 휴대폰 LTE 테더링, 다른 장소의 인터넷)에서 확인합니다.

### A. 도메인과 HTTPS

아래 항목(DNS, HTTPS 이동, 인증서, 상태 확인, 다운로드 파일)을 한 번에 확인하는 스크립트입니다.

```bash
deploy/oci/check.sh <도메인>
```

하나씩 확인하려면:

```bash
dig +short <도메인> @1.1.1.1
```

```bash
curl -sI https://<도메인>/api/health
```

```bash
curl -sI http://<도메인>/
```

1. `dig` 결과가 VM 공인 IP인지 봅니다.
2. `/api/health`가 `HTTP/2 200`인지 봅니다.
3. HTTP 주소가 `308`으로 `https://`로 넘어가는지 봅니다.
4. 브라우저 주소창의 자물쇠에서 인증서 발급자가 Let's Encrypt인지 확인합니다.
5. 웹에서 가입·로그인 → 녹화 방송 업로드·재생 → 댓글 → 테스트 결제 후원이 그대로 되는지 확인합니다.

### B. 데스크톱 클라이언트 (Windows, macOS 각각)

1. 브라우저로 `https://<도메인>/download/`를 열어 자기 OS 파일을 받습니다.
2. 페이지의 설치 방법대로 설치하고 실행합니다(첫 실행 경고는 페이지 안내대로 허용).
3. 앱 위쪽에 서버 주소(`https://<도메인>`)가 보이고 방송 목록이 나오는지 확인합니다.
4. 녹화 방송을 골라 재생되는지 확인합니다.
5. 웹(휴대폰 또는 PC 브라우저)에서 라이브를 시작한 뒤 앱에서 **새로고침** → **LIVE** 방송을 골라 재생되는지 확인합니다.

### 확인한 결과

**서버·HTTPS** (2026-11-22, 이 컴퓨터의 가정용 인터넷에서 `https://nadri.me`로 접속)
- `deploy/oci/check.sh nadri.me` 10개 항목 모두 통과:
  - DNS A `129.203.181.147`
  - HTTP → HTTPS `308`
  - 인증서 발급자 Let's Encrypt
  - `/api/health`·방송 목록·웹 앱·다운로드 페이지 `200`
  - 설치 파일 3개와 체크섬 `200`
- 가입 `201`(쿠키에 `Secure`), 녹화 영상 업로드 `201`, 영상 Range 요청 `206`, 댓글 `201`
- 다른 계정의 후원 주문 `201`(테스트 키, `pending`). 자기 방송 후원은 `400`으로 막힘
- ffmpeg로 WHIP 라이브 송출 → MediaMTX에 `publishing to path 'live/2'`
- 웹 앱 목록에 라이브·녹화 방송이 표시됨

**데스크톱 클라이언트**

| OS · 버전 | 아키텍처 | 받은 파일 | 설치·실행 | 목록 조회 | 녹화 재생 | 라이브 재생 |
| --- | --- | --- | --- | --- | --- | --- |
| macOS 27.0.1 | arm64 (Apple Silicon) | `https://nadri.me/downloads/nadri-macos-arm64.dmg` (체크섬 일치) | 디스크 이미지에서 앱 복사 후 실행 | 성공 (라이브 1·녹화 1) | 성공 (재생 시간 증가, 길이 10초) | 성공 (MediaMTX `reading from path 'live/2'`, 앱 "라이브 재생 중") |
| macOS 27.0.1 (브라우저로 다운로드) | arm64 (Apple Silicon) | 다운로드 페이지의 Apple Silicon 디스크 이미지 | 설치·실행 성공 | 성공 | 성공 | 성공 |
| Windows 11 64비트 | x64 | 다운로드 페이지의 `nadri-windows-x64-setup.exe` | 설치·실행 성공 | 성공 | 성공 | 성공 |

- 첫 번째 macOS 줄은 명령줄로 받은 파일이라 브라우저 다운로드에 붙는 격리 표시(Gatekeeper 첫 실행 확인)를 거치지 않았습니다.
- 두 번째 macOS 줄과 Windows 줄은 사용자가 각 컴퓨터의 브라우저로 공개 링크에서 직접 받아 설치하고 확인한 결과입니다.
- 휴대폰 브라우저에서 `https://nadri.me`로 라이브 송출 화면을 열어 전면 ↔ 후면 카메라 전환을 확인했습니다(사용자 확인). 처음 확인 때는 후면 전환이 "다른 앱이 카메라·마이크를 쓰고 있습니다"로 실패해 006-13에서 고쳤습니다.
- 다운로드 페이지는 접속한 컴퓨터에 맞는 파일 하나를 강조합니다. Apple Silicon Mac의 Chrome에서 Apple Silicon 카드만 강조되는 것을 확인했습니다.

### 문제 해결

- **인증서를 못 받음**(Caddy 로그 `challenge failed`): DNS A 레코드가 VM 공인 IP를 가리키는지, Cloudflare 프록시가 꺼져 있는지(회색 구름), 보안 목록과 VM iptables에 80·443이 열려 있는지 확인합니다. `ssh nadri 'cd /opt/nadri && docker compose logs caddy'`
- **라이브만 재생되지 않음**: 신호는 HTTPS로 가지만 영상은 8189로 직접 오갑니다. 보안 목록과 VM iptables에 UDP·TCP 8189가 열려 있는지, `.env`의 `PUBLIC_IP`가 실제 공인 IP인지 확인합니다.
- **macOS에서 "손상되었기 때문에 열 수 없습니다"**: 서명하지 않은 앱이라 생기는 경고입니다. `xattr -dr com.apple.quarantine /Applications/Nadri.app` 후 다시 엽니다.
- **Windows에서 앱 창이 비어 있음**: Microsoft Edge WebView2 런타임을 설치합니다.
