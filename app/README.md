# ocr4code desktop

`ocr4code` 的桌面外壳：**快捷键框选截图 → 本地内核识别 → 文本直接进剪贴板**。
Tauri 2 + React 19 + TypeScript，直接复用仓库根目录的 Rust crate，不复制管线逻辑。

**用户安装后不需要装 llama.cpp，也不需要手动下载文件。**
安装包自带推理运行时；首次启动只需点一次「下载并配置」，应用会自动下载模型、
校验 SHA-256、启动引擎。之后完全离线。

> Windows 预览版安装包：
> <https://github.com/hfwang0318/ocr4code/releases/tag/v0.1.10>
> 由 GitHub Actions 的 `windows-latest` 构建 NSIS 安装器；版本记录见
> [../CHANGELOG.md](../CHANGELOG.md)。

| 首启向导 | 模型下载 |
|---|---|
| ![首次设置](docs/setup-idle.webp) | ![模型下载](docs/setup-progress.webp) |

| 识别结果 | 下载代理 / 镜像 |
|---|---|
| ![识别结果](docs/capture-result.webp) | ![下载设置](docs/settings-download.webp) |

实际 macOS 窗口（无边框玻璃材质）：
[识图](docs/capture-window.webp) · [首启向导（干净机器）](docs/setup-native.webp) · [已有 tesseract 时的首启横幅](docs/setup-banner-native.webp)

---

## 1. 依赖：哪些打包，哪些下载

| 依赖 | 体积 | 处理方式 | 说明 |
|---|---|---|---|
| 应用本体（Tauri / React） | ~15–80 MB | **打包** | 随安装包 |
| `llama-server` 运行时 + 动态库 | macOS ~29 MB / Windows ~49 MB | **打包** | 固定 `llama.cpp b12018`（MIT），见 [runtime/README.md](src-tauri/runtime/README.md) |
| 截图能力 | 0 | **应用内实现** | macOS `screencapture`；Windows 自绘浮层 + GDI 抓屏，预览全分辨率 |
| WebView2（Windows） | — | **安装器自动处理** | `embedBootstrapper`，无需用户手动装 |
| VC++ 运行库（Windows） | ~1.5 MB | **打包** | VS 2022 x64 CRT 随运行时放在 `llama-server.exe` 旁，避免旧 `msvcp140` 导致 `0xc0000005` |
| GLM-OCR Q8_0 权重 + mmproj | **1.43 GB** | **应用内一键下载** | 950,433,408 + 484,403,648 bytes，MIT；不支持塞进安装包 |
| tesseract（可选兜底） | ~50–100 MB | **不打包、不强制** | 只检测系统已安装；默认不需要，模型路径是主力 |

模型不随安装包分发的原因：1.43 GB 会让安装包膨胀、更新成本高；而且权重与运行时可以独立升级。
如果面向内网/离线分发，后续可以额外做一个「离线模型包」，把两个 `.gguf` 放进 U 盘或内部镜像即可。

### 首次启动的一键流程

```text
打开应用
  └─ 首启向导：llama.cpp 运行时 ✓（已内置）
                 GLM-OCR 模型 ✗（1.43 GB）
        │
        └─ 点击「下载并配置」
              ├─ 下载两个文件（断点续传）
              ├─ SHA-256 校验
              ├─ 启动内置 llama-server（自动选端口、健康检查）
              └─ 就绪 → 直接按快捷键截图
```

下载过程有百分比、速度、ETA、当前文件；可取消，取消后保留 `.part`，下次继续。
不需要手动解压、放路径、启动服务或配置环境变量。

### 模型在哪里

| 平台 | 路径 |
|---|---|
| macOS | `~/Library/Application Support/com.ocr4code.desktop/models/glm-ocr/` |
| Windows | `%APPDATA%\com.ocr4code.desktop\models\glm-ocr\` |

`verified.json` 记录校验通过的模型 revision；`llama-server.pid` 用于清理异常退出留下的服务进程。

---

## 2. 能做什么

| 能力 | 说明 |
|---|---|
| 一键模型安装 | 首启向导下载 1.43 GB 模型、校验、启动服务；断点续传，可取消/重试/移除 |
| 全局快捷键 | 默认 `⌘⇧2` / `Ctrl+Shift+2`；支持单键、2 键或 3 键组合（必须有 1 个主键），设置里可录制 |
| 原生截图 | macOS `screencapture -i`；Windows 自绘选区浮层 + GDI 抓屏（不调用系统截屏工具） |
| 自动识别 | 复用 `ocr4code::pipeline`：引擎 → 切块/重试 → 几何层 → 修复 → 渲染 |
| 自动复制 | 成功后直接写剪贴板；窗口隐藏时不抢焦点，托盘提示「copied N lines」 |
| 引擎管理 | 内置运行时、模型状态、服务状态、启动/停止、移除/重新下载，全部在设置里 |
| 设置面板 | 引擎、外部 server URL、prompt、token 预算、并发、tesseract 参数、版面开关、输出格式、历史条数 |
| 界面缩放 | 默认 115%，可选 100/115/130%；窗口默认 1120×740 |
| 历史 | 最近 N 条结果留在本机 `localStorage`，可复制/删除/清空 |
| 深色玻璃 | 无边框窗口 + macOS `NSVisualEffectView` / Windows Acrylic + CSS 半透明层 |
| 托盘常驻 | 关闭窗口只隐藏；托盘可截图、打开窗口、退出 |
| 更新 | 启动时自动检查 GitHub Releases；签名校验；下载后重启安装 |

刻意没做：开机自启、多图批量、云同步、账号、跨屏框选。

### 自动更新

应用通过 Tauri Updater 检查 GitHub Releases 的 `latest.json`：

- 默认启动 4 秒后检查；可在 **设置 → 更新** 关闭
- 发现新版本后默认自动下载；安装前仍需点击「重启并安装」
- 安装前会自动停止内置 `llama-server` 并等待文件句柄释放；NSIS 安装器还会在 preinstall 阶段兜底结束残留的 `llama-server.exe`
- 更新包使用 Tauri/minisign 公钥校验；私钥只存在于 GitHub Actions Secrets
- 更新端点：`https://github.com/hfwang0318/ocr4code/releases/latest/download/latest.json`
- 首次支持更新的版本是 v0.1.8，更早版本需要手动安装一次
- `latest.json` 由 `tauri-action` 生成；同时上传 `.exe`、`.sig` 和 `latest.json`

---

## 3. 架构

```text
快捷键 / 托盘 / 按钮
        │
        ▼
capture.rs ──► 区域选择（macOS 系统 / Windows 自绘） ──► 临时 PNG
        │
        ▼
engine.rs ──► 运行时/模型检查 ──► 启动/复用 llama-server ──► 健康检查
        │
        ▼
pipeline::Config ──► pipeline::recognize（根 crate，CLI 同款）
        │
        ▼
render(text/markdown/json) ──► 剪贴板
        │
        └──► Tauri events ──► React（阶段、结果、下载进度）
```

| 层 | 文件 | 职责 |
|---|---|---|
| Web UI | `src/App.tsx`、`src/views/`、`src/components/` | 窗口 chrome、Capture / History / Settings、首启向导、快捷键录制、toast |
| 桥接 | `src/lib/bridge.ts` | Tauri `invoke`/`listen` 的类型化封装；浏览器预览自动切 mock |
| 引擎管理 | `src-tauri/src/engine.rs` | 模型 manifest、断点续传、SHA-256、llama-server 生命周期、端口/健康检查、下载事件 |
| 截图 | `src-tauri/src/capture.rs`、`src-tauri/src/capture/win.rs`、`src/overlay/` | macOS 原生选区；Windows GDI 抓屏 + Tauri 浮层选区；OCR 流程、剪贴板、托盘提示 |
| 更新 | `src/lib/updater.ts`、`tauri-plugin-updater` | 检查/下载/安装、进度、签名校验、重启；安装前 stop 引擎 |
| 设置 | `src-tauri/src/settings.rs` | `settings.json` 读写、校验、`Settings → pipeline::Config` |
| Tauri | `src-tauri/src/lib.rs` | 命令、事件、托盘、窗口材质、autostart、SIGTERM/SIGINT 清理 |
| 内核 | `../src/pipeline.rs` | 与 CLI 共用的可复用管线 |

### 事件

| 事件 | 载荷 | 用途 |
|---|---|---|
| `ocr:phase` | `selecting` / `starting` / `recognizing` / `idle` | 忙碌态、引导文案 |
| `ocr:result` | `OcrResult` | 结果卡片、历史、toast |
| `ocr:error` | `{ message, hint, code? }` | 错误卡片；`setup-required` 自动打开向导 |
| `engine:download` | `DownloadProgress` | 百分比、速度、ETA、阶段、错误 |
| `engine:state` | `ServerInfo` | 服务 stopped/starting/online/failed |

### 引擎行为

- `auto`：优先外部 `llama_url` → 内置 server → tesseract 兜底。
- 显式 `llamacpp`：只用 llama.cpp；模型没装会提示 setup。
- 显式 `tesseract`：完全绕过模型下载。
- 启动时若模型已安装，后台自动拉起内置 server；加载约 7–10 秒。
- 外部已有 `llama-server` 时直接复用，不下载模型也能用（状态里显示 external）。
- 退出/收到 SIGTERM/SIGINT 时停止子进程；异常退出留下的进程由下次启动的 pid 文件清理。

---

## 4. 内核参数 → 设置面板

根 crate 的 `pipeline::Config` 是唯一事实来源；桌面层只负责持久化和展示。

| UI | 内核参数 | 默认 | 备注 |
|---|---|---|---|
| Recognition engine | `engine` | `auto` | 见上面的引擎行为 |
| External llama.cpp URL | `llama_url` | `http://127.0.0.1:8099` | 可选；内置 server 用独立端口 |
| Prompt | `prompt` | 基线默认 prompt | 对版面影响大于其它参数 |
| Max output tokens | `max_tokens` | `3072` | 同时决定切块行数上限 |
| Parallel engine calls | `jobs` | `3` | 只影响切块并发 |
| Tesseract language | `tess_lang` | `eng` | 如 `eng+chi_sim` |
| Page segmentation | `psm` | `6` | 3/4/6/7/11/13 |
| Tile tall screenshots | `tile` | 开 | 超预算按行带切块 + 一次重试 |
| Rebuild indentation | `!no_geometry` | 开 | 纯 CV 几何层 |
| Drop line-number gutters | `!keep_gutter` | 开 | 开关取反 |
| Join soft-wrapped lines | `join_wrap` | 关 | 启发式，误合并会毁行结构 |
| Format | `format` | `text` | text / markdown / json |
| Fence language | `language` | 空=猜 | 仅 markdown |
| Auto-copy | —（应用层） | 开 | 成功后写剪贴板 |
| History limit | —（应用层） | 20 | 只影响 `localStorage` |

`mock_text` / `mock_text_file` 保留给 CLI 测试，不暴露在 UI。

设置保存时会重新注册全局快捷键：失败会恢复旧值并把错误显示在 Settings 底栏；
启动时注册失败则标题栏显示「Shortcut unavailable」。

---

## 5. 开发 / 构建

要求：Rust stable、Node 20+、pnpm；macOS 需要 Xcode CLT，Windows 需要 C++ build tools。

```bash
cd app
pnpm install
pnpm tauri dev          # Vite + Tauri 窗口
pnpm build              # TS 类型检查 + 前端构建
pnpm tauri build        # 发布包；需要 Tauri updater 签名密钥
pnpm tauri build --no-bundle  # 本地只构建可执行文件，不需要签名
```

发布带更新签名的安装包时，需要设置 `TAURI_SIGNING_PRIVATE_KEY` 和
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`（CI 已配置为 GitHub Secrets）。当前 `tauri.conf.json`
启用了 `createUpdaterArtifacts`，未设置密钥的 `pnpm tauri build` 会在打包阶段报错。

运行时二进制不提交到仓库（约 78 MB），构建前先获取：

```bash
cd app
python3 scripts/fetch-runtime.py --platform macos-arm64     # macOS
python  scripts/fetch-runtime.py --platform windows-x64     # Windows
pwsh -NoProfile -File scripts/stage-vc-runtime.ps1          # Windows：内置 VC++ CRT
```

Windows 发布流程见 [`.github/workflows/release.yml`](../.github/workflows/release.yml)：
tag push 后在 `windows-latest` 上拉取 runtime、staging VC++ CRT、跑后端测试、构建 NSIS，
并由 `tauri-action` 上传安装包、`.sig` 和 `latest.json`。

### 更新签名 / 发布

- 公钥在 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`；
- 私钥由维护者生成并只保存在 GitHub Actions Secrets：
  `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`；
- 更新端点：`https://github.com/hfwang0318/ocr4code/releases/latest/download/latest.json`；
- 发布前确认 release 附件包含 `*-setup.exe`、`*-setup.exe.sig`、`latest.json`；
- v0.1.8 是首个带更新器的版本，旧版需要手动安装一次；
- v0.1.10 起安装前先停止 `llama-server`，NSIS `PREINSTALL` hook 再兜底结束残留进程。

### 运行时资源

`src-tauri/runtime/macos-arm64/` 与 `runtime/windows-x64/` 是随包分发的 llama.cpp 运行时。
平台配置 `tauri.macos.conf.json` / `tauri.windows.conf.json` 把它们映射到安装包的
`Resources/runtime/llama/`。macOS 必须保留 `@loader_path` 依赖的 dylib 平铺目录。

重新获取（固定 b12018）：

```bash
cd app
python3 scripts/fetch-runtime.py --platform macos-arm64
python3 scripts/fetch-runtime.py --platform windows-x64
```

### 浏览器预览

```bash
cd app && pnpm dev
# http://127.0.0.1:1420/?demo=result
#                       ?demo=setup | empty | history | settings | error
```

`src/lib/bridge.ts` 在没有 `__TAURI_INTERNALS__` 时切到 mock：模拟下载进度、结果、引擎状态；
设置写 `localStorage`。界面截图就是这样生成的。

### 后端自测

```bash
# 用真实 Hugging Face 下载模型到应用数据目录（约 1.43 GB），并打印进度
cd app/src-tauri
cargo run --example engine_probe -- download
# 启动内置 server，健康检查后退出
cargo run --example engine_probe -- start
```

### 下载代理与镜像

在 **设置 → 引擎 → 模型下载** 中可以直接填写：

- **下载代理**：HTTP 代理，例如 `http://user:pass@127.0.0.1:7890`
  - 密码里的 `@` 写成 `%40`，程序会先解码再认证
- **模型镜像**：填包含 `resolve` 路径的基址，不提供固定预设
  - 下载器会自动拼上文件名；浏览器要打开「基址/GLM-OCR-Q8_0.gguf」才能验证，只打开基址会 404
  - 镜像需同时提供两个 `.gguf`，且 SHA-256 与内置 manifest 一致

保存后对下一次下载生效。本地健康检查（`127.0.0.1`）不走代理。

TLS 证书使用系统信任库；如果代理/安全软件会劫持 HTTPS，需要把它的根证书安装到系统信任库，否则会出现 `unknown issuer`。

留空时仍兼容环境变量：`HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY`，以及
`OCR4CODE_MODEL_BASE`（同样要包含 resolve 路径，例如
`https://mirror.example/ggml-org/GLM-OCR-GGUF/resolve/65a42de...`）。Finder 启动的 App
可能不继承 shell 环境变量，此时用界面字段最方便。

镜像上的文件名必须一致，SHA-256 仍按内置 manifest 校验。也可以直接把两个 `.gguf`
放进模型目录做离线预置。

---

## 6. 平台细节

| | macOS | Windows |
|---|---|---|
| 选区 | `/usr/sbin/screencapture -i -x -o -t png` | 自绘 Tauri 浮层 + GDI `BitBlt` |
| 取消 | Esc；再按一次全局快捷键 kill 子进程 | Esc；再按一次全局快捷键关闭浮层 |
| 权限 | 首次需要 **Screen Recording**，在系统设置 → 隐私与安全勾选后重启 | 无系统截屏权限；GDI 直接抓当前显示器 |
| 运行时 | bundled macos-arm64（Metal） | bundled win-cpu-x64（CPU，`-ngl 0`） |
| 材质 | `NSVisualEffectMaterial::HudWindow` | `apply_acrylic` |
| 无边框窗口 | 自绘红黄绿 + `data-tauri-drag-region` | 自绘最小化/最大化/关闭 |
| WebView2 | 系统自带 | 安装器 `embedBootstrapper` 自动处理 |

Windows 自绘浮层和 GDI 抓屏已随 v0.1.5 发布，并在实际 Windows 机器上验证过安装、模型启动和更新；
一次只抓鼠标所在显示器，跨屏拖选、多显示器混合 DPI、HDR/受保护内容仍需更多覆盖。Windows 运行时是 CPU 版；
要 GPU 加速可以后续增加 Vulkan/CUDA 运行时包，或让高级用户接外部 llama-server。
当前 macOS 产物是 arm64；Intel Mac 需要单独构建 x64 并补充 `runtime/macos-x64`。

---

## 7. 数据与文件

| 内容 | 位置 |
|---|---|
| 设置 | macOS `~/Library/Application Support/com.ocr4code.desktop/settings.json`；Windows `%APPDATA%\com.ocr4code.desktop\settings.json` |
| 模型 | 同级 `models/glm-ocr/`；`*.part` 为可续传临时文件 |
| 服务日志 | macOS `~/Library/Logs/com.ocr4code.desktop/llama-server.log`；Windows `%LOCALAPPDATA%\com.ocr4code.desktop\logs\llama-server.log` |
| 历史 | WebView `localStorage`（`ocr4code.history.v1`） |
| 临时截图 | cache 目录，识别完成/取消后删除 |
| 设置损坏 | 改名为 `settings.json.bak`，回退默认值 |

历史只存文本和元数据，不存原图；缩略图只存在于当前结果卡片。

---

## 8. 验证状态（2026-10-31）

```bash
cargo test                       # 根 crate: 34 unit + 5 integration
cd app/src-tauri && cargo test   # 桌面层: 12 tests（manifest/校验/断点续传/设置/快捷键形状）
cd app && pnpm build             # tsc + vite，通过
cd app && pnpm tauri build --no-bundle
```

Windows CI（`.github/workflows/release.yml`）在 `windows-latest` 上完成：

- 拉取固定 llama.cpp b12018 Windows CPU runtime；
- 从 VS 2022 Redist 复制 x64 CRT 到 `llama-server.exe` 同目录，并跑 `--version` 冒烟；
- 运行 12 个桌面后端测试；
- 构建 NSIS、生成 `.sig` 和 `latest.json`，上传 GitHub Release。

已在 macOS 真机验证：

- 安装包内 `Resources/runtime/llama/llama-server` 可执行，资源路径解析正确；
- 用内置 manager 从 Hugging Face 实际下载 1.43 GB 模型，SHA-256 校验通过；
- 内置 `llama-server` 加载真实 GLM-OCR 模型，`/health` 返回 200，启动约 7–10 秒；
- 应用 autostart 后侧栏显示「llama.cpp 已就绪 · 内置本地引擎」；
- 通过同一 server 跑通 CLI OCR；
- SIGTERM 会停止子进程；异常退出留下的 pid 文件会在下次启动时清理；
- 模型下载代理/镜像、`%40` 代理密码和系统 TLS 可工作。

Windows 实际验证：

- v0.1.6 起安装包内置 VC++ runtime，解决旧 `msvcp140.dll` 14.16 导致的 `0xc0000005`；
- v0.1.8 起自动更新可用；实际更新流程已跑通；
- v0.1.10 修复更新时 `llama-server` 占用运行时目录的问题
  （应用先 stop + NSIS preinstall hook 兜底 kill）；
- 快捷键单键/2 键/3 键录制已通过浏览器交互测试；单键在系统全局快捷键上的实际注册仍需更多机器覆盖。

未完全覆盖：Windows 自绘选区在多显示器混合 DPI/HDR/受保护内容上的表现、外部 llama-server 占用运行时目录的场景、代码签名/公证后的嵌套 dylib 签名；自动更新的签名链已通过 `latest.json`/`.sig` 校验，但安装器仍未做 Authenticode 签名。

---

## 9. 目录

```text
app/
├── overlay.html                 Windows 选区浮层入口
├── src/
│   ├── overlay/                 Windows 自绘选区 UI
│   ├── components/              SetupDialog, Controls, WindowChrome, Sidebar, Toast
│   ├── views/                   CaptureView, HistoryView, SettingsView
│   ├── lib/                     bridge, updater, types, defaults, platform, history, status, format
│   └── styles/                  tokens.css, shell.css, views.css, overlay.css
├── src-tauri/
│   ├── src/lib.rs               Tauri builder / commands / tray / window material / signals
│   ├── src/engine.rs            运行时、模型下载/校验、llama-server 生命周期
│   ├── src/capture.rs            region selection → pipeline → clipboard
│   ├── src/capture/win.rs       Windows GDI 抓屏 + 浮层会话
│   ├── src/settings.rs           Settings ↔ pipeline::Config
│   ├── windows/nsis-hooks.nsh   更新/卸载前结束残留 llama-server
│   ├── runtime/                  bundled llama.cpp（macos-arm64 / windows-x64）
│   ├── examples/engine_probe.rs 开发用 download/start/status 探针
│   ├── capabilities/             窗口权限
│   ├── tauri.conf.json
│   ├── tauri.macos.conf.json     resources: runtime
│   └── tauri.windows.conf.json   resources + NSIS hook + WebView2 bootstrapper
├── scripts/fetch-runtime.py     重新获取固定版本运行时
├── scripts/stage-vc-runtime.ps1 Windows VC++ CRT 内置
├── THIRD_PARTY.md               运行时与模型许可
├── docs/                        界面截图
└── app-icon.png                 源图标
```
