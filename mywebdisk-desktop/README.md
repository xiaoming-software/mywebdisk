# mywebdisk-desktop

Rust + **egui** 桌面客户端。

## 编译打包

### 编译报错 `E0463 can't find crate for core`

说明用了 **Homebrew 的 rust**，它没有 Windows 交叉编译标准库。请改用 rustup：

```bash
export PATH="$HOME/.cargo/bin:$PATH"
which rustc   # 应显示 ~/.cargo/bin/rustc
./build.sh
```

Windows 建议在 Windows 本机用 `scripts/build-windows.ps1` 打包；Mac 上交叉编译需额外安装 `brew install mingw-w64`。

### macOS（`.app` 双击运行，无终端窗口）

```bash
cd mywebdisk-desktop
./build.sh
open dist/macos/MyWebDisk.app
```

产物：`dist/macos/MyWebDisk.app`（应用包内仅一个可执行文件 `MyWebDisk`）

本地调试若不想占用终端，可先 `./build.sh` 再 `./run.sh`。

### Windows（生成 `.exe`，无控制台窗口）

在 Windows 上：

```powershell
cd mywebdisk-desktop
.\scripts\build-windows.ps1
```

产物：`dist\windows\MyWebDisk.exe`（单文件可执行程序）

在 macOS 上交叉编译 Windows（需 `brew install mingw-w64`）：

```bash
./build.sh   # 会同时尝试生成 dist/windows/MyWebDisk.exe
```

### 开发调试

```bash
cd mywebdisk-desktop
./dev.sh && cargo run          # 终端里调试（会有终端窗口）
./build.sh && ./run.sh         # 仅 GUI，无终端（推荐验收 UI）
```

更快的本地构建（推荐）：

```bash
cd mywebdisk-desktop
./dev.sh
cargo run
```

## 已实现

- 登录页（token / 密码、空值校验、中英文）
- 经静态链接 webrpc SDK 的登录（New + 每秒 LoginStatus，最多 10 秒）
- 登录成功进入会话占位页

## 待实现

- 会话列表、连接远程网盘、目录浏览等
