#!/usr/bin/env sh
# astral-core 一键安装（Linux / macOS）
#
# 最短:
#   curl -fsSL https://raw.githubusercontent.com/AstralNext/astral-core/main/scripts/get.sh | sh
#
# 或 Release 附件:
#   curl -fsSL https://github.com/AstralNext/astral-core/releases/latest/download/get.sh | sh
#
# 常用环境变量 / 参数:
#   ASTRAL_VERSION=v0.1.0   指定版本（默认 latest）
#   ASTRAL_SERVICE=0        只下载不装服务（默认装）
#   ASTRAL_LISTEN=127.0.0.1:50051
#   ASTRAL_NAME=default
#   ASTRAL_CONTROLLER=https://...
#   ASTRAL_CONTROLLER_TOKEN=...
#   ASTRAL_REPO=AstralNext/astral-core
#
#   curl -fsSL .../get.sh | sh -s -- --no-service
#   curl -fsSL .../get.sh | sh -s -- --listen 0.0.0.0:50051 --controller http://x:8443 --token secret

set -eu

REPO="${ASTRAL_REPO:-AstralNext/astral-core}"
VERSION="${ASTRAL_VERSION:-latest}"
NAME="${ASTRAL_NAME:-default}"
LISTEN="${ASTRAL_LISTEN:-127.0.0.1:50051}"
CONTROLLER="${ASTRAL_CONTROLLER:-}"
CONTROLLER_TOKEN="${ASTRAL_CONTROLLER_TOKEN:-}"
CONTROLLER_TLS_CA="${ASTRAL_CONTROLLER_TLS_CA:-}"
CONTROLLER_TLS_DOMAIN="${ASTRAL_CONTROLLER_TLS_DOMAIN:-}"
SERVICE="${ASTRAL_SERVICE:-1}"
PREFIX="${ASTRAL_PREFIX:-}"

info() { printf '[*] %s\n' "$*"; }
ok() { printf '[+] %s\n' "$*"; }
warn() { printf '[!] %s\n' "$*"; }
die() { printf '[x] %s\n' "$*" >&2; exit 1; }

while [ $# -gt 0 ]; do
  case "$1" in
    --no-service) SERVICE=0; shift ;;
    --service) SERVICE=1; shift ;;
    --version) VERSION="$2"; shift 2 ;;
    --name) NAME="$2"; shift 2 ;;
    --listen) LISTEN="$2"; shift 2 ;;
    --controller) CONTROLLER="$2"; shift 2 ;;
    --token|--controller-token) CONTROLLER_TOKEN="$2"; shift 2 ;;
    --tls-ca) CONTROLLER_TLS_CA="$2"; shift 2 ;;
    --tls-domain) CONTROLLER_TLS_DOMAIN="$2"; shift 2 ;;
    --prefix) PREFIX="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,20p' "$0" 2>/dev/null || true
      exit 0
      ;;
    *) die "未知参数: $1" ;;
  esac
done

need_cmd() { command -v "$1" >/dev/null 2>&1 || die "缺少命令: $1"; }
need_cmd uname
need_cmd mktemp

download() {
  url="$1"
  out="$2"
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$url" -o "$out"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$out" "$url"
  else
    die "需要 curl 或 wget"
  fi
}

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64) ARCH=x86_64 ;;
  aarch64|arm64) ARCH=aarch64 ;;
  *) die "不支持的架构: $ARCH" ;;
esac

case "$OS" in
  linux) ASSET="astral-core-linux-${ARCH}" ;;
  darwin) ASSET="astral-core-macos-${ARCH}" ;;
  *) die "不支持的系统: $OS (Windows: irm .../get.ps1 | iex)" ;;
esac

if [ -z "$PREFIX" ]; then
  if [ "$(id -u)" -eq 0 ]; then
    PREFIX=/usr/local
  else
    PREFIX="${HOME}/.local"
  fi
fi
BIN_DIR="${PREFIX}/bin"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
APP_DIR="${DATA_HOME}/astral-core"
mkdir -p "$BIN_DIR" "$APP_DIR"

if [ "$VERSION" = "latest" ]; then
  BASE="https://github.com/${REPO}/releases/latest/download"
else
  case "$VERSION" in
    v*) ;;
    *) VERSION="v${VERSION}" ;;
  esac
  BASE="https://github.com/${REPO}/releases/download/${VERSION}"
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

info "正在下载 ${ASSET} (${VERSION})"
download "${BASE}/${ASSET}" "${TMP}/astral-core"
chmod +x "${TMP}/astral-core"
install -m 755 "${TMP}/astral-core" "${BIN_DIR}/astral-core"
ok "已安装 ${BIN_DIR}/astral-core"

case ":$PATH:" in
  *":${BIN_DIR}:"*) ;;
  *) warn "请将下列目录加入 PATH: export PATH=\"${BIN_DIR}:\$PATH\"" ;;
esac

if [ "$SERVICE" != "1" ]; then
  ok "完成（仅二进制）。运行: astral-core --listen ${LISTEN}"
  exit 0
fi

ARGS="service install --name ${NAME} --listen ${LISTEN} --program ${BIN_DIR}/astral-core"
if [ -n "$CONTROLLER" ]; then
  [ -n "$CONTROLLER_TOKEN" ] || die "启用控制端时需要 --token / ASTRAL_CONTROLLER_TOKEN"
  ARGS="$ARGS --controller ${CONTROLLER} --controller-token ${CONTROLLER_TOKEN}"
  [ -n "$CONTROLLER_TLS_CA" ] && ARGS="$ARGS --controller-tls-ca ${CONTROLLER_TLS_CA}"
  [ -n "$CONTROLLER_TLS_DOMAIN" ] && ARGS="$ARGS --controller-tls-domain ${CONTROLLER_TLS_DOMAIN}"
fi

info "正在安装系统服务（可能需要 sudo）"
# shellcheck disable=SC2086
if [ "$(id -u)" -eq 0 ]; then
  ${BIN_DIR}/astral-core $ARGS
elif command -v sudo >/dev/null 2>&1; then
  sudo ${BIN_DIR}/astral-core $ARGS
else
  warn "非 root 且无 sudo：二进制已安装，已跳过服务"
  warn "请用 root 执行: astral-core $ARGS"
  exit 0
fi

ok "服务已安装: dev.astral.core-${NAME}"
echo "监听: ${LISTEN}"
echo "令牌: 数据目录 bootstrap_token.txt（首次启动后生成）"
echo "状态: astral-core service status --name ${NAME}"
