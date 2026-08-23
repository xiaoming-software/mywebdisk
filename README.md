# MyWebDisk

**无公网 IP 的个人 NAS / 私人网盘服务端** — Personal NAS server over P2P (webrpc)

人在外面也能浏览、上传、下载家里电脑或 NAS 上的文件。不需要公网 IP、不需要路由器端口映射、也不把文件上传到第三方网盘。传输走 [webrpc](https://www.webrpc.cn/controllor.html) 加密 P2P 通道；手机和电脑端用 [File2File](https://github.com/xiaoming-software/File2File-Desktop) 桌面客户端连接并操作网盘。

[English README](README.en.md)

**关键词：** 个人 NAS、私人网盘、无公网 IP、P2P 网盘、webrpc、File2File、macOS / Linux / Windows、树莓派、ARM NAS

---

## 目录

- [它解决什么问题](#它解决什么问题)
- [功能一览](#功能一览)
- [工作原理](#工作原理)
- [界面预览](#界面预览)
- [你需要准备什么](#你需要准备什么)
- [各平台部署](#各平台部署)
- [启动服务端](#启动服务端)
- [用 File2File 连接网盘](#用-file2file-连接网盘)
- [从源码编译](#从源码编译)
- [安全说明](#安全说明)
- [常见问题](#常见问题)
- [相关项目](#相关项目)

---

## 它解决什么问题

把家里的 Mac、Windows 电脑、Linux 小主机或 ARM NAS 变成一台**只属于你的网盘**：

| 常见做法 | 痛点 | MyWebDisk |
| --- | --- | --- |
| 公有云网盘 | 文件离开自己的硬盘，容量与隐私受限 | 文件始终在你指定的本地目录 |
| 公网 IP + 端口映射 / frp | 运营商没公网 IP，穿透配置复杂 | 不需要公网 IP，webrpc 负责打洞 |
| 自建 WebDAV / SMB 暴露公网 | 暴露端口、证书、防火墙 | 对端必须同时知道 Token 和口令 |

适合：出差访问家里照片和文档、跨网络取安装包、给自己搭一台私人云盘。

---

## 功能一览

服务端是一条命令即可运行的 Go 程序。绑定一个本地目录后，经 File2File 可完成：

- **浏览目录**：列表 / 图标视图，路径限制在 `--path` 内，禁止 `../` 越界
- **磁盘信息**：总容量、剩余、已用、文件数量
- **上传 / 下载**：大文件分块传输
- **新建、重命名、移动、删除** 文件和文件夹
- **搜索** 网盘内文件名
- **打包文件夹** 为 zip（带进度）

webrpc 运行库已嵌入各平台可执行文件，**发布目录里不需要再附带** `.so` / `.dylib` / `.dll`。

---

## 工作原理

1. 在 NAS / 家里电脑上启动 `mywebdisk-server`，传入该设备的 webrpc **Token、密码、口令** 和本地目录。
2. 服务端登录 webrpc，在本机回调端口上等待会话。
3. 另一台设备打开 File2File，用**另一枚** Token 登录。
4. 在「网盘连接」里填写服务端 Token 和同一条口令，建立 P2P 会话。
5. 之后的列目录、上传下载都走这条加密通道，不经过中心化存储。

两端都要能访问互联网以完成握手；文件内容优先点对点直传。

---

## 界面预览

### 1. 服务端在家里电脑上运行

传入 Token、密码、口令和 NAS 目录后，登录成功即进入待命：

![mywebdisk-server 运行日志：webrpc 登录成功并等待客户端会话](img/mywebdisk-server-run.png)

### 2. File2File 连接网盘（连接前）

客户端已登录后，在左侧「网盘连接」选择节点，弹出「请先连接网盘」，填写**网盘 Token** 和**认证口令**（与服务端 `--permission` 一致）：

![File2File 连接网盘前：输入网盘 Token 与认证口令](img/link-mywebdisk.png)

### 3. 连接成功后的网盘内容

连通后可看磁盘用量、搜索、上传，并以列表或图标浏览家里的文件：

![File2File 已连接个人 NAS，浏览家里网盘文件与磁盘用量](img/mywebdisk-client.png)

---

## 你需要准备什么

1. **两枚 webrpc Token**（一枚给服务端，一枚给 File2File）  
   在 [webrpc 控制台](https://www.webrpc.cn/controllor.html) 注册后，到「我的 Token」复制 **Token 值** 和 **密码**。
2. **口令（permission）**  
   服务端启动时设置，File2File 连接网盘时必须填同一条。不要留空。
3. **一块本地磁盘目录**  
   例如 `/Users/you/mywebdisk_space`、`D:\nas`、`/mnt/data/nas`。不存在会自动创建。
4. **客户端**  
   安装 [File2File Desktop](https://github.com/xiaoming-software/File2File-Desktop)。

---

## 各平台部署

仓库 [`dist/`](dist/) 目录已提供各平台单文件程序。下载对应文件、赋予执行权限后即可运行。

| 平台 | 架构 | 仓库内路径 | 适用机器 |
| --- | --- | --- | --- |
| macOS 11+ | Apple Silicon (arm64) | [`dist/darwin-arm64/mywebdisk-server`](dist/darwin-arm64/mywebdisk-server) | M 系列 Mac |
| Linux | x86_64 (amd64) | [`dist/linux-amd64/mywebdisk-server`](dist/linux-amd64/mywebdisk-server) | 普通 x86 NAS / 小主机 / 云主机 |
| Linux | ARM64 | [`dist/linux-arm64/mywebdisk-server`](dist/linux-arm64/mywebdisk-server) | 树莓派 64 位、ARM NAS、部分软路由 |
| Windows 10/11 | x64 | [`dist/windows-amd64/mywebdisk-server.exe`](dist/windows-amd64/mywebdisk-server.exe) | 家里的 Windows 电脑 |

单文件即可，无需再安装 webrpc SDK 或配置动态库路径。

### macOS (Apple Silicon)

```bash
chmod +x dist/darwin-arm64/mywebdisk-server
./dist/darwin-arm64/mywebdisk-server \
  --token=你的服务端Token \
  --passwd=Token密码 \
  --permission=你设的口令 \
  --path=/Users/你/mywebdisk_space
```

若系统提示「无法验证开发者」，在「系统设置 → 隐私与安全性」中允许，或执行：

```bash
xattr -dr com.apple.quarantine dist/darwin-arm64/mywebdisk-server
```

### Linux x86_64

```bash
chmod +x dist/linux-amd64/mywebdisk-server
./dist/linux-amd64/mywebdisk-server \
  --token=你的服务端Token \
  --passwd=Token密码 \
  --permission=你设的口令 \
  --path=/mnt/data/nas
```

长期挂在 NAS / 小主机上，可用 systemd（把路径和参数改成你的）：

```ini
# /etc/systemd/system/mywebdisk.service
[Unit]
Description=MyWebDisk personal NAS (webrpc P2P)
After=network-online.target

[Service]
Type=simple
ExecStart=/opt/mywebdisk/mywebdisk-server --token=TOKEN --passwd=PASSWD --permission=PIN --path=/mnt/data/nas
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now mywebdisk
```

### Linux ARM64（树莓派 / ARM NAS）

```bash
chmod +x dist/linux-arm64/mywebdisk-server
./dist/linux-arm64/mywebdisk-server \
  --token=你的服务端Token \
  --passwd=Token密码 \
  --permission=你设的口令 \
  --path=/mnt/usb/nas
```

请使用 **64 位** 系统镜像。32 位 ARM 不在当前发布范围内。

### Windows x64

在 PowerShell 或「命令提示符」中：

```bat
dist\windows-amd64\mywebdisk-server.exe --token=服务端Token --passwd=Token密码 --permission=口令 --path=D:\nas
```

关机后若要自动起来，可把上述命令做成开机启动脚本，或放到「任务计划程序」。保持窗口不关，或用 nssm 等工具注册为服务。

---

## 启动服务端

四个参数都必填：

| 参数 | 含义 |
| --- | --- |
| `--token` | 这台 NAS 设备的 webrpc Token |
| `--passwd` | 该 Token 在控制台里的密码 |
| `--permission` | 口令。File2File「认证口令」必须与此相同 |
| `--path` | 作为网盘根目录的本地路径；不存在则创建 |

成功时日志大致如下：

```text
webrpc 客户端已创建，等待登录...
webrpc 登录成功 status=1
NAS 目录: /Users/a1/mywebdisk_space
服务已就绪，等待客户端通过 webrpc 建立会话
已连接 webrpc 回调端口 127.0.0.1:xxxxx
```

看到「服务已就绪」后再去开 File2File。登录失败会在约 30 秒超时，请核对 Token、密码和网络。`Ctrl+C` 退出。

---

## 用 File2File 连接网盘

客户端仓库：[xiaoming-software/File2File-Desktop](https://github.com/xiaoming-software/File2File-Desktop)

1. 安装并启动 File2File（macOS / Windows / Linux 均可）。
2. 用**客户端自己的** webrpc Token 和密码登录。认证口令可按 File2File 说明填写；这与网盘口令不是同一件事。
3. 左侧找到 **网盘连接**，点 **新建连接** 或选已保存的节点（例如「家里网盘mac」）。
4. 在「请先连接网盘」窗口填写：
   - **网盘 Token**：服务端 `--token`
   - **认证口令**：服务端 `--permission`
5. 点 **连接**。成功后即可看磁盘用量、浏览目录、上传、搜索。

服务端必须先在线。只知道 Token 但口令不对，无法建立会话。

---

## 从源码编译

环境：Go 1.22+，仓库内 `sdk/` 已包含各平台 webrpc 动态库。

```bash
git clone https://github.com/xiaoming-software/mywebdisk.git
cd mywebdisk
./build.sh
```

产物：

```text
dist/darwin-arm64/mywebdisk-server
dist/linux-amd64/mywebdisk-server
dist/linux-arm64/mywebdisk-server
dist/windows-amd64/mywebdisk-server.exe
```

在 Apple Silicon Mac 上可一次编出上述四个目标（`CGO_ENABLED=0` 交叉编译，无需 zig）。

---

## 安全说明

- 口令用于限制谁能 `OpenSession` 连上这台 NAS，请使用足够长的随机口令，不要提交到 Git。
- 所有文件操作都被限制在 `--path` 目录内。
- webrpc 通道默认加密；平台不托管你的文件内容。
- 仅供合法个人与商业使用。禁止用于法律法规不允许的内容与活动，详见 [webrpc 合规说明](https://www.webrpc.cn/controllor.html)。
- Token、密码、口令等同于网盘钥匙，不要发给不信任的人。

---

## 常见问题

**Q：没有公网 IP 能用吗？**  
能。这是本项目的主要使用场景。两端能上网即可尝试握手。

**Q：File2File 登录用的 Token 和服务端是同一枚吗？**  
不要共用。一枚给家里的 `mywebdisk-server`，另一枚给出门用的 File2File。连接网盘时，在客户端里填**服务端那枚** Token 和口令。

**Q：登录一直超时？**  
检查 Token / 密码是否从「我的 Token」原样复制、机器能否访问外网、是否用了对应系统的二进制（不要在 Linux 上跑 macOS 文件）。

**Q：连接网盘失败？**  
确认服务端日志已是「服务已就绪」、服务端进程还在跑、口令与 `--permission` 完全一致、客户端已登录且网络可达。

**Q：支持 Intel Mac / Android / iOS 服务端吗？**  
当前发布的是 darwin-arm64、linux-amd64、linux-arm64、windows-amd64。Android / iOS 不做 NAS 服务端。Intel Mac 不在预编译列表中。

**Q：可以当多人共享网盘吗？**  
协议上多个 File2File 可以连同一台服务端，但本项目按**个人 NAS** 设计：知道 Token + 口令的人都能操作该目录，请自行控制分享范围。

---

## 相关项目

| 项目 | 作用 |
| --- | --- |
| 本仓库 **MyWebDisk** | 个人 NAS 服务端 |
| [File2File Desktop](https://github.com/xiaoming-software/File2File-Desktop) | 连接并操作网盘的桌面客户端，也支持聊天与直传文件 |
| [webrpc](https://www.webrpc.cn/controllor.html) | P2P RPC 传输与 Token 服务 |

问题与建议请在本仓库提 Issue。请自行保管 Token 与口令。
