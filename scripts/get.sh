#!/usr/bin/env sh
# astral-core 一键安装（Linux / macOS）
#
#   curl -fsSL https://raw.githubusercontent.com/AstralNext/astral-core/main/scripts/get.sh | sh
#
# 环境变量:
#   ASTRAL_VERSION=v0.1.0
#   ASTRAL_SERVICE=0        只下载不装服务
#   ASTRAL_LISTEN=127.0.0.1:50051
#   ASTRAL_REPO=AstralNext/astral-core

set -eu

REPO="${ASTRAL_REPO:-AstralNext/astral-core}"
VERSION="${ASTRAL_VERSION:-latest}"
LISTEN="${ASTRAL_LISTEN:-127.0.0.1:50051}"
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
    --listen) LISTEN="$2"; shift 2 ;;
    --prefix) PREFIX="$2"; shift 2 ;;
    -h|--help)
      sed -n '2,12p' "$0" 2>/dev/null || true
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

ARGS="service install --listen ${LISTEN} --program ${BIN_DIR}/astral-core --user"

info "正在安装用户级系统服务"
# shellcheck disable=SC2086
${BIN_DIR}/astral-core $ARGS

ok "服务已安装: dev.astral.core-default"
echo "监听: ${LISTEN}（仅本机）"
echo "状态: astral-core service status --user"
