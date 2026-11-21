#!/usr/bin/env bash
# 인스턴스 수와 CPU 사용률을 주기적으로 찍는다. Ctrl+C 로 멈춘다.
#   deploy/watch.sh        # 10초마다
#   INTERVAL=5 deploy/watch.sh
set -uo pipefail
printf '%-8s %-12s %-9s %-8s %s\n' TIME HPA-CPU DESIRED READY PODS
while true; do
  cpu=$(kubectl get hpa nadri-server -o jsonpath='{.status.currentMetrics[0].resource.current.averageUtilization}' 2>/dev/null)
  desired=$(kubectl get hpa nadri-server -o jsonpath='{.status.desiredReplicas}' 2>/dev/null)
  ready=$(kubectl get deployment nadri-server -o jsonpath='{.status.readyReplicas}' 2>/dev/null)
  pods=$(kubectl get pods -l app=nadri-server --no-headers 2>/dev/null | awk '{print $3}' | sort | uniq -c | awk '{printf "%s:%s ", $2, $1}')
  printf '%-8s %-12s %-9s %-8s %s\n' "$(date +%H:%M:%S)" "${cpu:-?}%/50%" "${desired:-?}" "${ready:-0}" "$pods"
  sleep "${INTERVAL:-10}"
done
