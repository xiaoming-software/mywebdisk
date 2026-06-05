# MyWebDisk

基于 **webrpc P2P** 的个人网盘系统。远程客户端通过点对点加密通道，直接读写服务端指定目录下的文件，无需公网 IP 或自建 HTTP 服务。

```
mywebdisk/
├── sdk/                 # 全平台共用的 webrpc 静态库与头文件
├── mywebdisk-server/    # Go 服务端：工作区文件 API + webrpc 接入
├── mywebdisk-desktop/   # Rust + egui 桌面客户端
├── mywebdisk-app/       # 移动端客户端（预留，Godot 工程目录）
├── scripts/             # 环境安装脚本
└── test.sh              # 桌面端运行与日志采集
```

---

## 项目介绍

| 子项目 | 技术栈 | 说明 |
|--------|--------|------|
| **mywebdisk-server** | Go + webrpc SDK | 在本地目录上提供网盘文件操作，等待远程客户端 P2P 连接 |
| **mywebdisk-desktop** | Rust + egui + webrpc SDK | 跨平台桌面客户端，登录、连接远程网盘、浏览与管理文件 |
| **mywebdisk-app** | Godot（预留） | 移动端客户端工程目录，当前为空，后续可扩展 iOS / Android |
| **sdk** | C 静态库 | `WebrpcClient_*` API，供 server / desktop 静态或动态链接 |

通信链路：**桌面客户端 ↔ webrpc P2P ↔ 服务端**，控制指令走 JSON 数据流，大文件走文件流（约 10MB/桶，自动多桶传输）。

---

## 功能特性

### 服务端（mywebdisk-server）

- 指定本地目录作为网盘工作区（`--path`）
- 通过 webrpc 注册 token / passwd / permission，等待远程连接
- 支持 JSON 控制操作：
  - `list` / `stat` — 列目录、查文件信息
  - `mkdir` / `delete` / `rename` — 新建文件夹、删除、重命名
  - `download` — 多桶 SendFile 下载
  - `upload` + `upload_done` — 多桶 SendFile 上传（>10MB 自动分桶）
  - `write` — 文本内容写入（在线编辑保存，单文件上限 50MB）
  - `download_stream` — 流媒体分块下载
  - `health` — 健康检查
- 路径安全：相对工作区根目录，禁止 `../` 越界
- 单文件可执行程序：webrpc 运行时库通过 `go:embed` 打包，**dist 无需额外附带 .dylib/.so/.dll**

### 桌面客户端（mywebdisk-desktop）

- **登录与会话**：token + passwd 登录 webrpc；可连接多个远程网盘会话
- **资源管理器**（Finder 风格）：
  - 目录浏览、返回上级、刷新
  - 上传文件（系统选文件 → 进度条 → 成功提示）
  - 右键空白区新建文件夹 / 新建文件
  - 每行操作：下载到本地、删除
- **在线预览与编辑**：
  - 图片预览（≤50MB）
  - 文本预览与保存（≤50MB，UTF-8）
  - 视频 / 音频在线播放（≤200MB，系统默认播放器）
  - Office 文档：下载到临时目录 → 系统打开 → 关闭会话时检测修改并自动回写
- **界面**：霓虹蓝绿主题、中英文切换
- **打包**：macOS `.app`、Windows `.exe`，静态链接 webrpc SDK

### 移动端（mywebdisk-app）

当前为**预留目录**，尚未实现。`scripts/setup-env.sh` 已包含 Godot 4.6 编辑器与导出模板安装，便于后续开发移动端客户端。

---

## 特性与优势

1. **真正的 P2P**：基于 webrpc 点对点网络，不依赖中心服务器转发文件流量
2. **单文件部署**：server 与 desktop 构建产物均为单可执行文件，携带 webrpc 运行时，分发简单
3. **大文件友好**：SendFile 按约 10MB/桶传输，上传与下载均支持多桶，可稳定处理 GB 级文件
4. **完整文件管理**：浏览、上传、下载、新建、删除、重命名、在线预览与编辑
5. **跨平台**：server 支持 macOS / Linux / Windows；desktop 支持 macOS / Windows
6. **安全边界**：工作区路径校验，防止目录穿越
7. **轻量 UI**：Rust + egui 原生桌面应用，无需浏览器

---

## 快速开始

### 1. 编译服务端

```bash
cd mywebdisk-server
./build.sh
```

产物示例：

| 路径 | 平台 |
|------|------|
| `dist/darwin-arm64/mywebdisk-server` | macOS Apple Silicon |
| `dist/linux-amd64/mywebdisk-server` | Linux x86_64 |
| `dist/linux-arm64/mywebdisk-server` | Linux arm64 |
| `dist/windows-amd64/mywebdisk-server.exe` | Windows x86_64 |

### 2. 启动服务端

```bash
./mywebdisk-server/dist/darwin-arm64/mywebdisk-server \
  --path=/home/tem \
  --token=webroc_token \
  --passwd=123456 \
  --permission=123456abcd
```

| 参数 | 说明 |
|------|------|
| `--path` | 工作区根目录（必填） |
| `--token` | webrpc token（必填） |
| `--passwd` | webrpc 密码（必填） |
| `--permission` | webrpc 口令（必填） |

### 3. 编译桌面客户端

```bash
cd mywebdisk-desktop
./build.sh
```

- macOS：`dist/macos/MyWebDisk.app`
- Windows：`dist/windows/MyWebDisk.exe`（需 mingw-w64 或在 Windows 本机构建）

### 4. 运行桌面客户端

```bash
# macOS 双击或命令行
open mywebdisk-desktop/dist/macos/MyWebDisk.app

# 或从仓库根目录（带日志）
./test.sh
```

日志输出：`/tmp/mywebdisk-desktop-run.log`

### 5. 连接远程网盘

1. 启动桌面客户端，输入 **token** 与 **passwd** 登录
2. 点击「连接我的网盘」，填写**目标 token**（服务端 token）与**口令**
3. 连接成功后，在资源管理器中浏览、上传、下载、新建文件/文件夹

---

## 开发调试

### 桌面端

```bash
cd mywebdisk-desktop
./dev.sh && cargo run          # 终端调试
./build.sh && ./run.sh         # 仅 GUI，无终端
```

### 环境安装（macOS）

```bash
./scripts/setup-env.sh
```

安装 Godot 4.6、交叉编译工具（zig / mingw-w64）等。

### webrpc SDK

位于 `sdk/`，详见 [sdk/README.md](sdk/README.md)。

主要 C API：

- `WebrpcClient_New` / `WebrpcClient_LoginStatus` / `WebrpcClient_GetReceivePort`
- `WebrpcClient_OpenSession` / `WebrpcClient_CloseSession`
- `WebrpcClient_SendData` / `WebrpcClient_SendFile`
- `WebrpcClient_Free`

---

## 通信协议概要

回调 TCP 帧格式：**4 字节 sessionId（大端）+ 1 字节类型**

| 类型 | 含义 |
|------|------|
| `1` | 文件流（SendFile 多桶数据） |
| `2` | JSON 数据流（控制指令与响应） |

数据流 JSON 请求示例：

```json
{"id":"req-001","op":"list","path":""}
{"id":"req-002","op":"upload","path":"docs/readme.txt"}
{"id":"req-003","op":"upload_done","size":12345}
{"id":"req-004","op":"write","path":"notes.txt","content":"hello"}
```

上传 / 下载大文件流程：

1. 客户端发送 `upload` 或 `download` 准备指令
2. 通过 `SendFile` 按约 10MB/桶传输文件数据
3. 上传完成后客户端发送 `upload_done`（含总字节数）；下载完成后服务端回复 `fileSent: true`

---

## 各子项目文档

- [mywebdisk-server/README.md](mywebdisk-server/README.md) — 服务端编译、运行、协议
- [mywebdisk-desktop/README.md](mywebdisk-desktop/README.md) — 桌面端编译打包说明
- [sdk/README.md](sdk/README.md) — webrpc SDK 引用方式

---

## 许可证

请参阅各子项目及 SDK 提供方的许可条款。
