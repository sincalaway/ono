# ono 3.1.0 桌面重建版

这是一个可独立编译的 Rust / Tauri 2 项目。Cargo 会把 frontend 目录打入应用。运行界面和本地后端无需 Node.js 或单独的前端服务器。

## 构建和启动

安装稳定版 Rust 和对应平台的 Tauri 2 编译依赖。macOS 使用 Xcode Command Line Tools，Windows 原生编译使用 MSVC C++ Build Tools、Windows SDK 和 WebView2。Linux 需要 WebKitGTK 4.1、GTK 3 和桌面输入相关开发库。本次实际构建平台为 Apple Silicon macOS，并从该平台交叉编译了 Windows x64 可执行文件。[Tauri 平台准备说明](https://v2.tauri.app/start/prerequisites/)

在包含 Cargo.toml 的目录执行以下命令。第一次构建需要下载 Cargo 依赖。

```sh
cargo run --locked --release
```

Windows 发布包需解压整个目录，保持 ono.exe 和 WebView2Loader.dll 在一起。目标系统需要 Microsoft Edge WebView2 Runtime。macOS 发布包供 Apple Silicon 使用，内含本地临时签名，没有开发者公证。

## 运行第一个本地模型

1. 打开概览页。应用会检测 CPU、内存、磁盘和可用的显卡信息。
2. 如果已有 llama-server，应用会从 PATH 查找。在设置里也可以导入解压后的 llama.cpp 引擎目录。环境变量 ONO_LLAMA_SERVER 支持指定可执行文件。应用内安装功能从 llama.cpp 官方发行包读取 SHA-256 并核对下载。
3. 在“模型 → 已安装”选择 GGUF 文件，或通过 Hugging Face 搜索并下载。导入本地文件会登记路径。删除这一登记会保留用户的源文件。
4. 在“运行”选择模型并启动。后端启动子进程，等待 /health 成功，前端随后通过 /v1/chat/completions 对话。默认仅监听 127.0.0.1。设置里可单独开启局域网访问。[llama.cpp 服务接口](https://github.com/ggml-org/llama.cpp/tree/master/tools/server)

源码包和桌面包均不含推理权重。本地文本推理使用 llama.cpp，画图和视频使用带对应任务接口的 stable-diffusion.cpp sd-server。第三方对话接口需在界面中自行配置。首次访问图像生成功能时，可在界面安装或导入引擎和对应模型。[stable-diffusion.cpp](https://github.com/leejet/stable-diffusion.cpp)

## 恢复依据与代码位置

| 路径 | 内容 |
|---|---|
| frontend/ | 提取的前端资源。app.js 保留含空格的启动参数，并连接重建的本地后端。 |
| assets/catalog.json、src/media/catalog.json | 从主程序恢复的 16 条文本模型记录、12 组图像和视频模型及 51 条量化记录。目录是 3.1.0 快照。 |
| assets/hardware-probe.ps1、src/hardware.rs | Windows 硬件探测脚本，以及重写的内存、GPU 和进程监测。 |
| src/models/、src/models.rs | GGUF 元数据、分卷、文件登记、mmproj、推荐、Hugging Face 下载与续传。 |
| src/engine.rs | 引擎安装、命令行参数、进程生命周期、健康检查及崩溃事件。 |
| src/media/、src/media.rs | 图像和视频任务、模型及 LoRA 导入、画布文件、取消和作品历史。 |
| src/agent/、src/agent.rs | 工作区工具、附件、桌面控制、网络检索、第三方 API 流及取消。 |
| src/lib.rs、src/state.rs | 前端 IPC 分发、应用生命周期、设置持久化。 |

前端 start_model、server_status、server-log 等调用和事件定义了前后端接口。样本的 0x1405703b0 引用了 start_model，src/server.rs 的位置记录、/health 和 llama-server 参数也能在原生样本中定位。重建版用这些证据连接前端和 Rust 实现。原始 Rust 类型、线程实现、显存估算算法和完整构建工程没有随安装包发布。项目中的资源规划和恢复策略是可读的重新实现。

## 功能边界

- 本地聊天、模型管理和模型文件下载可独立运行。可在设置中配置 Hugging Face 镜像。HF 访问令牌仅附加到官方 HTTPS 源的请求。
- Windows 便携模式优先使用可写的 onoData 目录（存在 portable.txt、portable 或 onoData 文件夹时启用）。
- 图像和视频的提交、取消、输出保存和多参考图顺序已用本地协议测试服务验证。尚未用完整扩散模型权重验证实际生成效果。
- 第三方 API 的 SSE 和取消使用本地服务验证。真实服务商密钥、屏幕录制与输入授权、实际多显卡推理均未经实机验证。停止 Agent 时会终止模型推理，并尝试结束当前任务启动的 shell/代码子进程。
- 设置和 API 配置保存在平台应用数据目录，应用标识为 local.ono.reconstruction。设置中的 API 密钥和令牌保存在本地 JSON 文件。可通过 ONO_DATA_DIR 指定独立的数据目录。

## 验证

```sh
cargo test --locked --lib
cargo test --locked --test desktop
```

第二条命令需要图形会话。它启动真正的 Tauri 窗口，让内嵌前端通过 IPC 读取硬件、目录和设置，并切换页面。要同时验证真实本地聊天，请将 ONO_TEST_GGUF 设置为有效的文本模型绝对路径，并准备 llama-server。桌面测试使用临时数据目录。

本机使用 stories260K（1,185,376 字节）完成了导入、加载、聊天接口返回生成 token 和停止引擎的流程。另通过测试镜像进行了联网搜索、下载、解析及移除。Windows 发布程序已完成交叉编译并核对 PE 架构和 WebView2 加载器，尚未在 Windows 桌面会话中运行。
