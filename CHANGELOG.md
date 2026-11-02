# Changelog

All notable changes to the desktop app. The root OCR kernel keeps its own
`cargo test`/baseline history; this file records the shipped desktop releases.

## v1.2.0 — 2026-11-02

图片进入模型前的预处理（内核 + 桌面端）：

- EXIF 方向：拖入手机照片不再按存储方向识别（`image::load_from_memory` 不应用，
  llama.cpp 也不解析 EXIF）。
- 透明背景合成白底：剪贴板复制的带 alpha PNG 不再把透明区当作墨迹/黑底。
- 解码像素上限（默认 5000 万）：此前只限制文件字节数，一个 40 MB 的 PNG 可以
  解码成数亿像素并耗尽内存。
- 内容裁边：只裁掉面积 ≥35% 的背景边距；2–20% 的小裁边会让模型对画面尺寸敏感，
  `text_acc` 反而掉 1.1 pp，因此按 35% 设阈值。
- 1× 截图 2× 放大：26 场景 `content_acc` 0.934→0.955，`sym_recall` 0.970→0.995，
  基本追平原生 2×；代价是同批次耗时 +88%。
- 几何层背景改为 64 px 分块插值，深浅混排/渐变背景更稳。
- 桌面端复用 `inspect_capture` 的解码结果，不再对同一张 4K 图解码两次。
- 远程部署自动用 JPEG q90（SSH 隧道带宽受限）；本地仍优先转发原始字节。
- `--stats` 增加 `preprocess` 段；CLI 新增 `--no-trim`、`--no-upscale-small`、
  `--no-adaptive-background`、`--max-pixels`。
- 结果页的截图预览改为完整缩放（`object-fit: contain`），宽图和竖图不再被裁成
  左上角一块。
- 点击结果预览可在弹窗中放大查看；点击遮罩、按 `Esc` 或点关闭都会退出，焦点锁在
  弹窗内并在关闭后还给缩略图。
- 新增「通用 → 预览标注」开关（默认关闭）：在预览图上框出模型识别到的每一行文字，
  放大弹窗中同样生效。`Line` 新增 `bbox` 字段，`OcrResult` 新增 `previewBoxes`。
- `sofer-app` 新增 `SOFER_IMAGE_MAX_TOKENS` 环境变量，映射到 llama-server 的
  `--image-max-tokens`（默认不限制）。

2× 基线（26 场景）逐项不变：0.957 / 0.983 / 0.996 / 0.91 / 0.98。
完整消融见 [docs/baseline.md](docs/baseline.md) §5。

## v1.1.0 — 2026-11-02

Windows 默认更省内存：开启「压缩识别缓存」（KV q8_0），并在每次识别结束后立即释放
模型内存（下次截图重新加载）；macOS 保持空闲 5 分钟释放。设置 → OCR 的「空闲释放内存」
新增「立即释放」，两个平台都可手动选择。

内核后处理改进。26 个基准场景的质量指标与 1.0.0 完全持平（逐场景 `content_acc`
只有 tesseract `git-diff` 曾短暂回退，已修正），新增能力主要作用于病态模型输出。

- 模型把整段结果包在 Markdown 围栏里时自动剥离，围栏语言回填到输出。
- 空行由行带间距重建，不再照搬模型多吐或漏吐的空行；行带漏检时回退到模型换行。
- 行号槽场景用行带内部的第二个墨迹段定位代码列，行号从 9 变 10 不再带动代码缩进。
- 模型复读（连续 3 行以上相同、且总行数明显超出图像行数）折叠为一行。
- 行内多空格可由像素列段补回；代码场景的缩进按文档缩进单位吸附。
- diff 中单独出现的 `—` 还原为 `--`；`-—`、`——` 不会被多补。
- Markdown 输出的围栏长度会超过内容里的反引号，避免日志中的围栏提前闭合。
- `--stats` 增加 clean 段与 blanks / snapped / suspicious 计数。

基线（26 场景，2x，与上一版逐项相同）：mock `1.000 / 0.990 / 1.00`，
llamacpp `0.960 / 0.910 / 0.98`，tesseract `0.899 / 0.870 / 0.93`
（content_acc / indent_exact / rows_ratio）。

## v1.0.0 — 2026-11-01

首个正式版本，提供 Windows NSIS 安装包与 macOS（Apple Silicon）DMG。

- 截图转文字：快捷键、托盘、拖入图片、从剪贴板识别四种入口；识别结果直接进剪贴板。
- 版面还原：缩进、列对齐与行结构由几何层从像素计算，不依赖引擎输出排版。
- 本地离线识别：应用内下载并校验 GLM-OCR 模型，内置 llama.cpp 运行时，
  首次配置后无需联网；提供高精度与低精度两档。
- 流式结果与快速预览：识别过程中持续回填文本；macOS 可先用系统 Vision 出快预览。
- 历史记录：最近 50 条，支持搜索、置顶、导出 Markdown/JSON。
- 设置：引擎模式、模型精度、并发数、全局快捷键、界面缩放、下载代理与镜像。
- 可选远程部署：把模型部署到自己的 Linux 服务器，经 SSH 隧道识别。
- 自动更新：应用内检查并安装新版本，安装包经 Tauri/minisign 签名校验。

---

Current release: <https://github.com/hfwang0318/sofer/releases/latest>
