# Changelog

All notable changes to the desktop app. The root OCR kernel keeps its own
`cargo test`/baseline history; this file records the shipped desktop releases.

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
