#!/bin/sh
# 将 dist/ 里已编译好的各平台程序发布到 GitHub Release（不重新编译）。
# 以后改好下面的 VERSION，执行 ./publish-release.sh 即可发布该版本。
#
# 依赖: git、gh（已登录 github.com）
# 产物来源:
#   dist/darwin-arm64/mywebdisk-server
#   dist/linux-amd64/mywebdisk-server
#   dist/linux-arm64/mywebdisk-server
#   dist/windows-amd64/mywebdisk-server.exe

# ---- 每次发布只改这里 ----
VERSION=1.2.1
# --------------------------

set -e

ROOT="$(cd "$(dirname "$0")" && pwd)"
DIST_DIR="${ROOT}/dist"
STAGE="${ROOT}/.release-stage"

cd "$ROOT"

log() { printf '==> %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

# 允许写成 1.0.0 或 v1.0.0
case "$VERSION" in
  "") die "VERSION 为空" ;;
  v*|V*) VERSION="${VERSION#v}"; VERSION="${VERSION#V}" ;;
esac
TAG="v${VERSION}"

command -v git >/dev/null 2>&1 || die "未找到 git"
command -v gh >/dev/null 2>&1 || die "未找到 gh，请安装 GitHub CLI: https://cli.github.com/"
gh auth status -h github.com >/dev/null 2>&1 || die "gh 未登录或 token 无效，请先执行: gh auth login -h github.com"

need_file() { [ -f "$1" ] || die "缺少编译产物: $1（请先 ./build.sh）"; }

need_file "${DIST_DIR}/darwin-arm64/mywebdisk-server"
need_file "${DIST_DIR}/linux-amd64/mywebdisk-server"
need_file "${DIST_DIR}/linux-arm64/mywebdisk-server"
need_file "${DIST_DIR}/windows-amd64/mywebdisk-server.exe"

REPO="$(gh repo view --json nameWithOwner -q .nameWithOwner 2>/dev/null || true)"
[ -n "$REPO" ] || die "无法识别 GitHub 仓库，请确认 origin 已指向 GitHub 且 gh 有权限"

if git ls-remote --exit-code --tags origin "refs/tags/${TAG}" >/dev/null 2>&1; then
  die "远程已存在标签 ${TAG}，请改 VERSION 后再发布"
fi
if git show-ref --verify --quiet "refs/tags/${TAG}"; then
  die "本地已存在标签 ${TAG}，请改 VERSION 或删除该标签后再发布"
fi
if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
  die "GitHub 上已有 Release ${TAG}"
fi

if [ -n "$(git status --porcelain)" ]; then
  warn "工作区有未提交改动，仍将用当前 dist/ 文件创建 ${TAG}"
fi

rm -rf "$STAGE"
mkdir -p "$STAGE"

DARWIN="${STAGE}/mywebdisk-server-${TAG}-darwin-arm64"
LINUX64="${STAGE}/mywebdisk-server-${TAG}-linux-amd64"
LINUXARM="${STAGE}/mywebdisk-server-${TAG}-linux-arm64"
WIN="${STAGE}/mywebdisk-server-${TAG}-windows-amd64.exe"

cp "${DIST_DIR}/darwin-arm64/mywebdisk-server" "$DARWIN"
cp "${DIST_DIR}/linux-amd64/mywebdisk-server" "$LINUX64"
cp "${DIST_DIR}/linux-arm64/mywebdisk-server" "$LINUXARM"
cp "${DIST_DIR}/windows-amd64/mywebdisk-server.exe" "$WIN"
chmod 0755 "$DARWIN" "$LINUX64" "$LINUXARM"

SUMS="${STAGE}/SHA256SUMS.txt"
if command -v shasum >/dev/null 2>&1; then
  (cd "$STAGE" && shasum -a 256 mywebdisk-server-* > SHA256SUMS.txt)
elif command -v sha256sum >/dev/null 2>&1; then
  (cd "$STAGE" && sha256sum mywebdisk-server-* > SHA256SUMS.txt)
else
  die "未找到 shasum 或 sha256sum"
fi

NOTES="${STAGE}/notes.md"
cat > "$NOTES" <<EOF
# MyWebDisk ${TAG}

个人 NAS / 私人网盘服务端预编译包（webrpc P2P，单文件，无需再附带动态库）。

## 下载

| 平台 | 架构 | 文件 |
| --- | --- | --- |
| macOS 11+ | Apple Silicon (arm64) | \`mywebdisk-server-${TAG}-darwin-arm64\` |
| Linux | x86_64 (amd64) | \`mywebdisk-server-${TAG}-linux-amd64\` |
| Linux | ARM64 | \`mywebdisk-server-${TAG}-linux-arm64\` |
| Windows 10/11 | x64 | \`mywebdisk-server-${TAG}-windows-amd64.exe\` |

校验和见附件 \`SHA256SUMS.txt\`。

## 运行

\`\`\`bash
chmod +x mywebdisk-server-${TAG}-linux-amd64
./mywebdisk-server-${TAG}-linux-amd64 \\
  --token=TOKEN \\
  --passwd=PASSWD \\
  --permission=PERMISSION \\
  --path=/path/to/nas
\`\`\`

电脑端请使用 [File2File Desktop](https://github.com/xiaoming-software/File2File-Desktop) 连接网盘。  
手机端请使用 [File2File App](https://github.com/xiaoming-software/File2File-App) 连接网盘。
EOF

HEAD="$(git rev-parse HEAD)"
log "仓库: ${REPO}"
log "版本: ${TAG}  (commit ${HEAD})"
log "上传资源:"
ls -lh "$DARWIN" "$LINUX64" "$LINUXARM" "$WIN" "$SUMS" | awk '{print "  " $0}'

gh release create "$TAG" \
  --repo "$REPO" \
  --target "$HEAD" \
  --title "MyWebDisk ${TAG}" \
  --notes-file "$NOTES" \
  --latest \
  "$DARWIN" "$LINUX64" "$LINUXARM" "$WIN" "$SUMS"

rm -rf "$STAGE"

URL="https://github.com/${REPO}/releases/tag/${TAG}"
log "已发布: ${URL}"
printf '%s\n' "$URL"
