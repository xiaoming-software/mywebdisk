# webrpc SDK（全平台共用）

本目录供 **mywebdisk** 仓库内各子项目共用（`mywebdisk-server`、`mywebdisk-app`、`mywebdisk-desktop` 等）。

## 内容

| 文件 | 平台 |
|------|------|
| `libwebrpc-Mac.a` / `.h` | macOS |
| `libwebrpc-Linux.a` / `.h` | Linux amd64 |
| `libwebrpc-Linux-arm64.a` / `.h` | Linux arm64 |
| `libwebrpc-Windows.a` / `.h` | Windows |

构建 `mywebdisk-server` 时会在本目录生成运行时动态库：

- macOS: `libwebrpc.dylib`
- Linux: `libwebrpc.so`

## 在其他项目中引用

- **C/CGO 单程序**（与官方示例相同）：`#include "libwebrpc-Mac.h"`，`-L/path/to/sdk -lwebrpc-Mac`
- **另一个 Go 主程序**：`.a` 为 Go `c-archive`，不能直接 `-lwebrpc-Mac` 链进第二个 Go 可执行文件；请参考 `mywebdisk-server` 的 embed 构建方式，或向 SDK 提供方索取 Go 源码模块

## C API 摘要

- `WebrpcClient_New(token, passwd, permission)`
- `WebrpcClient_LoginStatus` / `WebrpcClient_GetReceivePort`
- `WebrpcClient_SendData` / `WebrpcClient_SendFile`
- `WebrpcClient_Free`
