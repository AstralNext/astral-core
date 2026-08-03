#!/usr/bin/env bash
# astral-core 本地部署向导（Linux/macOS）
# 用法:
#   ./scripts/install-wizard.sh              # 菜单
#   ./scripts/install-wizard.sh install      # 交互安装
#   ./scripts/install-wizard.sh status
#   NONINTERACTIVE=1 NAME=default LISTEN=127.0.0.1:50051 \
#     ./scripts/install-wizard.sh install

set -euo pipefail

ACTION="${1:-menu}"
NAME="${NAME:-default}"
LISTEN="${LISTEN:-127.0.0.1:50051}"
DATA_DIR="${DATA_DIR:-}"
INSTALL_ROOT="${INSTALL_ROOT:-}"
PROGRAM="${PROGRAM:-}"
VERSION="${VERSION:-}"
CONTROLLER="${CONTROLLER:-}"
CONTROLLER_TOKEN="${CONTROLLER_TOKEN:-}"
CONTROLLER_TLS_CA="${CONTROLLER_TLS_CA:-}"
CONTROLLER_TLS_DOMAIN="${CONTROLLER_TLS_DOMAIN:-}"
RETAIN="${RETAIN:-3}"
NO_START="${NO_START:-0}"
NONINTERACTIVE="${NONINTERACTIVE:-0}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

info() { printf '[*] %s\n' "$*"; }
ok() { printf '[+] %s\n' "$*"; }
warn() { printf '[!] %s\n' "$*"; }
err() { printf '[x] %s\n' "$*" >&2; }

find_exe() {
  local c
  for c in \
    "$PROGRAM" \
    "$ROOT/target/release/astral-core" \
    "$ROOT/target/debug/astral-core" \
    "./astral-core" \
    "$(command -v astral-core 2>/dev/null || true)"; do
    [[ -n "$c" && -x "$c" ]] && { printf '%s' "$c"; return 0; }
  done
  return 1
}

ask() {
  local prompt="$1" default="${2:-}"
  if [[ "$NONINTERACTIVE" == "1" ]]; then
    printf '%s' "$default"
    return
  fi
  local v
  if [[ -n "$default" ]]; then
    read -r -p "$prompt [$default]: " v || true
  else
    read -r -p "$prompt: " v || true
  fi
  printf '%s' "${v:-$default}"
}

ask_yes() {
  local prompt="$1" default="${2:-y}"
  if [[ "$NONINTERACTIVE" == "1" ]]; then
    [[ "$default" == "y" ]]
    return
  fi
  local v
  read -r -p "$prompt [y/N]: " v || true
  v="${v:-$default}"
  [[ "$v" == "y" || "$v" == "Y" || "$v" == "yes" ]]
}

need_root_hint() {
  if [[ "$(uname -s)" == "Linux" ]] && [[ "$EUID" -ne 0 ]]; then
    warn "systemd 系统级服务通常需要 sudo；若失败请: sudo $0 $*"
  fi
}

run_core() {
  local exe="$1"; shift
  info "$exe $*"
  "$exe" "$@"
}

do_install() {
  local exe="$1"
  echo
  echo "=== astral-core 安装向导 ==="
  echo
  NAME="$(ask '实例名' "$NAME")"
  LISTEN="$(ask '本地 gRPC 监听' "$LISTEN")"
  DATA_DIR="$(ask '数据目录（空=默认）' "$DATA_DIR")"
  INSTALL_ROOT="$(ask '安装根（空=默认）' "$INSTALL_ROOT")"
  PROGRAM="$(ask 'astral-core 路径' "$exe")"
  exe="$PROGRAM"

  if ask_yes '是否配置出站控制端？' n || [[ -n "$CONTROLLER" ]]; then
    CONTROLLER="$(ask '控制端 URL' "${CONTROLLER:-http://127.0.0.1:8443}")"
    CONTROLLER_TOKEN="$(ask '控制端 token' "$CONTROLLER_TOKEN")"
    [[ -n "$CONTROLLER_TOKEN" ]] || { err '必须提供 CONTROLLER_TOKEN'; exit 1; }
    if [[ "$CONTROLLER" == https://* ]]; then
      CONTROLLER_TLS_CA="$(ask 'TLS CA PEM（可空）' "$CONTROLLER_TLS_CA")"
      CONTROLLER_TLS_DOMAIN="$(ask 'TLS 域名（可空）' "$CONTROLLER_TLS_DOMAIN")"
    fi
  fi

  if [[ "$NONINTERACTIVE" != "1" ]]; then
    if ask_yes '安装后立即启动？' y; then NO_START=0; else NO_START=1; fi
    ask_yes '确认安装？' y || { warn '已取消'; return; }
  fi

  need_root_hint install
  local args=(service install --name "$NAME" --listen "$LISTEN" --program "$exe" --retain "$RETAIN")
  [[ -n "$DATA_DIR" ]] && args+=(--data-dir "$DATA_DIR")
  [[ -n "$INSTALL_ROOT" ]] && args+=(--install-root "$INSTALL_ROOT")
  [[ -n "$VERSION" ]] && args+=(--version "$VERSION")
  [[ "$NO_START" == "1" ]] && args+=(--no-start)
  if [[ -n "$CONTROLLER" ]]; then
    args+=(--controller "$CONTROLLER" --controller-token "$CONTROLLER_TOKEN")
    [[ -n "$CONTROLLER_TLS_CA" ]] && args+=(--controller-tls-ca "$CONTROLLER_TLS_CA")
    [[ -n "$CONTROLLER_TLS_DOMAIN" ]] && args+=(--controller-tls-domain "$CONTROLLER_TLS_DOMAIN")
  fi
  run_core "$exe" "${args[@]}"
  ok "服务已安装: dev.astral.core-$NAME"
  echo "引导 Token: 数据目录下 bootstrap_token.txt"
  echo "连接: $LISTEN  Authorization: Bearer <token>"
}

do_simple() {
  local exe="$1" sub="$2"
  need_root_hint "$sub"
  run_core "$exe" service "$sub" --name "$NAME"
}

do_update() {
  local exe="$1"
  need_root_hint update
  local prog="${PROGRAM:-$exe}"
  local args=(service update --program "$prog" --retain "$RETAIN")
  [[ -n "$VERSION" ]] && args+=(--version "$VERSION")
  [[ -n "$INSTALL_ROOT" ]] && args+=(--install-root "$INSTALL_ROOT")
  [[ "$NO_START" == "1" ]] && args+=(--no-start)
  run_core "$exe" "${args[@]}"
  ok '更新完成'
}

menu() {
  echo
  echo 'astral-core 本地部署'
  echo '  1) 安装为系统服务'
  echo '  2) 启动'
  echo '  3) 停止'
  echo '  4) 状态'
  echo '  5) 更新'
  echo '  6) 版本列表'
  echo '  7) 卸载'
  echo '  0) 退出'
  echo
  local c
  read -r -p '选择: ' c || true
  case "$c" in
    1) ACTION=install ;;
    2) ACTION=start ;;
    3) ACTION=stop ;;
    4) ACTION=status ;;
    5) ACTION=update ;;
    6) ACTION=versions ;;
    7) ACTION=uninstall ;;
    *) exit 0 ;;
  esac
}

EXE="$(find_exe)" || { err '未找到 astral-core，请 cargo build --release 或设置 PROGRAM='; exit 1; }
info "使用二进制: $EXE"

[[ "$ACTION" == "menu" && "$NONINTERACTIVE" != "1" ]] && menu

case "$ACTION" in
  install) do_install "$EXE" ;;
  uninstall) do_simple "$EXE" uninstall; ok 已卸载 ;;
  start) do_simple "$EXE" start; ok 已启动 ;;
  stop) do_simple "$EXE" stop; ok 已停止 ;;
  status) do_simple "$EXE" status ;;
  update) do_update "$EXE" ;;
  versions) run_core "$EXE" service versions ;;
  *) err "未知动作: $ACTION"; exit 1 ;;
esac
