# mywebdisk-server

个人网盘服务端，基于 **webrpc P2P**。远程客户端通过 webrpc 会话读写 `--path` 工作区内的目录与文件。

webrpc SDK 位于仓库根目录 `[../sdk](../sdk)`（与各子项目同级）。

## 关于 webrpc 静态库

`sdk/libwebrpc-*.a` 由 Go 以 `c-archive` 方式导出，**无法像普通 C 库那样再静态链接进另一个 Go 可执行文件**（会与主程序 `go.o` 产生 `__cgo_topofstack` 等符号冲突）。因此构建时从 `.a` 生成各平台运行时库，并通过 `go:embed` **打包进单个可执行文件**——`dist` 目录下**无需再附带** `libwebrpc.dylib/.so/.dll`。

运行时 API 调用方式与您提供的 CGO 示例一致（`WebrpcClient_New` / `SendData` / `SendFile` 等）。

## 编译

```bash
cd mywebdisk-server
./build.sh
```

产物（每个目录仅一个可执行文件）：


| 目录                                        | 说明                  |
| ----------------------------------------- | ------------------- |
| `dist/darwin-arm64/mywebdisk-server`      | macOS Apple Silicon |
| `dist/linux-amd64/mywebdisk-server`       | Linux x86_64        |
| `dist/linux-arm64/mywebdisk-server`       | Linux arm64         |
| `dist/windows-amd64/mywebdisk-server.exe` | Windows x86_64      |


交叉编译 Linux/Windows 时建议安装 [zig](https://ziglang.org/download/) 或对应交叉 gcc/mingw。

## 运行

```bash
./mywebdisk-server \
  --path=/home/tem \
  --token=webroc_token \
  --passwd=123456 \
  --permission=123456abcd
```


| 参数             | 说明               |
| -------------- | ---------------- |
| `--path`       | 工作区根目录（必填）       |
| `--token`      | webrpc token（必填） |
| `--passwd`     | webrpc 密码（必填）    |
| `--permission` | webrpc 口令（必填）    |


## 通信协议

回调 TCP 帧：4 字节 `sessionId`（大端）+ 1 字节类型（`1`=文件流，`2`=JSON 数据流）。

数据流 JSON 操作：`list` `stat` `mkdir` `delete` `rename` `download` `upload` `write` `health`（详见此前文档）。

`write` 用于将 UTF-8 文本内容写入已有文件（在线编辑保存），请求体含 `path` 与 `content`，单文件上限 50MB。

`path` 为相对工作区根的路径，禁止 `../` 越界。