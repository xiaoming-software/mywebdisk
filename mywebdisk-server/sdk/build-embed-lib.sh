#!/bin/sh
# 从 .a 生成各平台 embed 用动态库（仅构建阶段使用，不随 dist 分发）
set -e
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SDK="$(cd "$(dirname "$0")" && pwd)"
OUT="${ROOT}/internal/webrpc/runtimelib"

OS="$(uname -s)"
ARCH="$(uname -m)"

mkdir -p "$OUT"

case "$OS" in
  Darwin)
    [ "$ARCH" = "arm64" ] || { echo "darwin embed lib: arm64 only"; exit 1; }
    mkdir -p "${OUT}/darwin_arm64"
    clang -arch arm64 -shared -o "${OUT}/darwin_arm64/libwebrpc.dylib" \
      -Wl,-all_load "${SDK}/libwebrpc-Mac.a" \
      -framework Security -framework CoreFoundation
    echo "embedded: darwin_arm64/libwebrpc.dylib"
    ;;
  Linux)
    case "$ARCH" in
      aarch64|arm64)
        mkdir -p "${OUT}/linux_arm64"
        cc -shared -o "${OUT}/linux_arm64/libwebrpc.so" \
          -Wl,--whole-archive "${SDK}/libwebrpc-Linux-arm64.a" -Wl,--no-whole-archive -lpthread
        echo "embedded: linux_arm64/libwebrpc.so"
        ;;
      *)
        mkdir -p "${OUT}/linux_amd64"
        cc -shared -o "${OUT}/linux_amd64/libwebrpc.so" \
          -Wl,--whole-archive "${SDK}/libwebrpc-Linux.a" -Wl,--no-whole-archive -lpthread
        echo "embedded: linux_amd64/libwebrpc.so"
        ;;
    esac
    ;;
  *)
    echo "run on target OS or extend sdk/build-embed-lib.sh for cross-build"
    exit 1
    ;;
esac
