# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Before changing UI or user-facing copy (mandatory)

Read [docs/ui-guidelines.md](docs/ui-guidelines.md) before editing anything under
`app/src/`, `app/src/overlay/`, or user-facing strings in `app/src-tauri/`
(`capture.rs`, `lib.rs`, `engine.rs`, `settings.rs`). It is the project contract,
not a suggestion. The gates that reviewers enforce:

- One readiness state (`app/src/lib/status.ts`): titlebar, sidebar and capture page
  must agree. Never invent a second status vocabulary in a component.
- Every blocking state keeps a visible exit: banner + primary page action
  (download model / grant permission). An old result must not hide the exit.
- Model setup comes before screen-recording permission.
- Terminology: OCR 引擎 / 引擎模式 / OCR 模型 / tesseract / 运行时 / 截图 / 历史.
  Do not leak `llama-server`, GGUF or stack traces into first-layer copy.
- Errors follow `[发生了什么] + [下一步]` and use the `OcrError.code` taxonomy
  (`permission` / `setup-required` / `blank` / `engine` / `capture`).
- Dialogs trap focus, refocus after branch changes, restore focus on close, and
  only allow bare F1–F24 as global shortcuts.
- No new toast for routine success; no `window.confirm`/`alert`.

If an implementation genuinely needs to break one of these rules, update the
guide in the same change and explain why. Do not silently diverge.

## Repository map

- `src/` — `ocr4code` kernel: a pure Rust library plus the `ocr4code` CLI.
  Engine-agnostic OCR pipeline; no Tauri or platform UI code.
- `app/` — Tauri 2 + React 19 desktop shell. Its crate (`app/src-tauri`,
  package `ocr4code-app`) depends on the root crate by path. **This is not a
  Cargo workspace**: root `cargo test` does not build the desktop crate, and
  the desktop crate must be tested with its own manifest.
- `app/src/` — React UI. Two Vite entries: `index.html` → `src/main.tsx`
  (main window) and `overlay.html` → `src/overlay/main.tsx` (Windows selection
  overlay).
- `app/src-tauri/` — privileged layer: commands/events/tray/shortcut/window
  (`lib.rs`), capture (`capture.rs`, `capture/win.rs`), model + llama-server
  lifecycle (`engine.rs`), persistence (`settings.rs`).
- `tests/golden.rs` — integration tests that feed ground-truth text through the
  geometry layer and compare indentation.
- `bench/` — Python scene generators, scorers and baseline runners. `bench/images/`,
  `bench/truth/` and `bench/results/` are generated; do not hand-edit.
- `docs/` — design and operating documents (index below).
- `.github/workflows/` — `ci.yml` runs root tests, the frontend typecheck/build
  and the desktop backend tests on macOS + Windows; `release.yml` builds the
  signed Windows NSIS release on `v*` tags.

## Commands

### Kernel / CLI (from repo root)

```bash
cargo test                                   # unit + golden integration tests
cargo test --test golden                     # integration tests only
cargo test --test golden gutter_is_detected_and_model_spacing_kept
cargo test repair::                          # any test name/module filter
cargo run --release -- ocr shot.png --format markdown
cargo run --release -- doctor                # engine availability
cargo build --release
```

Baseline work needs the Python venv and generated scenes; see
[docs/maintenance.md](docs/maintenance.md):

```bash
python3 bench/scenes.py
python3 bench/scenes2.py
python3 bench/scenes3.py
.venv/bin/python bench/baseline.py --engines mock,tesseract,llamacpp --variant 2x
```

### Desktop shell

```bash
cd app
pnpm install
pnpm dev                    # browser preview, mock backend, no model needed
pnpm typecheck              # tsc --noEmit (strict + noUnused*)
pnpm build                  # typecheck + Vite production build

pnpm tauri dev              # Vite + native window
pnpm tauri build --no-bundle  # local executable; no updater signing key
pnpm tauri build            # release bundle; requires TAURI_SIGNING_PRIVATE_KEY
```

The bundled llama.cpp runtime is gitignored and must be fetched before a
native build:

```bash
cd app
python3 scripts/fetch-runtime.py --platform macos-arm64   # or windows-x64
pwsh -NoProfile -File scripts/stage-vc-runtime.ps1        # Windows CRT staging
```

Desktop backend tests and probes:

```bash
cargo test --manifest-path app/src-tauri/Cargo.toml
cargo test --manifest-path app/src-tauri/Cargo.toml capture::tests
cargo test --manifest-path app/src-tauri/Cargo.toml engine::tests::sha256_matches_a_known_value
cargo check --manifest-path app/src-tauri/Cargo.toml

cd app/src-tauri
cargo run --example engine_probe -- download   # real ~1.43 GB download
cargo run --example engine_probe -- start      # start + health check
```

Remote deployment E2E (local Docker Linux SSH server; downloads ~1 GB):

```bash
app/scripts/dev-remote-server.sh up
app/scripts/test-remote.sh
```

The script covers probe, server download, reuse without re-download, tunnel +
OCR, stop, local upload, bad key and unreachable host. The manual cases
(cancel/resume, corruption recovery, non-Linux rejection, tunnel port
conflict, app restart) are listed in `app/README.md`.

Full check before shipping a change that touches both crates:

```bash
cargo test
cargo test --manifest-path app/src-tauri/Cargo.toml
cd app && pnpm build
```

No ESLint/Prettier/rustfmt/clippy configuration is checked in; `pnpm build`
(strict TypeScript) is the only frontend static gate. Do not claim a lint gate
that does not exist.

### Browser preview

`pnpm dev` starts Vite on fixed port 1420. The UI switches `lib/bridge.ts` to a
mock bus when `__TAURI_INTERNALS__` is absent:

```text
http://127.0.0.1:1420/?demo=result
                            ?demo=setup | permission | error | history | settings
```

Use it for visual/interaction checks. Real capture, model download and updater
signing must still be verified in a native build.

## Architecture

### Kernel pipeline

```text
image ─► engine (llamacpp / tesseract / mock) ─► text lines (bbox optional)
      └► geom (ink mask, row bands, x0/x1, advance) ─► tile when over budget
                                                         │
              assemble (pairing, gutter, DP alignment, soft wrap) ◄─┘
                    │
              repair (dashes, quotes, context-aware full-width)
                    │
              render (text / markdown / json)
```

Key invariant: **engines only supply text**. Indentation, column alignment and
line structure are recovered from pixels by `geom`/`assemble`. Do not move
indentation responsibility into an engine.

- `src/pipeline.rs` — the reusable pipeline and `Config`, shared by CLI and the
  desktop app. `Config` is the single source of truth for kernel knobs.
- `src/geom.rs` — polarity-independent ink mask, row projection, per-band x0/x1,
  grid-fit advance.
- `src/tile.rs` — budget-based tiling, stitching by band index, adaptive retry.
- `src/pool.rs` — ordered parallel map and the shared job semaphore.
- `src/assemble.rs` — text-line ↔ band pairing, line-number gutter, bbox column
  gaps, soft-wrap joining.
- `src/repair.rs` / `src/lang.rs` / `src/render.rs` — text repair, fence-language
  guessing, output formats.
- `src/engine/` — `llamacpp` (primary), `tesseract` (fallback), `mock` (tests).

Kernel change guardrails (details in [docs/maintenance.md](docs/maintenance.md)):

- Geometry/assembly changes require all three: `cargo test`, mock baseline
  `indent_exact ≈ 1.00`, and the real-editor screenshots.
- Tiling may only cut at blank space between text lines and must stitch by band
  index, not by text matching. `huge-4k` is the tiling regression scene.
- `--jobs` is a global semaphore budget shared by image-level and tile-level
  work; tile temp filenames must stay unique per process.
- Changing the default prompt or model requires rerunning the full baseline;
  `docs/baseline.md` and README numbers are maintained by hand from
  `bench/results/baseline-*.md`.

### Desktop request flow

```text
global shortcut / tray / Capture button
  └─ capture::start (returns immediately; busy+cancel flags)
      └─ OS region selection → temp PNG → permission/blank guards
          └─ engine::Manager::ensure_ready (external server → bundled server → tesseract)
              └─ ocr4code::pipeline::recognize (same code as CLI)
                  └─ render → clipboard → Tauri events → React
```

- `capture::start` is non-blocking and runs on a thread; a second trigger while
  capture is busy sets the cancel flag instead of stacking overlays.
- macOS uses `screencapture -i` and CoreGraphics permission preflight. Windows
  uses a self-drawn Tauri overlay plus GDI (`capture/win.rs`), so it needs no
  screen-recording permission.
- Errors are classified into `permission` / `setup-required` / `blank` /
  `engine` / `capture` in `capture.rs`, then rendered by `CaptureView` and the
  dialogs. A new code must be wired through `capture.rs`, `CaptureView`'s error
  copy, `OcrError.code` consumers and `docs/ui-guidelines.md` together.
- Blank detection is deliberately conservative: a uniform-frame check on the
  520×360 thumbnail. Do not reintroduce sparse-grid sampling; it false-positived
  on large screenshots containing one line of text.

### Desktop state and bridge

- `App.tsx` owns settings, engine status, phase, result/error, download state
  and modal visibility. It coordinates mutual exclusion: close one modal before
  opening another, and close both before a result/error event opens one.
- `lib/status.ts` is the single readiness state machine, including the
  model-before-permission order. `engineSummary` / `engineForMode` feed the
  sidebar and settings; they must not expose URLs or ports in first-layer copy.
- `lib/bridge.ts` wraps every `invoke`/`listen` and provides a mock implementation
  for browser preview. Any new command or event needs a mock branch and demo
  data, otherwise `pnpm dev` breaks.
- Tauri events: `ocr:phase`, `ocr:result`, `ocr:error`, `engine:download`,
  `engine:state`. Payload structs use serde `camelCase`; `lib/types.ts` mirrors
  Rust structs by hand. Keep both sides in sync.
- The overlay is a separate Vite entry and Tauri window; it talks to
  `overlay_info` / `overlay_finish` / `overlay_cancel` in `capture.rs`.

### Engine and settings lifecycle

- `engine.rs` deliberately has no Tauri types so it can be tested standalone.
  It owns the pinned model manifest (revision + SHA-256), resumable parallel
  download, verification, port selection, server spawn/stop and health checks.
- Runtime binaries are bundled in the installer but not committed. Development
  falls back to `app/src-tauri/runtime/<platform>/`; `fetch-runtime.py` fills it.
- `settings.rs` persists one camelCase `settings.json`, validates it, and maps it
  to `pipeline::Config`. Changing `engine`, quality, `jobs`, `max_tokens`, KV
  quantization or idle sleep changes the server command line and stops the
  managed server (`server_signature`). A quality change also cancels an in-flight
  download for the old pack.
- Defaults: low-precision Q4_K_M, 5-minute idle sleep, KV q8 on macOS / off on
  Windows CPU, 3 parallel jobs, 3072 max output tokens, UI scale 1.15.
- `pnpm tauri build` enables updater artifacts and fails without signing keys;
  use `--no-bundle` for local builds. Windows install stops `llama-server`
  first so NSIS can replace the runtime files.

### Versioning and data locations

Version must stay in sync in `app/package.json`, `app/src-tauri/Cargo.toml` and
`app/src-tauri/tauri.conf.json`; release tags are `v*` and are documented in
`CHANGELOG.md`.

| Data | Location |
|---|---|
| Settings | macOS `~/Library/Application Support/com.ocr4code.desktop/settings.json`; Windows `%APPDATA%\com.ocr4code.desktop\settings.json` |
| Models | app-data `models/glm-ocr/` (`*.part` is resumable) |
| Server log | macOS `~/Library/Logs/com.ocr4code.desktop/llama-server.log`; Windows `%LOCALAPPDATA%\com.ocr4code.desktop\logs\llama-server.log` |
| History | WebView `localStorage` key `ocr4code.history.v1` (text + metadata only) |
| Temp screenshots | OS cache dir, deleted after success/cancel |

## Documentation index

| Document | Read it when |
|---|---|
| [docs/ui-guidelines.md](docs/ui-guidelines.md) | **Mandatory before any UI, state-flow or user-facing copy change.** State machine, blocking exits, dialog focus, terminology, error formulas, AI-flavor substitutions, review checklist. |
| [README.md](README.md) | CLI usage/parameters, kernel architecture, tiling/concurrency, baseline numbers, platform deps, known limits. |
| [app/README.md](app/README.md) | Desktop dependency split, native architecture, events, settings→kernel mapping, build/release/signing, runtime resources, data paths, verification status. Some test counts/demo names there may lag behind the code; the commands above are authoritative. |
| [docs/maintenance.md](docs/maintenance.md) | Changing geometry/tiling/concurrency/prompt, metric definitions, extending scenes/engines, release checklist, troubleshooting, known debt. |
| [docs/baseline.md](docs/baseline.md) | Per-scene quality numbers and how to reproduce the baseline. |
| [docs/memory-footprint.md](docs/memory-footprint.md) | Choosing context size, KV quantization, idle release and runtime; RSS vs footprint caveats. |
| [docs/ocr4code-选型报告.md](docs/ocr4code-选型报告.md) | Why this architecture/model/engine was chosen; prior art and measured comparisons. |
| [bench/SCENES.md](bench/SCENES.md) | What the synthetic scenes cover and how to add one. |
| [bench/tools/real_capture/README.md](bench/tools/real_capture/README.md) | Collecting real-editor screenshots for geometry validation. |
| [CHANGELOG.md](CHANGELOG.md) | Desktop release history and the unreleased section; update it for user-visible behavior. |
| [app/THIRD_PARTY.md](app/THIRD_PARTY.md) | Licenses for the runtime, model packs and bundled CRT. |
| [app/src-tauri/runtime/README.md](app/src-tauri/runtime/README.md) | Bundled llama.cpp layout and the pinned version. |
