#!/bin/sh
# 本地快速开发构建（静态链接 webrpc SDK）
set -e

DESKTOP_DIR="$(cd "$(dirname "$0")" && pwd)"

if [ -d "${HOME}/.cargo/bin" ]; then
	export PATH="${HOME}/.cargo/bin:${PATH}"
fi
export CARGO_TARGET_DIR="${DESKTOP_DIR}/target"

log() { printf '==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

command -v cargo >/dev/null 2>&1 || die "cargo not found"

log "cargo build (debug)"
(
	cd "${DESKTOP_DIR}"
	cargo build
)

log "done (fast dev build)"
