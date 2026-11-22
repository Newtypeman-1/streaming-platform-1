#!/usr/bin/env bash
# 공개 주소를 바깥에서 점검한다: DNS, HTTP→HTTPS 이동, 인증서, 상태 확인, 방송 목록, 다운로드 페이지와 설치 파일.
#   deploy/oci/check.sh nadri.me
# 모두 통과하면 0, 하나라도 실패하면 1 로 끝난다.
set -uo pipefail

DOMAIN=${1:?사용법: deploy/oci/check.sh <도메인>}
BASE="https://$DOMAIN"
failed=0

check() { # 이름 기대값 실제값
  if [ "$2" = "$3" ]; then echo "  ok   $1: $3"; else echo "  FAIL $1: 기대 $2, 실제 $3"; failed=1; fi
}
status() { curl -s -o /dev/null -w '%{http_code}' "$@"; }

echo "[$DOMAIN]"
ip=$(dig +short "$DOMAIN" @1.1.1.1 | tail -1)
echo "  DNS  A 레코드: ${ip:-없음}"
[ -n "$ip" ] || failed=1

check "HTTP 는 HTTPS 로 이동" "308 $BASE/" "$(curl -s -o /dev/null -w '%{http_code} %{redirect_url}' "http://$DOMAIN/")"
issuer=$(echo | openssl s_client -connect "$DOMAIN:443" -servername "$DOMAIN" 2>/dev/null | openssl x509 -noout -issuer 2>/dev/null)
echo "  인증서 발급자: ${issuer:-확인 실패}"
case "$issuer" in *"Let's Encrypt"*) ;; *) failed=1 ;; esac

check "상태 확인 /api/health" 200 "$(status "$BASE/api/health")"
check "방송 목록 /api/broadcasts" 200 "$(status "$BASE/api/broadcasts")"
check "웹 앱 /" 200 "$(status "$BASE/")"
check "다운로드 페이지 /download/" 200 "$(status "$BASE/download/")"
for file in nadri-windows-x64-setup.exe nadri-macos-arm64.dmg nadri-macos-x64.dmg SHA256SUMS.txt; do
  check "설치 파일 $file" 200 "$(status -I "$BASE/downloads/$file")"
done

exit $failed
