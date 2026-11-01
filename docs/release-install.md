## sofer — Windows 与 macOS 正式版

简体中文界面，OCR 和布局还原在本地完成。

### 安装

**Windows**：在 **Assets** 下载 `*-setup.exe`，运行安装包（未签名，SmartScreen 可能提示；点 **更多信息 → 仍要运行**）。

**macOS（Apple Silicon）**：下载 `*.dmg`，打开后把 sofer 拖进「应用程序」。构建暂未做 Apple 公证，首次打开请右键 → **打开**；如果仍被拦截，可在终端执行 `xattr -dr com.apple.quarantine /Applications/sofer.app`。

首次启动点击 **下载并配置**。应用会下载 1.43 GB 的 GLM-OCR 模型，校验 SHA-256，并自动启动内置 `llama-server`。macOS 首次截图需要授予 **屏幕录制** 权限。

### 截图

Windows 端使用应用自绘的全屏选区浮层和 GDI 抓屏，不调用 Snipping Tool 或 `ms-screenclip:`；macOS 端使用系统选区。预览使用全分辨率截图，不再压缩到 2400px。

### 界面

默认窗口 1120×740，界面缩放 115%；可在 **设置 → 界面** 调整为 100%、115% 或 130%。

### 更新

从 **设置 → 更新** 检查并安装新版本；安装包使用 Tauri Updater 签名校验。Windows 安装前会先停止内置 llama-server，NSIS 安装器还会兜底结束残留的 llama-server.exe，避免运行时文件被占用。

### 快捷键

支持单键、2 键或 3 键组合（必须有且只有一个主键）；例如 `F8`、`Ctrl+Q`、`Ctrl+Shift+2`。

### 环境要求

- Windows 10/11 x64，或 Apple Silicon macOS 11+
- 模型约需 2 GB 磁盘空间
- 首次下载模型需要联网（Hugging Face）
- Windows 的 WebView2 由安装器自动处理

运行时已内置（CPU 构建，`-ngl 0`）；Windows 安装包还内置 Visual C++ 运行库，无需安装 llama.cpp 或 VC++ Redistributable。如果 Hugging Face 无法访问，可在 **设置 → 引擎 → 模型下载** 中填写下载代理或模型镜像；环境变量 `HTTPS_PROXY` / `SOFER_MODEL_BASE` 仍然兼容。代理支持 `user:pass@host:port`，密码中的 `@` 可写成 `%40`。TLS 使用系统证书信任库；企业代理/安全软件的根证书需要安装到系统信任库。

设置 → 引擎 → 本地引擎可查看运行时、模型和服务状态，并支持停止、移除和重新下载。日志在应用日志目录；反馈问题时请附上错误文本。
