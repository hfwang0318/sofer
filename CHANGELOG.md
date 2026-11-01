# Changelog

All notable changes to the desktop app. The root OCR kernel keeps its own
`cargo test`/baseline history; this file records the shipped desktop releases.

## Unreleased

- Added optional remote deployment: put the OCR model on a Linux server and use it
  from the desktop over an SSH tunnel. Settings → 识别 → 部署到服务器 probes the
  host, installs the llama.cpp runtime when missing, downloads the model on the
  server or streams the local copy, verifies SHA-256, starts `llama-server` on
  loopback and forwards it with `ssh -L`. Remote-only mode surfaces a deploy CTA
  instead of the local setup dialog, and the About page privacy copy follows the
  selected mode.
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
- Settings is now five sections — 通用 / 识别 / 输出 / 高级 / 关于 — with engine
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
