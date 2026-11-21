#!/usr/bin/env bash
# 스케일이 바뀌는 동안 로그인 상태·방송 목록·댓글·영상이 그대로인지 확인한다.
# 요청마다 응답한 인스턴스(X-Instance)를 함께 찍는다. Ctrl+C 로 멈춘다.
#   deploy/check.sh <로그인 쿠키 nadri_session=...> <방송 id>
set -uo pipefail
BASE=${BASE_URL:-http://localhost:8000}
COOKIE=$1 ID=$2
call() { curl -s -o /tmp/nadri-check.$$ -w '%{http_code} %header{x-instance}' "$@"; }
while true; do
  me=$(call -H "Cookie: $COOKIE" "$BASE/api/auth/me"); user=$(grep -o '"username":"[^"]*"' /tmp/nadri-check.$$ | cut -d'"' -f4)
  list=$(call "$BASE/api/broadcasts"); count=$(grep -o '"status":' /tmp/nadri-check.$$ | wc -l | tr -d ' ')
  comments=$(call "$BASE/api/broadcasts/$ID/comments"); ccount=$(grep -o '"body":' /tmp/nadri-check.$$ | wc -l | tr -d ' ')
  video=$(call -r 0-99 "$BASE/api/broadcasts/$ID/video")
  printf '%s  me=%s(%s)  broadcasts=%s(%s)  comments=%s(%s)  video=%s\n' "$(date +%H:%M:%S)" \
    "${me%% *}:$user" "${me#* }" "${list%% *}:$count" "${list#* }" "${comments%% *}:$ccount" "${comments#* }" "$video"
  sleep "${INTERVAL:-10}"
done
