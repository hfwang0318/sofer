# Changelog

All notable changes to the desktop app. The root OCR kernel keeps its own
`cargo test`/baseline history; this file records the shipped desktop releases.

## Unreleased

- The completion toast clears the window and webview background layers
  explicitly and no longer gives the pill a drop shadow, which the window
  clipped into a straight-edged translucent rectangle. Only the pill is painted.
- Capture no longer falls back to tesseract silently: once `ensure_ready`
  succeeds the config is pinned to the engine that answered, so a slow-but-up
  server is not health-checked a second time. If the kernel's trace says the
  reply may have been truncated, the result keeps a one-line notice and a
  **重新识别（分块）** action, and the screenshot is retained until the next
  capture so the retry does not need a new selection. The engine is also woken
  with one token while the user is still dragging the selection.
- Recognition streams: `ocr:partial` carries the text accumulated so far and
  the capture page shows a read-only preview until the final result replaces it.
  The kernel gains `--stream`; tiled images and non-text formats fall back to
  one shot. On macOS, **设置 → 通用 → 快速预览** shows an Apple Vision result
  first and refines it with the local model; the preview is never copied and
  never enters history.
- New input paths: drop an image onto the window, or click **从剪贴板识别**.
  Both reuse the same readiness, busy/cancel, error classification, clipboard
  and result-event path as the shortcut; unsupported formats and oversized
  files get a specific next step.
- History moved to `<app-data>/history.json` with atomic writes, corruption
  quarantine, a 50-entry cap (pinned entries evicted last) and a one-time
  migration from the old localStorage key. The history page adds search,
  pinning, Markdown/JSON export, thumbnails and an empty state.
- OCR prompt presets: 代码截图 / 终端日志 / 表格 / 公式 / 自定义, with the old
  free-form prompt migrated to 自定义 and a 2000-character limit. The default
  preset is byte-identical to the previous default prompt.
- The CLI accepts `-` for stdin, directories (with `--recursive`), and
  `--region X,Y,W,H`; it also gains `--box-art` (reconstruct `tree`/table lines
  from pixels, off by default), `--image-format png|jpeg[:quality]`, and
  `--engine vision` on macOS.
- Kernel image path: the image is decoded once and handed to engines in memory;
  tiles are encoded from pixels instead of being written to a temp file and
  read back. This removes the per-tile disk round trip while keeping the
  default whole-image request byte-identical.
- Remote mode now repairs its SSH tunnel: a background watchdog probes the
  forwarded port, reconnects with jittered exponential backoff (five attempts,
  at most 30 s apart) when the ssh child exits or health checks keep failing,
  and reports 连接中 / 已连接 / 重连中 / 失败 in Settings. When every attempt is
  exhausted the error says what happened and offers **重连**; the old result is
  never discarded.
- Added optional remote deployment: put the OCR model on a Linux server and use it
  from the desktop over an SSH tunnel. Settings → OCR → 部署到服务器 probes the
  host, installs the llama.cpp runtime when missing, downloads the model on the
  server or streams the local copy, verifies SHA-256, starts `llama-server` on
  loopback and forwards it with `ssh -L`. Remote-only mode surfaces a deploy CTA
  instead of the local setup dialog, and the About page privacy copy follows the
  selected mode. When the verified model is already on the server the action
  becomes **连接** instead of **一键部署**, so it no longer looks like a
  re-download is required. Server-side downloads use four ranged connections in
  parallel and fall back to a single connection when Range is unsupported.
- Added a completion toast for background captures: when OCR finishes while the
  main window is not in front and the text reached the clipboard, an app-drawn
  window at the bottom centre of the monitor under the cursor shows
  `已复制 · N 行` and fades out after about 2.5 s. It never takes keyboard focus,
  ignores the mouse and consecutive captures restart the same window instead of
  stacking. Turn it off under 通用 → 识别完成提示; the stored setting key stays
  `notifyOnResult`. This replaces the system notification, which depended on
  notification permissions and could be silenced or collapsed by the OS. The
  tray tooltip no longer repeats the same confirmation.
- Added a high/low precision model option: high uses the official Q8_0 pack,
  low uses the verified `mradermacher/GLM-OCR-GGUF` Q4_K_M pack. Both share the
  same mmproj; switching precision stops the old server and guides the user
  through downloading the selected pack when it is missing.
- Replaced the free-form model mirror URL with a **使用镜像** switch
  (`hf-mirror.com`); the old `modelBase`/`downloadProxy` settings are migrated
  automatically.
- Split download proxy settings into address, username and password. The
  password field is masked, and credentials are percent-encoded
  (`@` → `%40`) before being appended to the proxy URL.
- Added `docs/memory-footprint.md` with measured KV/quantization/memory data.
- Default to low-precision Q4_K_M unless the settings file explicitly says
  `"modelQuality":"high"`. Servers now default to a 5-minute idle sleep, `-cram 0`,
  q8 KV on macOS (f16 on Windows CPU), and derive `-c`/`-np` and
  `--max-tokens` context from the jobs/output settings.
- Reworked first-run and recovery flows: the setup dialog is skippable, keeps
  downloading in the background, reports cancellation and full errors, and
  distinguishes model, permission and engine failures with a direct next step.
- Added macOS Screen Recording onboarding (`CGPreflightScreenCaptureAccess` /
  `CGRequestScreenCaptureAccess`) with a dedicated permission dialog, a settings
  row, automatic re-check on app focus, and a screenshot-time guard.
- Captures that are almost entirely one colour now fail with an actionable
  `permission`/`blank` error instead of silently producing empty text.
- Settings is now five sections — 通用 / OCR / 输出 / 高级 / 关于 — with engine
  status, downloads and permission in context, and technical knobs tucked into
  the 高级 fold and disclosure rows. The save bar reports unsaved changes.
- Capture and history pages now share one readiness state (checking, setup,
  permission, starting, busy, error, ready), show the first-capture tip only
  once, confirm history clearing, and wrap long error details.
- The region overlay explains right-click/Esc cancel and warns when a selection
  is too small. Browser preview scaling no longer overflows the viewport.
- Model downloads now use up to 8 parallel Range connections for large files,
  with per-chunk resume, aggregate progress/ETA, cancel support, and a
  single-stream fallback when the server ignores Range.

- Fixed two history defects: `relativeTime` rendered 59.5 minutes as
  「60 分钟前」, and malformed stored entries could throw a `RangeError`; entries
  are now validated field by field before they reach the page. The result panel
  also keeps its copy badge and button in sync when the automatic copy already
  succeeded.
- CI now enforces `cargo fmt --check` and `cargo clippy --all-targets -- -D
  warnings` for both crates, checks the three version files agree, and runs the
  Playwright browser-preview suite on macOS. The frontend gains a vitest harness
  (81 unit tests) and an eight-case E2E suite.

## v0.1.10 — 2026-10-31

- Stop the managed `llama-server` and wait for Windows to release file handles
  before starting the updater (`prepare_update`).
- Add an NSIS `PREINSTALL`/`PREUNINSTALL` hook that terminates a leftover
  `llama-server.exe`; this also lets v0.1.9 update to v0.1.10 even though v0.1.9
  did not stop the engine itself.
- Fixes the "update fails because runtime files are in use" error.

## v0.1.9 — 2026-10-31

- Global shortcuts now accept one, two or three keys: a bare main key,
  modifier + main key, or two modifiers + main key.
- Removed the mandatory modifier requirement; added F1–F24, punctuation,
  editing keys and numpad main keys.
- macOS `Command` and `Control` are distinguished; Windows `Super` is shown as
  `Win`. More than three keys and pure modifier chords are rejected with clear
  messages.

## v0.1.8 — 2026-10-31

- Added Tauri Updater support: startup check (4 s), manual check, automatic
  download, signed update packages and restart-to-install.
- Added `latest.json` generation on GitHub Releases with NSIS as the update
  artifact; update signatures use a minisign key whose private half is stored
  only in GitHub Actions Secrets.
- Added install confirmation instead of silently restarting the app.

## v0.1.7 — 2026-10-31

- Default window grew from 920×620 to 1120×740.
- Added **Settings → 界面 → 界面缩放** (100% / 115% / 130%, default 115%) using
  the Tauri webview zoom.
- Windows selection preview now uses a full-resolution PNG (capped at 5120 px
  wide) instead of a 2400 px JPEG; overlay hint/size fonts were increased.

## v0.1.6 — 2026-10-31

- Bundled the Visual C++ 2015–2022 x64 runtime (`msvcp140.dll`,
  `vcruntime140.dll`, `vcruntime140_1.dll`, …) next to `llama-server.exe`.
- Fixes `0xc0000005` on machines with the old VS 2017 14.16 CRT.
- CI stages the CRT, runs `llama-server.exe --version` as a smoke test, and
  bundles it into the NSIS installer.

## v0.1.5 — 2026-10-31

- Replaced the Windows Snipping Tool path with an in-app implementation:
  GDI monitor capture plus a custom always-on-top Tauri selection overlay.
- No `ms-screenclip:`, no clipboard polling, no system screenshot UI.
- Selection is cropped from the original pixels; Esc, right-click or a second
  shortcut press cancels. One monitor (under the cursor) at a time.

## v0.1.4 — 2026-10-31

- Model downloads now use native TLS (Windows SChannel / macOS Secure
  Transport), so system-trusted proxy or mirror CAs work like `curl`.
- Fixes `invalid peer certificate: unknown issuer` behind TLS-intercepting
  proxies.

## v0.1.3 — 2026-10-31

- Decode percent-encoded proxy credentials before handing them to `ureq`
  (`%40` in a password now becomes `@` for Basic proxy auth).
- Removed the misleading `hf-mirror.com` preset; that host currently redirects
  to Hugging Face instead of serving an independent mirror.
- Added guidance that a mirror base must be tested with a full `.gguf` URL.

## v0.1.2 — 2026-10-31

- Added model-download proxy and mirror fields to Settings.
- Added a proxy-source fallback (`HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`) and
  `OCR4CODE_MODEL_BASE` environment fallback.
- Note: this release's `%40` proxy handling and mirror preset were fixed in
  v0.1.3.

## v0.1.1 — 2026-10-31

- Localized the entire desktop UI, setup wizard, settings, history, errors,
  tray menu and release notes to Simplified Chinese.
- Added Chinese typography/font fallbacks and refreshed screenshots.

## v0.1.0 — 2026-10-31

- First public Windows preview release built on GitHub Actions.
- Bundled llama.cpp b12018 runtime; one-click 1.43 GB GLM-OCR model download
  with resumable transfer and SHA-256 verification.
- Tauri 2 + React 19 desktop shell with global shortcut capture, local OCR,
  clipboard output, history and a dark glass UI.

---

Current release: <https://github.com/hfwang0318/ocr4code/releases/latest>
