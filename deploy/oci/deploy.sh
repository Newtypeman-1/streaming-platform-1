#!/usr/bin/env bash
# 이 컴퓨터에서 빌드해 OCI VM 에 배포한다. 저장소 루트에서 실행:
#   VM=nadri deploy/oci/deploy.sh
# 준비물: Docker, dx(Dioxus CLI), ~/.ssh/config 의 VM 접속 이름(VM), VM 에 setup-vm.sh 실행과 /opt/nadri/.env 작성
# 환경변수:
#   VM  ssh 접속 이름 (기본 nadri)
set -euo pipefail
cd "$(dirname "$0")/../.."

VM=${VM:-nadri}
DEST=/opt/nadri
TAG=$(git rev-parse --short HEAD)
SITE=target/oci-site

ssh "$VM" "test -f $DEST/.env" || { echo "$VM:$DEST/.env 가 없습니다. deploy/oci/.env.example 을 복사해 채워 주세요."; exit 1; }

# 1. 서버 이미지 (VM 이 arm64 라 linux/arm64 로 빌드)
docker build --platform linux/arm64 -t "nadri-server:$TAG" .

# 2. 웹 앱 정적 파일
(cd web && dx bundle --platform web --release)
rm -rf "$SITE" && mkdir -p "$SITE"
cp -R target/dx/web/release/web/public "$SITE/web"

# 3. 설정·정적 파일 올리기
ssh "$VM" "mkdir -p $DEST/site"
scp deploy/oci/compose.yml deploy/oci/Caddyfile mediamtx.yml "$VM:$DEST/"
rsync -a --delete "$SITE/web/" "$VM:$DEST/site/web/"

# 4. 이미지 옮기고 실행 (바뀐 서비스만 다시 뜬다)
docker save "nadri-server:$TAG" | gzip | ssh "$VM" 'gunzip | docker load'
ssh "$VM" "cd $DEST && sed -i 's/^SERVER_TAG=.*/SERVER_TAG=$TAG/' .env && docker compose up -d && docker compose ps"

# 5. 공개 주소로 상태 확인
domain=$(ssh "$VM" "grep '^NADRI_DOMAIN=' $DEST/.env | cut -d= -f2")
for _ in $(seq 30); do
  if curl -fsS "https://$domain/api/health" -o /dev/null; then
    echo "배포 완료: https://$domain (서버 $TAG)"
    exit 0
  fi
  sleep 2
done
echo "https://$domain/api/health 가 응답하지 않습니다. ssh $VM 'cd $DEST && docker compose logs' 로 확인하세요."
exit 1
