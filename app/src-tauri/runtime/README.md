# Bundled llama.cpp runtime

The desktop app ships a `llama-server` for each platform so users do not need
to install llama.cpp themselves.

| Target | Source | Runtime files |
|---|---|---|
| macOS Apple Silicon | `llama-b12018-bin-macos-arm64.tar.gz` | `llama-server` + required dylibs |
| Windows x64 | `llama-b12018-bin-win-cpu-x64.zip` | `llama-server.exe` + CPU DLLs |

Pinned release: [llama.cpp b12018](https://github.com/ggml-org/llama.cpp/releases/tag/b12018)
(version `0.8.0-dev`). `LICENSE` is the upstream MIT license; Windows also includes
`LICENSE-LLVM-OpenMP`.

The macOS dylibs use `@rpath` with `LC_RPATH=@loader_path`, so the app spawns
`llama-server` with its working directory set to this folder.

Reproduce with `scripts/fetch-runtime.py` (requires macOS `otool` for trimming
the macOS dylib closure). The weights are **not** here; they are downloaded once
by the app into its data directory.
