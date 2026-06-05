#!/bin/sh
# 单文件打包：macOS/MyWebDisk 与 Windows/MyWebDisk.exe（静态链接 webrpc SDK）
set -e

if [ -d "${HOME}/.cargo/bin" ]; then
	export PATH="${HOME}/.cargo/bin:${PATH}"
fi

DESKTOP_DIR="$(cd "$(dirname "$0")" && pwd)"
export CARGO_TARGET_DIR="${DESKTOP_DIR}/target"
DIST_DIR="${DESKTOP_DIR}/dist"
MACOS_APP="${DIST_DIR}/macos/MyWebDisk.app"
WIN_DIR="${DIST_DIR}/windows"

log() { printf '==> %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

command -v cargo >/dev/null 2>&1 || die "cargo not found (https://rustup.rs)"
command -v rustup >/dev/null 2>&1 || die "rustup not found — install via https://rustup.rs"

mkdir -p "${DIST_DIR}/macos" "${WIN_DIR}"

log "cargo build --release (host)"
(
	cd "${DESKTOP_DIR}"
	cargo build --release
)

HOST_BIN="${DESKTOP_DIR}/target/release/mywebdisk-desktop"
[ -f "${HOST_BIN}" ] || die "binary not found: ${HOST_BIN}"

pack_macos_app() {
	log "pack macOS app: ${MACOS_APP}"
	rm -rf "${MACOS_APP}"
	MACOS_BIN="${MACOS_APP}/Contents/MacOS"
	mkdir -p "${MACOS_BIN}" "${MACOS_APP}/Contents/Resources"
	cp -f "${HOST_BIN}" "${MACOS_BIN}/MyWebDisk"
	chmod +x "${MACOS_BIN}/MyWebDisk"
	cp -f "${DESKTOP_DIR}/packaging/Info.plist" "${MACOS_APP}/Contents/Info.plist"
	printf 'APPL????' >"${MACOS_APP}/Contents/PkgInfo"
	if command -v codesign >/dev/null 2>&1; then
		codesign -s - --force --deep "${MACOS_APP}" 2>/dev/null \
			|| warn "codesign skipped (app may need right-click Open on first run)"
	fi
}

pack_macos_app

build_windows_exe() {
	if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
		warn "mingw not found; skip Windows cross-build (brew install mingw-w64)"
		return 1
	fi
	log "cargo build --release --target x86_64-pc-windows-gnu"
	rustup target add x86_64-pc-windows-gnu
	(
		cd "${DESKTOP_DIR}"
		cargo build --release --target x86_64-pc-windows-gnu
	)
	WIN_BIN="${DESKTOP_DIR}/target/x86_64-pc-windows-gnu/release/mywebdisk-desktop.exe"
	[ -f "${WIN_BIN}" ] || return 1
	cp -f "${WIN_BIN}" "${WIN_DIR}/MyWebDisk.exe"
}

build_windows_exe || warn "Windows cross-build skipped or failed"

log "done"
printf '\nOutput:\n'
printf '  %s\n' "${MACOS_APP}"
[ -f "${WIN_DIR}/MyWebDisk.exe" ] && printf '  %s\n' "${WIN_DIR}/MyWebDisk.exe"
