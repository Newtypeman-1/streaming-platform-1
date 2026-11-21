#!/usr/bin/env bash
# 로컬 Kubernetes(kind) 클러스터에 API 서버를 여러 인스턴스로 띄운다.
# 준비물: Docker, kind, kubectl, 저장소 루트의 .env, docker compose 로 띄운 postgres·mediamtx
set -euo pipefail
cd "$(dirname "$0")/.."

CLUSTER=nadri
METRICS_SERVER=https://github.com/kubernetes-sigs/metrics-server/releases/download/v0.8.0/components.yaml

[ -f .env ] || { echo ".env 가 없습니다. .env.example 을 복사해 채워 주세요."; exit 1; }
set -a; . ./.env; set +a
mkdir -p storage/videos

# 1. 클러스터 (노드 3개, 모든 노드에 storage/videos 공유)
if ! kind get clusters | grep -qx "$CLUSTER"; then
  sed "s|__VIDEOS__|$PWD/storage/videos|" deploy/kind.yaml | kind create cluster --name "$CLUSTER" --config -
fi
kubectl config use-context "kind-$CLUSTER" >/dev/null

# 2. HPA 가 CPU 사용률을 읽을 수 있게 metrics-server 설치 (kind 의 kubelet 인증서는 자체 서명이라 검증을 끈다)
kubectl apply -f "$METRICS_SERVER"
kubectl -n kube-system patch deployment metrics-server --type=json \
  -p='[{"op":"add","path":"/spec/template/spec/containers/0/args/-","value":"--kubelet-insecure-tls"}]' 2>/dev/null || true

# 3. 서버 이미지 빌드 후 클러스터 노드에 넣기
docker build -t nadri-server:local .
kind load docker-image nadri-server:local --name "$CLUSTER"

# 4. 환경변수: 클러스터 안에서는 이 컴퓨터의 localhost 대신 host.docker.internal 로 DB·MediaMTX 에 접속한다
to_cluster() { echo "${1//localhost/host.docker.internal}"; }
kubectl create secret generic nadri-env --dry-run=client -o yaml \
  --from-literal=DATABASE_URL="$(to_cluster "$DATABASE_URL")" \
  --from-literal=MEDIAMTX_URL="$(to_cluster "${MEDIAMTX_URL:-http://localhost:8889}")" \
  --from-literal=MEDIAMTX_API_URL="$(to_cluster "${MEDIAMTX_API_URL:-http://localhost:9997}")" \
  --from-literal=TOSS_API_URL="${TOSS_API_URL:-https://api.tosspayments.com}" \
  --from-literal=TOSS_CLIENT_KEY="${TOSS_CLIENT_KEY:-}" \
  --from-literal=TOSS_SECRET_KEY="${TOSS_SECRET_KEY:-}" \
  | kubectl apply -f -

# 5. 서버 Deployment·Service·HPA
existed=$(kubectl get deployment nadri-server -o name 2>/dev/null || true)
kubectl apply -f deploy/k8s/server.yaml
# 이미 떠 있던 서버는 새로 넣은 이미지·환경변수로 다시 띄운다
[ -n "$existed" ] && kubectl rollout restart deployment/nadri-server >/dev/null
kubectl rollout status deployment/nadri-server --timeout=180s
kubectl get pods -l app=nadri-server -o wide
kubectl get hpa nadri-server
echo "API: http://localhost:8000/api/health"
