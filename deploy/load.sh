#!/usr/bin/env bash
# 클러스터에 부하를 건다. Ctrl+C 로 멈춘다.
#   deploy/load.sh                       # 가상 사용자 50명, 3분
#   VUS=100 DURATION=5m deploy/load.sh
# 부하 컨테이너를 kind 네트워크에 붙여 Service(NodePort)로 바로 보낸다.
# 이 컴퓨터의 8000 포트(Docker Desktop 포트 전달)를 거치면 새 연결이 몰릴 때 전달 단계에서 연결이 시간 초과된다.
set -euo pipefail
cd "$(dirname "$0")"
docker run --rm -i --network kind \
  -e VUS="${VUS:-50}" -e DURATION="${DURATION:-3m}" -e BASE_URL="${BASE_URL:-http://nadri-control-plane:30800}" \
  grafana/k6:1.4.2 run --quiet - < load.js
