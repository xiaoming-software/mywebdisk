#!/bin/sh
# 构建各平台单文件可执行程序（webrpc 自 sdk/*.a 嵌入，dist 目录无需附带动态库）
set -e

SERVER_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SERVER_DIR/.." && pwd)"
SDK_DIR="${ROOT_DIR}/sdk"
DIST_DIR="${SERVER_DIR}/dist"
RUNTIME_DIR="${SERVER_DIR}/internal/webrpc/runtimelib"

cd "$SERVER_DIR"

log() { printf '==> %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

HAS_ZIG=0
command -v zig >/dev/null 2>&1 && HAS_ZIG=1

need_file() { [ -f "$1" ] || die "missing: $1"; }

# 从静态库生成 embed 用运行时库
prepare_embed_darwin_arm64() {
  need_file "${SDK_DIR}/libwebrpc-Mac.a"
  mkdir -p "${RUNTIME_DIR}/darwin_arm64"
  log "embed lib: darwin_arm64"
  clang -arch arm64 -shared -o "${RUNTIME_DIR}/darwin_arm64/libwebrpc.dylib" \
    -Wl,-all_load "${SDK_DIR}/libwebrpc-Mac.a" \
    -framework Security -framework CoreFoundation
}

prepare_embed_linux_amd64() {
  need_file "${SDK_DIR}/libwebrpc-Linux.a"
  mkdir -p "${RUNTIME_DIR}/linux_amd64"
  log "embed lib: linux_amd64"
  if [ "$HAS_ZIG" -eq 1 ]; then
    zig cc -target x86_64-linux-gnu -shared -o "${RUNTIME_DIR}/linux_amd64/libwebrpc.so" \
      -Wl,--whole-archive "${SDK_DIR}/libwebrpc-Linux.a" -Wl,--no-whole-archive
  elif command -v x86_64-linux-gnu-gcc >/dev/null 2>&1; then
    x86_64-linux-gnu-gcc -shared -o "${RUNTIME_DIR}/linux_amd64/libwebrpc.so" \
      -Wl,--whole-archive "${SDK_DIR}/libwebrpc-Linux.a" -Wl,--no-whole-archive -lpthread
  else
    return 1
  fi
}

prepare_embed_linux_arm64() {
  need_file "${SDK_DIR}/libwebrpc-Linux-arm64.a"
  mkdir -p "${RUNTIME_DIR}/linux_arm64"
  log "embed lib: linux_arm64"
  if [ "$HAS_ZIG" -eq 1 ]; then
    zig cc -target aarch64-linux-gnu -shared -o "${RUNTIME_DIR}/linux_arm64/libwebrpc.so" \
      -Wl,--whole-archive "${SDK_DIR}/libwebrpc-Linux-arm64.a" -Wl,--no-whole-archive
  elif command -v aarch64-linux-gnu-gcc >/dev/null 2>&1; then
    aarch64-linux-gnu-gcc -shared -o "${RUNTIME_DIR}/linux_arm64/libwebrpc.so" \
      -Wl,--whole-archive "${SDK_DIR}/libwebrpc-Linux-arm64.a" -Wl,--no-whole-archive -lpthread
  else
    return 1
  fi
}

prepare_embed_windows_amd64() {
  need_file "${SDK_DIR}/libwebrpc-Windows.a"
  mkdir -p "${RUNTIME_DIR}/windows_amd64"
  log "embed lib: windows_amd64"
  if [ "$HAS_ZIG" -eq 1 ]; then
    zig cc -target x86_64-windows-gnu -shared -o "${RUNTIME_DIR}/windows_amd64/libwebrpc.dll" \
      -Wl,--whole-archive "${SDK_DIR}/libwebrpc-Windows.a" -Wl,--no-whole-archive
  elif command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
    x86_64-w64-mingw32-gcc -shared -o "${RUNTIME_DIR}/windows_amd64/libwebrpc.dll" \
      -Wl,--whole-archive "${SDK_DIR}/libwebrpc-Windows.a" -Wl,--no-whole-archive
  else
    return 1
  fi
}

go_build() {
  export CGO_ENABLED=1
  export GOOS=$1
  export GOARCH=$2
  export CC=$3
  go build -trimpath -ldflags="-s -w" -o "$4" .
}

build_darwin_arm64() {
  prepare_embed_darwin_arm64
  out="${DIST_DIR}/darwin-arm64/mywebdisk-server"
  mkdir -p "${DIST_DIR}/darwin-arm64"
  log "build darwin/arm64"
  go_build darwin arm64 "clang -arch arm64" "$out"
}

build_linux() {
  arch=$1
  outdir="${DIST_DIR}/linux-${arch}"
  mkdir -p "$outdir"
  case $arch in
    amd64)
      prepare_embed_linux_amd64 || return 1
      cc=$( [ "$HAS_ZIG" -eq 1 ] && echo "zig cc -target x86_64-linux-gnu" || echo "x86_64-linux-gnu-gcc" )
      ;;
    arm64)
      prepare_embed_linux_arm64 || return 1
      cc=$( [ "$HAS_ZIG" -eq 1 ] && echo "zig cc -target aarch64-linux-gnu" || echo "aarch64-linux-gnu-gcc" )
      ;;
  esac
  log "build linux/${arch}"
  go_build linux "$arch" "$cc" "${outdir}/mywebdisk-server"
}

build_windows() {
  prepare_embed_windows_amd64 || return 1
  out="${DIST_DIR}/windows-amd64/mywebdisk-server.exe"
  mkdir -p "${DIST_DIR}/windows-amd64"
  cc=$( [ "$HAS_ZIG" -eq 1 ] && echo "zig cc -target x86_64-windows-gnu" || echo "x86_64-w64-mingw32-gcc" )
  log "build windows/amd64"
  go_build windows amd64 "$cc" "$out"
}

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

BUILT=0
FAILED=0
try_build() { if "$@"; then BUILT=$((BUILT+1)); else FAILED=$((FAILED+1)); fi; }

HOST=$(uname -s)
if [ "$HOST" = "Darwin" ]; then
  try_build build_darwin_arm64
else
  warn "skip darwin/arm64 (build on macOS)"
  FAILED=$((FAILED+1))
fi

try_build build_linux amd64
try_build build_linux arm64
try_build build_windows

log "done: ${BUILT} binaries in ${DIST_DIR}"
[ "$BUILT" -eq 0 ] && die "nothing built"
[ "$FAILED" -gt 0 ] && warn "${FAILED} target(s) skipped"

find "$DIST_DIR" -type f \( -name 'mywebdisk-server' -o -name 'mywebdisk-server.exe' \) | sort | sed 's/^/  /'
exit 0
