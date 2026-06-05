#!/bin/sh
# 自动安装 mywebdisk 编译环境（macOS arm64）
# - Godot 4.6 编辑器 + Export Templates（桌面端）
# - zig、mingw-w64（服务端交叉编译，可选）
set -e

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOLS="${ROOT}/tools"
GODOT_APP="${TOOLS}/Godot.app"
GODOT_BIN="${GODOT_APP}/Contents/MacOS/Godot"
DESKTOP="${ROOT}/mywebdisk-desktop"
GODOT_VERSION="4.6-stable"
GODOT_TAG="4.6-stable"
# Godot 运行时查找目录为 4.6.stable（点号），与 release tag 4.6-stable（连字符）不同
GODOT_TEMPLATES_VERSION="4.6.stable"

GITHUB="https://github.com/godotengine/godot/releases/download/${GODOT_TAG}"
GODOT_ZIP="${GITHUB}/Godot_v${GODOT_VERSION}_macos.universal.zip"
TEMPLATES_TPZ="${GITHUB}/Godot_v${GODOT_VERSION}_export_templates.tpz"
TEMPLATES_BASE="${HOME}/Library/Application Support/Godot/export_templates"
TEMPLATES_DIR="${TEMPLATES_BASE}/${GODOT_TEMPLATES_VERSION}"
TEMPLATES_LEGACY="${TEMPLATES_BASE}/${GODOT_VERSION}"

log() { printf '==> %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }

need_cmd() {
	command -v "$1" >/dev/null 2>&1 || { echo "missing: $1"; exit 1; }
}

need_cmd curl
need_cmd unzip

mkdir -p "${TOOLS}" "${TEMPLATES_DIR}"

# ---------- Godot 编辑器 ----------
if [ -x "${GODOT_BIN}" ]; then
	log "Godot already installed: ${GODOT_BIN}"
else
	log "download Godot ${GODOT_VERSION} (macOS universal)"
	tmpzip="${TOOLS}/godot.zip"
	curl -fL --retry 3 --continue-at - -o "${tmpzip}" "${GODOT_ZIP}"
	rm -rf "${TOOLS}/Godot.app" "${TOOLS}/Godot_"*.app 2>/dev/null || true
	unzip -q -o "${tmpzip}" -d "${TOOLS}"
	rm -f "${tmpzip}"
	# 解压后可能是 Godot.app
	if [ ! -x "${GODOT_BIN}" ]; then
		found=$(find "${TOOLS}" -maxdepth 2 -name 'Godot.app' -type d 2>/dev/null | head -1)
		[ -n "$found" ] && GODOT_APP="$found" && GODOT_BIN="${GODOT_APP}/Contents/MacOS/Godot"
	fi
	[ -x "${GODOT_BIN}" ] || { echo "Godot binary not found after extract"; exit 1; }
	log "installed: ${GODOT_BIN}"
fi

# 写入桌面项目配置，build.sh 可直接使用
printf '%s\n' "${GODOT_APP}" > "${DESKTOP}/.godot-bin"
log "wrote ${DESKTOP}/.godot-bin"

# ---------- Export Templates ----------
if [ -f "${TEMPLATES_DIR}/version" ] || [ -d "${TEMPLATES_DIR}" ] && [ "$(ls -A "${TEMPLATES_DIR}" 2>/dev/null | head -1)" ]; then
	log "export templates already present: ${TEMPLATES_DIR}"
else
	log "download export templates"
	tmppz="${TOOLS}/export_templates.tpz"
	curl -fL --retry 3 --continue-at - -o "${tmppz}" "${TEMPLATES_TPZ}"
	rm -rf "${TOOLS}/_tpl_extract"
	mkdir -p "${TOOLS}/_tpl_extract" "${TEMPLATES_DIR}"
	unzip -q -o "${tmppz}" -d "${TOOLS}/_tpl_extract"
	# tpz 内可能是 templates/ 子目录或平铺文件
	if [ -d "${TOOLS}/_tpl_extract/templates" ]; then
		cp -R "${TOOLS}/_tpl_extract/templates/." "${TEMPLATES_DIR}/"
	else
		cp -R "${TOOLS}/_tpl_extract/." "${TEMPLATES_DIR}/"
	fi
	rm -rf "${TOOLS}/_tpl_extract" "${tmppz}"
	log "installed templates: ${TEMPLATES_DIR}"
fi

# 兼容旧脚本装到 4.6-stable 目录的情况：统一到 Godot 期望的 4.6.stable
if [ -d "${TEMPLATES_LEGACY}" ] && [ ! -e "${TEMPLATES_DIR}" ]; then
	log "link ${GODOT_TEMPLATES_VERSION} -> ${GODOT_VERSION}"
	ln -s "${TEMPLATES_LEGACY}" "${TEMPLATES_DIR}"
elif [ -d "${TEMPLATES_DIR}" ] && [ ! -d "${TEMPLATES_LEGACY}" ]; then
	ln -sf "${TEMPLATES_DIR}" "${TEMPLATES_LEGACY}" 2>/dev/null || true
fi

# ---------- Homebrew 交叉编译工具（服务端）----------
if command -v brew >/dev/null 2>&1; then
	log "install brew packages: zig mingw-w64 (for mywebdisk-server cross-build)"
	brew install zig mingw-w64 2>/dev/null || warn "brew install skipped or partial failure"
else
	warn "Homebrew not found; skip zig/mingw-w64 (server cross-compile may need them)"
fi

# ---------- 验证 ----------
log "verify Godot"
"${GODOT_BIN}" --version | head -3

log "verify Go"
if command -v go >/dev/null 2>&1; then
	go version
else
	warn "Go not in PATH; install from https://go.dev/dl/"
fi

log "environment ready"
printf '\nNext steps:\n'
printf '  cd %s/mywebdisk-desktop && ./build.sh\n' "${ROOT}"
printf '  cd %s/mywebdisk-server && ./build.sh\n' "${ROOT}"
