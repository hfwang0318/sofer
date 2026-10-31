# Third-party notices

## Bundled llama.cpp runtime

`app/src-tauri/runtime/` ships a pinned [llama.cpp](https://github.com/ggml-org/llama.cpp)
release so users do not install a runtime.

- Version: `b12018` (`0.8.0-dev`)
- macOS arm64: `llama-b12018-bin-macos-arm64.tar.gz`
- Windows x64: `llama-b12018-bin-win-cpu-x64.zip` (CPU build)
- License: MIT — see `runtime/<platform>/LICENSE`
- Windows also includes `LICENSE-LLVM-OpenMP` for `libomp.dll`.

## GLM-OCR model (downloaded, not bundled)

The 1.43 GB weights are downloaded once by the app. They are not part of the
installer.

- Base model: [zai-org/GLM-OCR](https://huggingface.co/zai-org/GLM-OCR) — MIT
- GGUF: [ggml-org/GLM-OCR-GGUF](https://huggingface.co/ggml-org/GLM-OCR-GGUF) — MIT
- Pinned revision: `65a42de1148dbed2297e922b5dbc7d9b70c36578`
- Files:
  - `GLM-OCR-Q8_0.gguf` — 950,433,408 bytes
  - `mmproj-GLM-OCR-Q8_0.gguf` — 484,403,648 bytes
- Verified in-app with SHA-256 before the server starts.

## App dependencies

The webview uses Tauri 2, React 19, Vite and TypeScript. Their licenses are in
the respective packages under `node_modules`; the Rust dependency licenses are
available via `cargo metadata`.
