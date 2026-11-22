#!/usr/bin/env bash
# 새로 만든 OCI VM(Ubuntu 24.04, arm64)을 한 번 준비한다. 이 컴퓨터에서:
#   ssh nadri 'bash -s' < deploy/oci/setup-vm.sh
# Docker 설치, 방화벽(80·443·8189) 열기, 배포 디렉터리(/opt/nadri) 생성.
set -euo pipefail

# 1. OCI Ubuntu 이미지는 VCN 보안 목록과 별개로 VM 안의 iptables 가 22번 외 접속을 거절(REJECT)한다.
#    거절 규칙 앞에 허용 규칙을 넣고 재부팅 후에도 남도록 저장한다. Docker 가 규칙을 더하기 전에 저장해야 깔끔하다.
reject=$(sudo iptables -L INPUT --line-numbers -n | awk '$2 == "REJECT" { print $1; exit }')
for rule in "-p tcp --dport 80" "-p tcp --dport 443" "-p udp --dport 443" "-p udp --dport 8189" "-p tcp --dport 8189"; do
  # shellcheck disable=SC2086
  if ! sudo iptables -C INPUT $rule -m state --state NEW -j ACCEPT 2>/dev/null; then
    sudo iptables -I INPUT "${reject:-1}" $rule -m state --state NEW -j ACCEPT
  fi
done
sudo netfilter-persistent save

# 2. Docker 와 compose 플러그인 (Ubuntu 저장소 패키지)
sudo apt-get update
sudo apt-get install -y docker.io docker-compose-v2
sudo systemctl enable --now docker
sudo usermod -aG docker "$USER"

# 3. 스왑 2GB. 메모리가 1GB 인 VM(E2.1.Micro)에서 DB·미디어 서버·API·Caddy 가 함께 떠도 메모리 부족으로 죽지 않게 한다.
if ! swapon --show=NAME --noheadings | grep -qx /swapfile; then
  sudo fallocate -l 2G /swapfile
  sudo chmod 600 /swapfile
  sudo mkswap /swapfile
  sudo swapon /swapfile
  grep -q '^/swapfile ' /etc/fstab || echo '/swapfile none swap sw 0 0' | sudo tee -a /etc/fstab
fi

# 4. 배포 디렉터리
sudo mkdir -p /opt/nadri
sudo chown "$USER" /opt/nadri

echo "준비 완료. 다시 접속하면 sudo 없이 docker 를 쓸 수 있습니다."
