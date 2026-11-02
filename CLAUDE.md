# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Before changing UI or user-facing copy (mandatory)

Read [docs/ui-guidelines.md](docs/ui-guidelines.md) before editing anything under
`app/src/`, `app/src/overlay/`, `app/src/toast/`, or user-facing strings in
`app/src-tauri/` (`capture/`, `toast.rs`, `lib.rs`, `engine/`, `settings.rs`).
It is the project contract,
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
- No new in-app toast for routine success. The one exception is the app-drawn
  background `识别完成弹窗` (bottom-centre, only when the main window is not in
  front and the result was copied). No `window.confirm`/`alert`.

If an implementation genuinely needs to break one of these rules, update the
guide in the same change and explain why. Do not silently diverge.

## Repository map

- `src/` — `sofer` kernel: a pure Rust library plus the `sofer` CLI.
  Engine-agnostic OCR pipeline; no Tauri or platform UI code.
- `app/` — Tauri 2 + React 19 desktop shell. Its crate (`app/src-tauri`,
  package `sofer-app`) depends on the root crate by path. **This is not a
  Cargo workspace**: root `cargo test` does not build the desktop crate, and
  the desktop crate must be tested with its own manifest.
- `app/src/` — React UI. Three Vite entries: `index.html` → `src/main.tsx`
  (main window), `overlay.html` → `src/overlay/main.tsx` (Windows selection
  overlay) and `toast.html` → `src/toast/main.tsx` (background completion toast).
- `app/src-tauri/` — privileged layer: commands/events/tray/shortcut/window
  (`lib.rs`), capture (`capture/`), completion toast (`toast.rs`), model +
  llama-server lifecycle (`engine/`), SSH deployment and tunnel supervision
  (`remote/`), persistence (`settings.rs`, `history.rs`).
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
cargo test                                   # 71 unit + 9 boxdraw + 10 CLI + 6 golden
cargo fmt --all -- --check                   # formatting gate
cargo clippy --all-targets -- -D warnings    # warnings are errors
cargo test --test golden                     # integration tests only
cargo test --test golden gutter_code_column_comes_from_pixels
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
python3 bench/scenes4.py
.venv/bin/python bench/baseline.py --engines mock,tesseract,llamacpp --variant 2x
```

### Desktop shell

```bash
cd app
pnpm install
pnpm dev                    # browser preview, mock backend, no model needed
pnpm typecheck              # tsc --noEmit (strict + noUnused*)
pnpm build                  # typecheck + Vite production build
pnpm test:run               # vitest + jsdom (81 tests, pure lib logic)
pnpm e2e                    # Playwright browser-preview flows (8 cases)

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
cargo fmt --manifest-path app/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path app/src-tauri/Cargo.toml --all-targets -- -D warnings
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
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo fmt --manifest-path app/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path app/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path app/src-tauri/Cargo.toml
cd app && pnpm build && pnpm test:run && pnpm e2e
python3 scripts/check-versions.py
```

There is no ESLint/Prettier config; rustfmt, clippy (warnings as errors), strict
TypeScript, vitest and the Playwright preview suite are the static/regression
gates. `scripts/check-versions.py` keeps the three version files in sync.

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
image ─► preprocess (EXIF, alpha, pixel cap, content crop, 2x small text)
             └► model copy ─► engine (llamacpp / vision / tesseract / mock) ─► text lines
      └► geom (ink mask, row bands, x0/x1, advance, column runs) ─► tile when over budget
                                                         │
              repair (fence strip, dedupe, dashes, quotes, full-width) ◄─┘
                    │
              assemble (pairing, gutter, blank rebuild, pixel spacing, snap, soft wrap)
                    │
              render (text / markdown / json)
```

Repair runs **before** assemble on purpose: zero-width marks and full-width
twins change the text width per character while the ink stays the same, which
would bias every width comparison in `assemble`.

Key invariant: **engines only supply text**. Indentation, column alignment and
line structure are recovered from pixels by `geom`/`assemble`. Do not move
indentation responsibility into an engine.

- `src/pipeline.rs` — the reusable pipeline and `Config`, shared by CLI and the
  desktop app. `Config` is the single source of truth for kernel knobs.
- `src/preprocess.rs` — the model copy: EXIF orientation, transparent→white,
  decoded-pixel cap, content-box crop, 2× upscale for small text. Geometry keeps
  working in the normalized original's coordinates; `Transform` maps a tile onto
  the model copy. Original bytes are forwarded only when nothing changed.
- `src/geom.rs` — polarity-independent ink mask (64 px local background), row
  projection, per-band x0/x1, grid-fit advance, per-band column runs (used for
  gutter columns and pixel spacing).
- `src/tile.rs` — budget-based tiling, per-tile horizontal ink bounds, stitching
  by band index, adaptive retry.
- `src/pool.rs` — ordered parallel map and the shared job semaphore.
- `src/assemble.rs` — text-line ↔ band pairing, line-number gutter (code column
  from the second ink run), blank-line rebuild from band spacing, pixel spacing,
  indent snapping (code only), bbox column gaps, soft-wrap joining.
- `src/repair.rs` — document classification (code / diff / terminal / table /
  prose), fence stripping, repeated-line collapse, dashes, quotes, full-width
  folding. `/src/lang.rs` guesses the fence language, `/src/text.rs` defines
  terminal column width (CJK = 2, zero-width = 0), `/src/render.rs` formats.
- `src/engine/` — `llamacpp` (primary), `vision` (macOS fast tier, Swift shim
  compiled by `build.rs`), `tesseract` (fallback), `mock` (tests).
- `src/boxdraw.rs` — reconstructs tree/table rules and missing table frames
  from the ink mask. **On by default** (`--no-box-art` disables). It only
  rewrites decoration confirmed by a stroke, skips text that already carries
  Unicode box glyphs, and can insert the horizontal rule rows the engine
  dropped.
- `src/cli_input.rs` — stdin, `--region` crops and directory expansion.

Kernel change guardrails (details in [docs/maintenance.md](docs/maintenance.md)):

- Geometry/assembly changes require all three: `cargo test`, mock baseline
  `indent_exact ≈ 1.00`, and the real-editor screenshots.
- Tiling may only cut at blank space between text lines and must stitch by band
  index, not by text matching. `huge-4k` is the tiling regression scene.
- `--jobs` is a global semaphore budget shared by image-level and tile-level
  work; temp image filenames (only engines that need a file) must stay unique
  per process. Tiles are encoded in memory, not written to disk.
- Changing the default prompt or model requires rerunning the full baseline;
  `docs/baseline.md` and README numbers are maintained by hand from
  `bench/results/baseline-*.md`.

### Desktop request flow

```text
shortcut / tray / Capture button / dropped file / clipboard image
  └─ capture::start_* (returns immediately; one busy+cancel flag)
      └─ region selection (or the given file) → temp PNG → permission/blank guards
          └─ optional fast preview: Apple Vision → ocr:partial
              └─ engine::Manager::ensure_ready (external → bundled → tesseract)
                  └─ sofer::pipeline::recognize_streaming (same code as CLI)
                      └─ render → clipboard → ocr:result → React
                           └─ main window not in front → toast::show (bottom-centre window)
```

- `capture::start` is non-blocking and runs on a thread; a second trigger while
  capture is busy sets the cancel flag instead of stacking overlays. Dropped
  files and clipboard images go through the same `process_capture` path.
- After `ensure_ready`, the config is pinned to the engine that answered. Do not
  let `auto` health-check it again: a transient failure used to fall back to
  tesseract silently. A truncated result keeps its screenshot as `retry.png`
  for the chunked `retry_capture` path.
- macOS fast preview (`fastPreview`) runs one Vision pass before the model and
  emits `ocr:partial { preview: true }`; it is never copied or stored.
- macOS uses `screencapture -i` and CoreGraphics permission preflight. Windows
  uses a self-drawn Tauri overlay plus GDI (`capture/win.rs`), so it needs no
  screen-recording permission.
- Errors are classified into `permission` / `setup-required` / `blank` /
  `engine` / `capture` in `capture/flow.rs`, then rendered by `CaptureView` and the
  dialogs. A new code must be wired through `capture/`, `CaptureView`'s error
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
- Tauri events: `ocr:phase`, `ocr:partial`, `ocr:result`, `ocr:error`,
  `ocr:drag`, `ocr:drop`, `engine:download`, `engine:state`, `remote:*`,
  `toast:show`. Payload structs use serde `camelCase`; `lib/types.ts` mirrors
  Rust structs by hand. Keep both sides in sync.
- The overlay is a separate Vite entry and Tauri window; it talks to
  `overlay_info` / `overlay_finish` / `overlay_cancel` in `capture/`.
- The completion toast is another separate entry/window (`toast`, created lazily
  by `toast.rs`). It is reused across captures, never focuses or accepts clicks,
  and talks to `toast_info` / `toast_dismiss`; `notify_on_result` controls it
  even though the stored key still says "notify".

### Engine and settings lifecycle

- `engine/` deliberately has no Tauri types so it can be tested standalone.
  It owns the pinned model manifest (revision + SHA-256), resumable parallel
  download, verification, port selection, server spawn/stop and health checks.
- Runtime binaries are bundled in the installer but not committed. Development
  falls back to `app/src-tauri/runtime/<platform>/`; `fetch-runtime.py` fills it.
- `settings.rs` persists one camelCase `settings.json`, validates it, and maps it
  to `pipeline::Config`. Changing `engine`, quality, `jobs`, `max_tokens`, KV
  quantization or idle sleep changes the server command line and stops the
  managed server (`server_signature`). A quality change also cancels an in-flight
  download for the old pack.
- Defaults: low-precision Q4_K_M, KV q8 on every platform, 5-minute idle sleep
  on macOS, release right after each capture on Windows, 3 parallel jobs,
  3072 max output tokens, UI scale 1.15.
- `pnpm tauri build` enables updater artifacts and fails without signing keys;
  use `--no-bundle` for local builds. Windows install stops `llama-server`
  first so NSIS can replace the runtime files.

### Versioning and data locations

Version must stay in sync in `app/package.json`, `app/src-tauri/Cargo.toml` and
`app/src-tauri/tauri.conf.json`; release tags are `v*` and are documented in
`CHANGELOG.md`.

| Data | Location |
|---|---|
| Settings | macOS `~/Library/Application Support/com.sofer.desktop/settings.json`; Windows `%APPDATA%\com.sofer.desktop\settings.json` |
| Models | app-data `models/glm-ocr/` (`*.part` is resumable) |
| Server log | macOS `~/Library/Logs/com.sofer.desktop/llama-server.log`; Windows `%LOCALAPPDATA%\com.sofer.desktop\logs\llama-server.log` |
| History | app-data `history.json` (atomic writes; the old `sofer.history.v1` localStorage is migrated once) |
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
| [docs/sofer-选型报告.md](docs/sofer-选型报告.md) | Why this architecture/model/engine was chosen; prior art and measured comparisons. |
| [bench/SCENES.md](bench/SCENES.md) | What the synthetic scenes cover and how to add one. |
| [bench/tools/real_capture/README.md](bench/tools/real_capture/README.md) | Collecting real-editor screenshots for geometry validation. |
| [CHANGELOG.md](CHANGELOG.md) | Desktop release history and the unreleased section; update it for user-visible behavior. |
| [docs/release-install.md](docs/release-install.md) | Shared install/environment section of the GitHub release page; the per-release changes come from `CHANGELOG.md` via `scripts/release_body.py`. |
| [app/THIRD_PARTY.md](app/THIRD_PARTY.md) | Licenses for the runtime, model packs and bundled CRT. |
| [app/src-tauri/runtime/README.md](app/src-tauri/runtime/README.md) | Bundled llama.cpp layout and the pinned version. |
