#!/bin/sh
# 编译各平台单文件可执行程序：
# - webrpc 动态库从 sdk/ 嵌入
# - 静态 ffmpeg 从 ffmpeg-static release 下载后临时替换占位文件并嵌入
set -e

ROOT="$(cd "$(dirname "$0")" && pwd)"
SDK_DIR="${ROOT}/sdk"
DIST_DIR="${ROOT}/dist"
RUNTIME_DIR="${ROOT}/internal/webrpc/runtimelib"
FFMPEG_DIR="${ROOT}/internal/nas/ffmpeglib"
FFMPEG_CACHE="${ROOT}/.ffmpeg-cache"
# Keep in sync with internal/nas/ffmpeg_bin.go ffmpegEmbedTag.
FFMPEG_VER="b6.1.1"
FFMPEG_BASE="https://github.com/eugeneware/ffmpeg-static/releases/download/${FFMPEG_VER}"

cd "$ROOT"

log() { printf '==> %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

command -v go >/dev/null 2>&1 || die "未找到 go，请先安装: https://go.dev/dl/"
command -v curl >/dev/null 2>&1 || die "未找到 curl"
command -v gzip >/dev/null 2>&1 || die "未找到 gzip"

need_file() { [ -f "$1" ] || die "缺少 SDK 文件: $1"; }

prepare_embed() {
  os=$1
  src=$2
  name=$3
  dest="${RUNTIME_DIR}/${os}/${name}"
  need_file "$src"
  mkdir -p "$(dirname "$dest")"
  cp "$src" "$dest"
  chmod 0755 "$dest"
}

# Download static ffmpeg into cache, then overwrite the committed placeholder
# so //go:embed picks up the real binary. Caller must restore afterward.
install_ffmpeg_for_embed() {
  platform=$1
  remote=$2
  outname=$3
  dest_dir="${FFMPEG_DIR}/${platform}"
  dest="${dest_dir}/${outname}"
  placeholder="${dest}.placeholder"
  cache_gz="${FFMPEG_CACHE}/${FFMPEG_VER}-${platform}.gz"
  mkdir -p "$dest_dir" "$FFMPEG_CACHE"

  if [ ! -f "$placeholder" ]; then
    # First run: preserve current (small) file as placeholder.
    [ -f "$dest" ] || die "缺少 ffmpeg 占位文件: $dest"
    cp "$dest" "$placeholder"
  fi

  if [ ! -f "$cache_gz" ] || [ ! -s "$cache_gz" ]; then
    log "download ffmpeg ${platform} (${FFMPEG_VER})"
    curl -fsSL -o "${cache_gz}.part" "${FFMPEG_BASE}/${remote}"
    mv "${cache_gz}.part" "$cache_gz"
  fi
  log "embed ffmpeg ${platform}"
  gzip -dc "$cache_gz" > "${dest}.tmp"
  mv "${dest}.tmp" "$dest"
  chmod 0755 "$dest"
  # Sanity: static builds are many MB; placeholders are tiny.
  size=$(wc -c < "$dest" | tr -d ' ')
  if [ "$size" -lt 1000000 ]; then
    die "ffmpeg ${platform} 看起来不是静态二进制 (size=${size})，请检查下载"
  fi
}

restore_ffmpeg_placeholder() {
  platform=$1
  outname=$2
  dest="${FFMPEG_DIR}/${platform}/${outname}"
  placeholder="${dest}.placeholder"
  if [ -f "$placeholder" ]; then
    cp "$placeholder" "$dest"
    chmod 0755 "$dest" 2>/dev/null || true
  fi
}

restore_all_ffmpeg_placeholders() {
  restore_ffmpeg_placeholder darwin_arm64 ffmpeg
  restore_ffmpeg_placeholder linux_amd64 ffmpeg
  restore_ffmpeg_placeholder linux_arm64 ffmpeg
  restore_ffmpeg_placeholder windows_amd64 ffmpeg.exe
}

# Always restore placeholders even if a build step fails.
trap restore_all_ffmpeg_placeholders EXIT

go_build() {
  goos=$1
  goarch=$2
  outfile=$3
  export GOOS="$goos"
  export GOARCH="$goarch"
  export CGO_ENABLED=0
  unset CC
  go build -trimpath -ldflags="-s -w" -o "$outfile" .
}

build_darwin_arm64() {
  prepare_embed darwin_arm64 "${SDK_DIR}/libwebrpc-Mac.dylib" libwebrpc.dylib
  install_ffmpeg_for_embed darwin_arm64 "ffmpeg-darwin-arm64.gz" ffmpeg
  out="${DIST_DIR}/darwin-arm64/mywebdisk-server"
  mkdir -p "${DIST_DIR}/darwin-arm64"
  log "build darwin/arm64"
  go_build darwin arm64 "$out"
  restore_ffmpeg_placeholder darwin_arm64 ffmpeg
}

build_linux() {
  arch=$1
  case $arch in
    amd64)
      prepare_embed linux_amd64 "${SDK_DIR}/libwebrpc-Linux.so" libwebrpc.so
      install_ffmpeg_for_embed linux_amd64 "ffmpeg-linux-x64.gz" ffmpeg
      ;;
    arm64)
      prepare_embed linux_arm64 "${SDK_DIR}/libwebrpc-Linux-arm64.so" libwebrpc.so
      install_ffmpeg_for_embed linux_arm64 "ffmpeg-linux-arm64.gz" ffmpeg
      ;;
    *)
      return 1
      ;;
  esac
  outdir="${DIST_DIR}/linux-${arch}"
  mkdir -p "$outdir"
  log "build linux/${arch}"
  go_build linux "$arch" "${outdir}/mywebdisk-server"
  restore_ffmpeg_placeholder "linux_${arch}" ffmpeg
}

build_windows_amd64() {
  prepare_embed windows_amd64 "${SDK_DIR}/webrpc-Windows.dll" libwebrpc.dll
  install_ffmpeg_for_embed windows_amd64 "ffmpeg-win32-x64.gz" ffmpeg.exe
  out="${DIST_DIR}/windows-amd64/mywebdisk-server.exe"
  mkdir -p "${DIST_DIR}/windows-amd64"
  log "build windows/amd64"
  go_build windows amd64 "$out"
  restore_ffmpeg_placeholder windows_amd64 ffmpeg.exe
}

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR"

BUILT=0
FAILED=0
try_build() {
  if "$@"; then
    BUILT=$((BUILT + 1))
  else
    FAILED=$((FAILED + 1))
  fi
}

try_build build_darwin_arm64
try_build build_linux amd64
try_build build_linux arm64
try_build build_windows_amd64

log "完成: ${BUILT} 个可执行文件 -> ${DIST_DIR}"
[ "$BUILT" -eq 0 ] && die "没有成功编译任何平台"
[ "$FAILED" -gt 0 ] && warn "${FAILED} 个目标未编译"

find "$DIST_DIR" -type f \( -name 'mywebdisk-server' -o -name 'mywebdisk-server.exe' \) | sort | sed 's/^/  /'
find "$DIST_DIR" -type f \( -name 'mywebdisk-server' -o -name 'mywebdisk-server.exe' \) -exec ls -lh {} \; | awk '{print "  size", $5, $9}'
exit 0
