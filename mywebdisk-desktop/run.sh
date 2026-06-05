#!/bin/sh
# 以 GUI 方式启动（不占用当前终端窗口）
set -e

DESKTOP_DIR="$(cd "$(dirname "$0")" && pwd)"
APP="${DESKTOP_DIR}/dist/macos/MyWebDisk.app"

if [ ! -d "${APP}" ]; then
	printf '==> app not found, running build.sh...\n'
	"${DESKTOP_DIR}/build.sh"
fi

exec open "${APP}"
