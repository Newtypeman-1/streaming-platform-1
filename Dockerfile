# API 서버 이미지. 저장소 루트에서 빌드한다: docker build -t nadri-server:local .
FROM rust:1-bookworm AS build
WORKDIR /src
COPY . .
# 레지스트리·빌드 결과를 캐시해 두 번째 빌드부터는 바뀐 크레이트만 다시 컴파일한다
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p server \
    && cp target/release/server /usr/local/bin/server

FROM debian:bookworm-slim
# 결제 API(HTTPS) 인증서 확인용
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home nadri \
    && mkdir -p /data/videos && chown nadri /data/videos
COPY --from=build /usr/local/bin/server /usr/local/bin/server
USER nadri
WORKDIR /home/nadri
ENV SERVER_PORT=8000 VIDEO_STORAGE_DIR=/data/videos
EXPOSE 8000
CMD ["server"]
