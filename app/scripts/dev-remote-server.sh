#!/usr/bin/env bash
# Local Linux "server" for exercising remote model deployment over real SSH.
#
# Usage:
#   app/scripts/dev-remote-server.sh up       # build + start, print app settings
#   app/scripts/dev-remote-server.sh down     # remove the container (keeps the image)
#   app/scripts/dev-remote-server.sh status
#   app/scripts/dev-remote-server.sh logs
#   app/scripts/dev-remote-server.sh shell
#
# The container is deliberately minimal: openssh-server, curl, tar and
# coreutils, which is what remote.rs probes for. It is CPU-only and arm64/x64
# follows the Docker host, so use 加速=CPU and GPU 层数=0 in the app.
set -euo pipefail

CONTAINER="${CONTAINER:-sofer-ssh-test}"
IMAGE="${IMAGE:-sofer-ssh-test:latest}"
SSH_PORT="${SSH_PORT:-2222}"
WORK_DIR="${WORK_DIR:-/tmp/sofer-ssh-test}"
SSH_USER="${SSH_USER:-ocr}"
KEY="${WORK_DIR}/test_key"

usage() {
  awk 'NR > 1 && /^#/ { sub(/^# ?/, ""); print; next } NR > 1 { exit }' "$0"
}

require_docker() {
  if ! command -v docker >/dev/null 2>&1; then
    echo "找不到 docker；先启动 Docker Desktop 或安装 docker。" >&2
    exit 1
  fi
  if ! docker info >/dev/null 2>&1; then
    echo "Docker daemon 没有运行；先启动 Docker Desktop。" >&2
    exit 1
  fi
}

wait_for_ssh() {
  for _ in $(seq 1 30); do
    if ssh -p "$SSH_PORT" -i "$KEY" \
        -o BatchMode=yes -o ConnectTimeout=2 -o StrictHostKeyChecking=accept-new \
        "${SSH_USER}@127.0.0.1" true >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  echo "SSH 没有在 30 秒内就绪；查看 docker logs ${CONTAINER}" >&2
  return 1
}

up() {
  require_docker
  mkdir -p "$WORK_DIR"
  if [ ! -f "$KEY" ]; then
    ssh-keygen -t ed25519 -f "$KEY" -N '' -q
    echo "生成测试密钥：$KEY"
  fi
  cat >"${WORK_DIR}/Dockerfile" <<'DOCKERFILE'
FROM ubuntu:24.04
RUN apt-get update && apt-get install -y --no-install-recommends \
      openssh-server curl ca-certificates tar coreutils libgomp1 \
    && rm -rf /var/lib/apt/lists/*
RUN useradd -m -s /bin/bash ocr \
    && mkdir -p /home/ocr/.ssh /run/sshd \
    && chmod 700 /home/ocr/.ssh
COPY test_key.pub /home/ocr/.ssh/authorized_keys
RUN chown -R ocr:ocr /home/ocr/.ssh \
    && chmod 600 /home/ocr/.ssh/authorized_keys
EXPOSE 22
CMD ["/usr/sbin/sshd", "-D", "-e"]
DOCKERFILE

  echo "构建镜像 ${IMAGE} ..."
  docker build -t "$IMAGE" "$WORK_DIR"
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
  # A rebuilt image has fresh host keys; stale entries make accept-new fail.
  ssh-keygen -R "[127.0.0.1]:${SSH_PORT}" >/dev/null 2>&1 || true
  echo "启动容器 ${CONTAINER}（SSH 127.0.0.1:${SSH_PORT}）..."
  docker run -d --name "$CONTAINER" -p "${SSH_PORT}:22" "$IMAGE" >/dev/null
  wait_for_ssh

  cat <<EOF

就绪。应用里这样填：

  服务器          127.0.0.1
  用户名          ${SSH_USER}
  SSH 端口        ${SSH_PORT}
  私钥            ${KEY}
  远程目录        ~/.sofer
  识别服务端口    8099
  本地隧道端口    0（自动）
  加速            CPU
  GPU 层数        0

进入容器：

  CONTAINER=${CONTAINER} app/scripts/dev-remote-server.sh shell

清理：

  CONTAINER=${CONTAINER} app/scripts/dev-remote-server.sh down
EOF
}

down() {
  require_docker
  docker rm -f "$CONTAINER" >/dev/null 2>&1 || true
  echo "已删除容器 ${CONTAINER}（镜像和 ${WORK_DIR} 保留）"
}

status() {
  require_docker
  docker ps -a --filter "name=^/${CONTAINER}$" \
    --format 'table {{.Names}}\t{{.Status}}\t{{.Ports}}'
}

logs() {
  require_docker
  docker logs "$CONTAINER"
}

shell() {
  require_docker
  docker exec -it "$CONTAINER" bash
}

case "${1:-}" in
  up) up ;;
  down) down ;;
  status) status ;;
  logs) logs ;;
  shell) shell ;;
  -h|--help|help|"") usage ;;
  *)
    echo "未知动作：$1" >&2
    usage >&2
    exit 2
    ;;
esac
