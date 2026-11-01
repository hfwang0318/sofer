#!/usr/bin/env bash
# End-to-end check for remote model deployment against a real SSH server.
#
# By default it uses the local Docker container from dev-remote-server.sh and
# downloads the low-precision pack once (~1 GB). Set OCR4CODE_REMOTE_QUALITY=high
# for the Q8 pack. Stop the app's remote service first; the script kills the
# server on port 8099 and replaces the model directory.
#
# Usage:
#   app/scripts/test-remote.sh
#   CASES="probe stop-releases-server" app/scripts/test-remote.sh
#   CONTAINER=my-ssh-box SSH_PORT=2222 OCR4CODE_REMOTE_KEY=~/.ssh/id_ed25519 \
#     app/scripts/test-remote.sh
#
# Exit code is non-zero if any case fails. Logs land in
# /tmp/ocr4code-ssh-test/e2e-logs/.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK_DIR="${WORK_DIR:-/tmp/ocr4code-ssh-test}"
CONTAINER="${CONTAINER:-ocr4code-ssh-test}"
SSH_PORT="${SSH_PORT:-2222}"
DOCKER_IMAGE="${DOCKER_IMAGE:-ocr4code-ssh-test:latest}"
KEY="${OCR4CODE_REMOTE_KEY:-$WORK_DIR/test_key}"
TEST_IMAGE="${OCR4CODE_TEST_IMAGE:-$ROOT/bench/images/cargo-build@1x.png}"
QUALITY="${OCR4CODE_REMOTE_QUALITY:-low}"
BIN="$ROOT/app/src-tauri/target/debug/examples/remote_probe"
LOG_DIR="$WORK_DIR/e2e-logs"
LOCAL_MODELS="$WORK_DIR/local-models"
CARGO="${CARGO:-$(command -v cargo || true)}"
[ -n "$CARGO" ] || CARGO="$HOME/.cargo/bin/cargo"

export OCR4CODE_REMOTE_HOST="${OCR4CODE_REMOTE_HOST:-127.0.0.1}"
export OCR4CODE_REMOTE_USER="${OCR4CODE_REMOTE_USER:-ocr}"
export OCR4CODE_REMOTE_SSH_PORT="$SSH_PORT"
export OCR4CODE_REMOTE_KEY="$KEY"
export OCR4CODE_REMOTE_DIR="${OCR4CODE_REMOTE_DIR:-~/.ocr4code}"
export OCR4CODE_REMOTE_ACCEL=cpu
export OCR4CODE_REMOTE_GPU_LAYERS=0
export OCR4CODE_REMOTE_QUALITY="$QUALITY"

PASS=0
FAIL=0

pass() { printf 'PASS  %s\n' "$1"; PASS=$((PASS + 1)); }
fail() {
  printf 'FAIL  %s  (log: %s)\n' "$1" "$2"
  FAIL=$((FAIL + 1))
  tail -n 20 "$2"
}

run_case() {
  local name="$1"
  shift
  local log="$LOG_DIR/$name.log"
  printf '\n== %s ==\n' "$name"
  if "$@" >"$log" 2>&1; then
    pass "$name"
  else
    fail "$name" "$log"
  fi
}

remote_sh() { docker exec -u ocr "$CONTAINER" sh -c "$1"; }
clear_remote_models() { remote_sh 'rm -rf ~/.ocr4code/models'; }
remote_model_ready() { remote_sh "test -f ~/.ocr4code/models/verified-${QUALITY}.json"; }
# The bracket keeps pgrep from matching the `sh -c` wrapper that contains the
# same pattern in its own command line.
remote_server_running() { remote_sh "pgrep -f '[r]untime/llama-server' >/dev/null 2>&1"; }

require_setup() {
  command -v docker >/dev/null || {
    echo "需要 docker" >&2
    exit 1
  }
  [ -f "$TEST_IMAGE" ] || {
    echo "找不到测试图片 $TEST_IMAGE" >&2
    exit 1
  }
  if [ ! -x "$KEY" ] && [ ! -f "$KEY" ]; then
    echo "找不到私钥 $KEY；先运行 dev-remote-server.sh up" >&2
    exit 1
  fi
  if ! docker ps --filter "name=^/${CONTAINER}$" --format '{{.Names}}' | grep -qx "$CONTAINER"; then
    echo "容器 $CONTAINER 没有运行；先运行 app/scripts/dev-remote-server.sh up" >&2
    exit 1
  fi
  mkdir -p "$LOG_DIR"
}

# --- cases ---------------------------------------------------------------

case_probe() {
  local out
  out=$("$BIN" probe 2>&1) || {
    printf '%s\n' "$out"
    return 1
  }
  printf '%s\n' "$out"
  grep -q '"reachable": true' <<<"$out" &&
    grep -q '"os": "Linux"' <<<"$out" &&
    grep -q '"modelInstalled"' <<<"$out"
}

case_download() {
  clear_remote_models
  local out
  out=$(OCR4CODE_REMOTE_SOURCE=download OCR4CODE_TEST_IMAGE="$TEST_IMAGE" "$BIN" deploy 2>&1) || {
    printf '%s\n' "$out"
    return 1
  }
  printf '%s\n' "$out"
  grep -q '\[progress\] downloading' <<<"$out" &&
    grep -q '\[health\].*200' <<<"$out" &&
    grep -q '\[ocr\].*行' <<<"$out" &&
    remote_model_ready
}

case_reuse() {
  local out
  out=$(OCR4CODE_REMOTE_SOURCE=download "$BIN" deploy 2>&1) || {
    printf '%s\n' "$out"
    return 1
  }
  printf '%s\n' "$out"
  grep -q '\[health\].*200' <<<"$out" &&
    ! grep -q '\[progress\] downloading' <<<"$out"
}

case_tunnel_and_ocr() {
  local out
  out=$(OCR4CODE_TEST_IMAGE="$TEST_IMAGE" "$BIN" start 2>&1) || {
    printf '%s\n' "$out"
    return 1
  }
  printf '%s\n' "$out"
  grep -q '\[start\] 隧道就绪' <<<"$out" &&
    grep -q '\[health\].*200' <<<"$out" &&
    grep -q '\[ocr\].*行' <<<"$out"
}

case_stop() {
  local out
  out=$(OCR4CODE_TEST_KEEP=1 OCR4CODE_TEST_IMAGE="$TEST_IMAGE" "$BIN" start 2>&1) || {
    printf '%s\n' "$out"
    return 1
  }
  printf '%s\n' "$out"
  remote_server_running || {
    echo "start 之后远端 llama-server 没有运行"
    return 1
  }
  "$BIN" stop >/dev/null
  local wait
  for wait in $(seq 1 20); do
    if ! remote_server_running; then return 0; fi
    sleep 0.25
  done
  echo "stop 之后远端 llama-server 仍在运行"
  return 1
}

case_upload() {
  rm -rf "$LOCAL_MODELS"
  mkdir -p "$LOCAL_MODELS"
  docker cp "$CONTAINER:/home/ocr/.ocr4code/models/." "$LOCAL_MODELS" || return 1
  clear_remote_models
  local out
  out=$(OCR4CODE_REMOTE_SOURCE=upload OCR4CODE_REMOTE_LOCAL_MODELS="$LOCAL_MODELS" \
    OCR4CODE_TEST_IMAGE="$TEST_IMAGE" "$BIN" deploy 2>&1) || {
    printf '%s\n' "$out"
    return 1
  }
  printf '%s\n' "$out"
  grep -q '\[progress\] uploading' <<<"$out" &&
    grep -q '\[health\].*200' <<<"$out" &&
    grep -q '\[ocr\].*行' <<<"$out" &&
    remote_model_ready
}

case_bad_key() {
  local out
  if out=$(OCR4CODE_REMOTE_KEY=/nonexistent/ocr4code-test-key "$BIN" probe 2>&1); then
    printf '%s\n' "$out"
    return 1
  fi
  printf '%s\n' "$out"
  grep -qiE '连不上|permission denied|no such file|denied' <<<"$out"
}

case_unreachable() {
  local out
  if out=$(OCR4CODE_REMOTE_SSH_PORT=2299 "$BIN" probe 2>&1); then
    printf '%s\n' "$out"
    return 1
  fi
  printf '%s\n' "$out"
  grep -qiE '连不上|timed out|connection refused|timeout' <<<"$out"
}

# --- main ----------------------------------------------------------------

require_setup
echo "构建 remote_probe ..."
"$CARGO" build --manifest-path "$ROOT/app/src-tauri/Cargo.toml" --example remote_probe
rm -f "$LOG_DIR"/*.log

CASES="${CASES:-probe server-download reuse-without-download tunnel-and-ocr stop-releases-server upload-local-models rejects-bad-key rejects-unreachable-host}"
for case_name in $CASES; do
  case "$case_name" in
    probe) run_case probe case_probe ;;
    server-download) run_case server-download case_download ;;
    reuse-without-download) run_case reuse-without-download case_reuse ;;
    tunnel-and-ocr) run_case tunnel-and-ocr case_tunnel_and_ocr ;;
    stop-releases-server) run_case stop-releases-server case_stop ;;
    upload-local-models) run_case upload-local-models case_upload ;;
    rejects-bad-key) run_case rejects-bad-key case_bad_key ;;
    rejects-unreachable-host) run_case rejects-unreachable-host case_unreachable ;;
    *)
      echo "未知用例：$case_name" >&2
      FAIL=$((FAIL + 1))
      ;;
  esac
done

printf '\n%d passed, %d failed\n' "$PASS" "$FAIL"
printf '日志：%s\n' "$LOG_DIR"
[ "$FAIL" -eq 0 ]
