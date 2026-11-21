#!/usr/bin/env bash
# 클러스터를 지운다. DB(docker compose)와 storage/videos 의 데이터는 그대로 남는다.
set -euo pipefail
kind delete cluster --name nadri
