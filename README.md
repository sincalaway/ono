# ono

> ono 3.1.0 · 本地模型工作台

ono 是一个可独立编译的 **Rust + Tauri 2** 桌面应用，将本地大模型推理、模型管理、对话、工作区 Agent、Skill 扩展、图像/视频生成和第三方 API 集成放在同一个界面中。

应用运行时不需要 Node.js 或单独的前端服务器。源码和安装包不包含模型权重，文本推理使用 llama.cpp，图像与视频使用 stable-diffusion.cpp 的 `sd-server`。

## 功能一览

| 模块 | 主要能力 |
|---|---|
| 概览与监测 | CPU、内存、磁盘、显卡和引擎状态展示；模型容量估算；实时资源监测 |
| 模型管理 | GGUF 导入、分卷模型、mmproj 视觉模型、模型索引、Hugging Face 搜索/下载/暂停/续传、SHA-256 校验 |
| 本地运行 | llama.cpp 引擎安装或导入、模型加载与停止、健康检查、日志、崩溃提示、自定义启动参数 |
| 对话 | 本地模型与第三方 API 切换、流式输出、Markdown/代码高亮、图片和文档附件、对话分类/标签/收藏/导出 |
| 工作区 Agent | 计划、文件读写与搜索、命令/代码执行、网页检索、附件解析、本地知识库关键词检索、长期记忆、复盘、ZIP 交付、停止与继续 |
| Skill | 导入 `SKILL.md`、`.skill` 或 ZIP，查看、启停、删除；启用的 Skill 会注入 Agent |
| 图像与视频 | 文生图、参考图/改图、画布、视频首帧/参考图、LoRA、作品历史、取消与保存 |
| 对话工具与电脑操作 | 时间、计算器、网页、文件、命令工具；截屏、鼠标、键盘和滚动操作，步骤执行前确认 |
| 第三方 API | OpenAI 兼容的 `/chat/completions` 与 `/models` 接口、SSE 流式响应、请求取消、模型列表获取 |
| 设置与便携 | 主题、语言、端口、局域网访问、GPU/性能参数、自启动、Windows 便携模式 |

## 核心优势

### 1. 本地优先，隐私边界清晰

本地模型推理在本机完成，不强制登录，也不依赖付费账号或激活服务。设置、API Key、Hugging Face 令牌、对话和 Agent 记忆均保存在本地；默认推理服务只监听 `127.0.0.1`。

### 2. 一站式模型工作流

从硬件检查、引擎安装、模型下载、容量规划到启动、对话、画图、视频和 Agent 任务，无需在多个工具之间反复切换。本地模型与第三方 API 可以在对话和 Agent 页面直接切换。

### 3. 面向真实硬件做容量规划

应用会检测 CPU、内存、磁盘和显卡，并估算模型在显存、内存与磁盘之间的分配。运行页支持上下文长度、KV 缓存、Flash Attention、GPU 设备和自定义参数，便于在速度与资源占用之间取舍。

### 4. 权限、确认与文件边界

工作区 Agent 提供三档权限：

- **仅限查看**：只读文件、搜索和联网，不修改文件、不执行命令。
- **仅限工作区**：默认档位，可读写工作文件夹；命令、删除和工作区外操作需要确认。
- **完全权限**：可操作工作区外文件并执行命令，仅适合明确信任的任务。

后端会解析符号链接和 `..` 路径，Skill 与 ZIP 打包也有路径、条目数和大小限制。重要配置使用临时文件原子写入。

### 5. 可扩展的 Skill 与 API 体系

Skill 使用标准 `SKILL.md` frontmatter，可附带 `references/`、`scripts/` 和 `assets/`。导入时不会自动执行包内脚本；Agent 只能读取用户启用的 Skill，且 Skill 指南不能绕过当前权限。

第三方 API 采用 OpenAI 兼容协议，除内置预设外也支持自定义服务地址。

### 6. 任务可恢复，执行过程可见

工作区 Agent 会保存任务、工具步骤和改动文件。任务停止、出错或达到步数上限后，可发送“继续”恢复；停止任务会取消模型请求，并尝试结束该任务启动的 shell/代码子进程。

### 7. 轻量前端与可维护后端

前端是原生 JavaScript，无框架、无打包器；Rust 后端按引擎、模型、媒体、Agent、Skill 和状态模块划分。Tauri IPC 统一通过动态分发和 `spawn_blocking` 处理，运行时无需前端工具链。

### 8. 便携与多语言

Windows 便携模式可将数据放在程序同级的 `onoData` 目录。界面支持简体中文、繁体中文、英语、日语和韩语；用户对话、提示词、代码、日志和模型回复不会被界面翻译覆盖。

## 快速开始

### 方式一：使用 Windows 安装包

1. 运行 `*-setup.exe`。
2. 目标系统需要 Microsoft Edge WebView2 Runtime。
3. 安装后从“模型”页导入 GGUF，或从推荐目录下载模型。
4. 在“运行”页启动模型，再进入“对话”。

如果使用解压版，请保持 `ono.exe` 与 `WebView2Loader.dll` 在同一目录。程序同级存在 `portable.txt`、`portable` 目录或 `onoData` 目录时，会优先启用便携模式。

### 方式二：从源码运行

安装稳定版 Rust 和对应平台的 Tauri 2 编译依赖，然后在仓库根目录执行：

```sh
cargo run --locked --release
```

第一次构建需要下载 Cargo 依赖。运行后的界面和本地后端由同一个可执行程序提供。

### 运行第一个本地模型

1. 打开“概览”，确认 CPU、内存、磁盘和显卡信息。
2. 在“设置”中安装或导入 llama.cpp 引擎。已有 `llama-server` 时，应用也会从 `PATH` 查找；还可用 `ONO_LLAMA_SERVER` 指定程序。
3. 在“模型 → 已安装”导入 GGUF，或通过 Hugging Face 搜索并下载。导入本地文件只登记路径，删除登记不会删除源文件。
4. 在“运行”页选择模型并启动。后端等待 `/health` 成功，前端再通过 `/v1/chat/completions` 对话。
5. 如需局域网访问，可在设置中单独开启。服务暴露到局域网前，请确认所在网络可信。

### 使用第三方 API

在“设置 → 第三方 API”中添加服务商或自定义 OpenAI 兼容接口，填写接口地址、Key 和模型。对话与 Agent 顶部都可以切换本地模型或 API。

API 配置保存在本地 JSON 文件中，**当前没有加密存储**。不要将含密钥的数据目录提交到仓库或分享给他人。

### 使用图像与视频功能

首次使用时，在界面中安装或导入 stable-diffusion.cpp `sd-server` 及对应模型。应用支持文生图、参考图、改图、画布、视频首帧/参考图、LoRA 和作品历史。

模型权重体积较大，需预留足够磁盘和显存。画图或视频引擎运行时可能会与本地对话模型争用显存。

### 使用工作区 Agent

1. 在 Agent 页选择工作文件夹和权限档位。
2. 启动本地模型，或切换到已配置的第三方 API。
3. 描述任务，例如检查代码、整理文档、批量修改文件或生成交付压缩包。
4. Agent 会展示计划、工具调用和文件改动；命令、代码、删除等敏感操作按权限设置确认。
5. 任务停止或暂停后，发送“继续”即可从保存的任务历史恢复。

Agent 支持的关键能力包括：

- `update_plan`、`ask_user`：计划与关键决策澄清。
- `list_files`、`read_file`、`write_file`、`edit_file`、`make_dir`、`delete_path`、`search_files`：工作区文件操作。
- `read_file`：文本、PDF、Word、PPT、Excel 等内容解析。
- `run_command`、`run_code`：终端命令与 Python/JavaScript/PowerShell/Bash 代码执行。
- `web_search`、`web_fetch`、`knowledge_base_query`：联网检索、网页正文和本地文档关键词检索。
- `memory_search`、`memory_add`、`memory_update`：跨会话本地长期记忆。
- `reflect`、`package_files`：任务复盘与 ZIP 交付。
- `read_skill`：读取已启用 Skill 的指南和资源文本。

### 导入 Skill

Skill 目录中必须包含一个 `SKILL.md`，例如：

```markdown
---
name: document-review
description: 检查文档结构、术语一致性和遗漏，适用于项目 README、技术方案和用户手册。
---

# 文档审查

先确认文档目标读者，再检查结构、术语、链接和示例。
```

在 Agent 工作区顶部点击“Skills”，导入独立的 `SKILL.md`、`.skill` 或 ZIP。导入后默认启用，可随时停用或删除。

## 技术架构

```text
原生 JavaScript 前端
        │
        ▼
Tauri IPC（动态命令分发 + spawn_blocking）
        │
        ├── Rust 状态与配置
        ├── 模型管理与 llama-server
        ├── 媒体任务与 sd-server
        ├── Agent 工具、记忆、Skill 与第三方 API
        └── 硬件探测、桌面控制与系统集成
```

### 目录结构

| 路径 | 内容 |
|---|---|
| `frontend/` | 原生 JavaScript 界面、样式、多语言和模型目录资源 |
| `src/lib.rs` | Tauri 初始化、动态 IPC 分发、应用生命周期 |
| `src/state.rs` | 数据目录、便携模式、配置读取与原子写入 |
| `src/engine.rs` | llama.cpp 引擎安装、启动参数、进程与健康检查 |
| `src/models.rs`、`src/models/` | GGUF 元数据、分卷、mmproj、模型索引、Hugging Face 下载 |
| `src/media.rs`、`src/media/` | 图像/视频目录、引擎、任务、LoRA 与作品历史 |
| `src/agent.rs`、`src/agent/` | 工作区工具、附件、桌面控制、联网、记忆、第三方 API 流 |
| `src/skills.rs` | Skill 导入、校验、启停、删除与读取 |
| `src/hardware.rs` | 跨平台硬件探测与资源监测 |
| `tests/desktop.rs`、`tests/desktop.js` | Tauri 桌面集成测试 |
| `.github/workflows/release.yml` | Windows NSIS 安装包构建与 Release 发布 |

## 构建与测试

### 平台依赖

- **Windows**：Rust、MSVC C++ Build Tools、Windows SDK、WebView2；NSIS 构建还需要 Node.js 与 Tauri CLI。
- **macOS**：Rust、Xcode Command Line Tools。
- **Linux**：Rust、WebKitGTK 4.1、GTK 3 和桌面输入相关开发库。

不同平台的引擎包和硬件后端需要分别验证。自动安装覆盖常见 x64 构建，其他环境可手动导入匹配的引擎目录。

### 常用命令

```sh
# 编译检查
cargo check --locked --tests

# 单元测试
cargo test --locked --lib

# 桌面集成测试（需要图形会话）
cargo test --locked --test desktop

# 前端语法检查
node --check frontend/app.js
node --check frontend/agentpage.js
node --check frontend/i18n-dict.js

# Windows NSIS 安装包
npx --yes @tauri-apps/cli@^2 build --bundles nsis
```

NSIS 产物默认位于 `target/release/bundle/nsis/`。Node.js 只用于构建阶段的 Tauri CLI，不是应用运行时依赖。

桌面测试使用临时数据目录。要同时验证真实本地聊天，可设置 `ONO_TEST_GGUF` 为有效的文本模型绝对路径，并准备 `llama-server`。

### GitHub Actions 发布

`.github/workflows/release.yml` 在推送 `v*` 标签或手动触发时，于 `windows-latest` 使用稳定版 Rust 和 Tauri CLI 构建 NSIS 安装包，并创建对应 Release。普通分支推送不会自动发布安装包。

## 数据与安全

### 数据目录

应用标识为 `local.ono.reconstruction`。数据目录优先级如下：

1. `ONO_DATA_DIR` 环境变量。
2. Windows 便携模式下的程序同级 `onoData` 目录。
3. 平台应用数据目录。

主要本地数据包括：

- `settings.json`：应用设置。
- `apis.json`：第三方 API 配置与 Key。
- `model_index.json`：本地模型登记。
- `agent-memory.json`：Agent 长期记忆。
- `skills.json`：Skill 启停状态。
- `images.json`：图像/视频作品历史。
- `engines/`、`models/`、`skills/`、`loras/`、`uploads/`：引擎与用户资源。

### 安全边界

- 本地推理默认仅监听回环地址；局域网访问需要显式开启。
- Hugging Face 令牌只附加到官方 HTTPS 源请求。
- Skill 压缩包拒绝越界路径、重复项和符号链接，限制压缩包、单文件、总解压大小和条目数。
- Skill 导入不会执行脚本；Agent 读取 Skill 时仍受当前权限和用户确认约束。
- Agent 将文件、网页和命令输出视为数据，不把工具返回内容当作新的系统指令。
- `run_command` 和 `run_code` **不是沙箱**，代码直接在用户电脑上执行。工作区权限和确认机制用于降低误操作风险，不能替代系统级隔离。
- API Key 和令牌当前以明文 JSON 保存在本地，请保护好数据目录和设备。

## 验证状态与已知限制

### 已完成或已验证

- 使用 `stories260K` 模型完成导入、加载、聊天接口返回生成 token 和停止引擎的流程。
- 使用本地测试镜像完成模型搜索、下载、解析、暂停续传和移除验证。
- 图像/视频的提交、取消、输出保存和多参考图顺序已用本地协议测试服务验证。
- 第三方 API 的 SSE 流式分片、取消和模型列表接口已用本地服务验证。
- Agent 的记忆、澄清、复盘、代码执行确认、知识库检索、ZIP 打包、停止与继续，以及 Skill 导入/读取流程已通过模拟 IPC 的浏览器测试。
- Rust 测试目标编译检查、Rustfmt、前端 JavaScript 语法检查和多语言词典对齐检查已通过。

### 仍需目标环境验证

- 尚未使用完整扩散模型权重验证实际图像/视频生成效果。
- 真实第三方服务商密钥、屏幕录制/输入授权和多显卡推理尚未完成实机验证。
- Windows 图形会话中的完整桌面流程仍需在目标机器上验证。
- 当前 Windows GNU 环境下，单元测试程序可以编译，但测试 EXE 启动会出现 `STATUS_ENTRYPOINT_NOT_FOUND`；这与正常应用构建不同，该环境下的 `cargo test --lib` 尚未跑通。
- Agent 的本地知识库目前是关键词检索，不是向量化语义检索。
- 任务继续基于已保存的任务历史，不是完整的虚拟机或环境快照。

## 项目说明

ono 是基于可获取的前端资源、模型目录和原生接口证据完成的桌面重建实现，不是原安装包源码的完整恢复。项目保留本地模型、模型管理、图像/视频、Agent、第三方 API、便携模式和引擎安装等功能，不包含付费账号、激活、在线更新或官方加速服务。

仓库当前未包含 `LICENSE` 文件，许可证状态待项目所有者确认。

## 相关项目

- [Tauri 2](https://v2.tauri.app/)
- [llama.cpp](https://github.com/ggml-org/llama.cpp)
- [stable-diffusion.cpp](https://github.com/leejet/stable-diffusion.cpp)
- [ono GitHub 仓库](https://github.com/sincalaway/ono)
