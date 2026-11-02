# sofer

把**代码截图 / 终端输出 / 报错堆栈**转成能直接粘回去的文本。默认离线运行，macOS + Windows；
也可以把模型一键部署到自己的 Linux 服务器，用 SSH 隧道识别（可选，适合本机内存紧张）。

> **安装包（Windows / macOS Apple Silicon）**：[GitHub Releases](https://github.com/hfwang0318/sofer/releases/tag/v1.0.0)
> —— 首次启动在应用内下载模型（默认低精度 1.03 GB，可在设置切换高精度 1.43 GB）；安装器自带 llama.cpp 运行时，无需另装。

> 一句话架构：**文本由本地 VLM 给，版面由纯 CV 给**。缩进和列对齐从像素算，不依赖任何单平台的 OCR API，
> 所以换平台不用换引擎，换引擎不用改版面代码。

**文档索引**
- 设计取舍与选型：[docs/sofer-选型报告.md](docs/sofer-选型报告.md)
- 可复现基线（26 场景 × 4 变体）：[docs/baseline.md](docs/baseline.md)
- **常驻内存实测与优化**（KV/量化/运行时横评）：[docs/memory-footprint.md](docs/memory-footprint.md)
- **维护手册**：[docs/maintenance.md](docs/maintenance.md)
- 桌面外壳（Tauri + TS，截图到剪贴板）：[app/README.md](app/README.md)
- 桌面端版本记录：[CHANGELOG.md](CHANGELOG.md)

---

## 1. 能力

### 能做

| 能力 | 说明 |
|---|---|
| 代码截图 | 保留缩进、列对齐、行号槽（行号列会被识别并保留） |
| 终端输出 | `git diff` / `pytest` 堆栈 / `tree` / `docker ps` 表格 / 长日志 |
| 结构化输出 | `text`（直接粘）、`markdown`（带语言围栏）、`json`（给脚本/agent） |
| 中英混排 | 中文注释 + 代码 + 中文报错（VLM 逐字正确） |
| 大图 | 4K/8.6MP 自动切块 + 自适应重试（见 §4） |
| 输入 | 文件 / 目录批处理 / stdin / `--region` 裁剪 |
| 框线 | `--box-art` 从像素重建 `tree` 与表格框线（默认关闭） |
| 流式 | `--stream` 边识别边输出；桌面端始终流式预览 |
| 多引擎 | GLM-OCR（主力）、Apple Vision（macOS 快档）、Tesseract（低配/兜底）、mock（测试） |
| 跨平台 | 核心是纯 Rust + `image` + HTTP；模型走 llama.cpp（mac: Metal / win: Vulkan·CUDA·CPU） |
| 完全离线 | 默认模型、引擎、几何层都在本地；启用远程部署时截图会发到自己的服务器 |

### 不能做（刻意或未做）

| 限制 | 原因 |
|---|---|
| 多栏（tmux 分屏）的阅读顺序 | 走"按行"的几何模型；真值按栏拼接时 GLM 0.990、Tesseract 0.270 |
| Powerline / Nerd Font 图标 | 私有区字形在像素上已经是 tofu，模型没有可映射的语义；需要图标模板匹配 |
| 真连字字体（Fira Code 把 `=>` 合成单字形） | 测试语料用 Menlo，不连字 |
| Windows 多显示器拖选 / 滚动长截图 | 自绘选区一次只抓鼠标所在显示器；长图需要上层拼接 |

---

## 2. 使用

### 2.1 构建

```bash
cargo build --release        # 产物 target/release/sofer（约 4 MB）
cargo test                   # 71 单元 + 25 集成（golden / boxdraw / CLI 输入）
```

### 2.2 启动主力引擎（llama.cpp + GLM-OCR）

> **桌面应用不需要做这一节**：`app/` 已经内置 `llama-server`，首次启动会一键下载并校验
> 模型（高精度 1.43 GB / 低精度 1.03 GB），自动拉起服务。下面是 CLI / 手动部署方式。

```bash
# 首次：下载官方 GGUF（MIT，Q8_0 约 950 MB + mmproj 约 484 MB）
hf download ggml-org/GLM-OCR-GGUF --include "*Q8_0.gguf" --local-dir ~/models/GLM-OCR-GGUF

# 启动（-ngl 99 = 所有层放 GPU；缺 GPU 时自动回落 CPU）
llama-server -m ~/models/GLM-OCR-GGUF/GLM-OCR-Q8_0.gguf \
  --mmproj ~/models/GLM-OCR-GGUF/mmproj-GLM-OCR-Q8_0.gguf \
  --host 127.0.0.1 --port 8099 -ngl 99 -c 32768 -np 3
```

`sofer doctor` 会检查它是否可用，并打印上面这段命令。

> **低精度（Windows CPU 省内存）**：官方仓库没有 Q4；社区 `mradermacher/GLM-OCR-GGUF`
> 的 `GLM-OCR.Q4_K_M.gguf`（548,515,136 B，MIT）已实测与自量化精度一致（25 个 `@2x`
> 场景 24 个逐位相同，`@1x` 略好，4K 切块 56/56）：
>
> ```bash
> hf download mradermacher/GLM-OCR-GGUF --include "GLM-OCR.Q4_K_M.gguf" \
>   --local-dir ~/models/GLM-OCR-GGUF
> llama-server -m ~/models/GLM-OCR-GGUF/GLM-OCR.Q4_K_M.gguf \
>   --mmproj ~/models/GLM-OCR-GGUF/mmproj-GLM-OCR-Q8_0.gguf \
>   --host 127.0.0.1 --port 8099 -ngl 0 -c 32768 -np 3
> ```

> **省内存变体**（macOS/Metal、官方 Q8 权重，实测活动 footprint 2.6 GB → 1.7 GB，
> 空闲 5 分钟后 → 146 MB；精度 25 场景逐场景相同）：
>
> ```bash
> llama-server ... -c 32768 -np 3 \
>   -ctk q8_0 -ctv q8_0 -cram 0 --sleep-idle-seconds 300
> ```
>
> Windows 内置运行时是 CPU-only，**不要加 `-ctk/-ctv q8_0`**（实测约 2× 慢）；只加
> `-cram 0 --sleep-idle-seconds 300`。完整实测见 [docs/memory-footprint.md](docs/memory-footprint.md)。

> **上下文别给太小**：一个 4K 切块的视觉 prompt 就吃掉约 **6k token**（实测 `/slots` 的
> `n_prompt_tokens` 分别为 6395 / 5850 / 6081），再加上 `--max-tokens 3072` 的输出，
> 单个在飞请求峰值 ≈ 9k。`-c 8192` 配 `--jobs 3` 会互相抢上下文；
> 手动部署用 `-c 32768 -np 3` 比较稳（`-c` 是所有 slot 共享的 KV 池）；桌面端会按
> `jobs`/`max_tokens` 自动算成 `-c 33024 -np 3`（默认）。

### 2.3 最常用的三条

```bash
sofer ocr shot.png                     # 纯文本，保留缩进
sofer ocr shot.png --format markdown   # ```lang 围栏，直接贴 issue / LLM
sofer ocr a.png b.png --format json --out out.json
```

### 2.4 完整参数

```
sofer ocr <IMAGES>... [OPTIONS]
```

**输入**

| 参数 | 默认 | 说明 |
|---|---|---|
| `<IMAGES>...` | — | 图片路径、目录，或 `-` 读标准输入；目录会展开为 png / jpg / jpeg / webp / bmp / tiff |
| `--recursive` | 关 | 目录输入时递归子目录 |
| `--region X,Y,W,H` | 整图 | 识别前裁剪；每张输入图都应用同一个区域，越界会报出实际尺寸 |
| `-o, --out <FILE>` | stdout | 写文件；多图依次拼接 |

**引擎**

| 参数 | 默认 | 说明 |
|---|---|---|
| `--engine` | `auto` | `auto`（先 llamacpp 再 tesseract）/ `llamacpp` / `tesseract` / `vision`（仅 macOS，见 §9）/ `mock` |
| `--llama-url` | `http://127.0.0.1:8099` | 也可用环境变量 `SOFER_LLAMA_URL` |
| `--image-format` | `png` | `png` / `jpeg` / `jpeg:<1-100>`；只影响需要重新编码的切块，整图原样转发 |
| `--prompt` | `OCR: preserve the original layout, spacing and line breaks exactly.` | 换 prompt 影响大于调参（见基线 §4⑤） |
| `--max-tokens` | `3072` | 同时决定切块的行数上限 |
| `--tile` | `auto` | `auto` / `off`；超预算时按行带切块 + 失败重试 |
| `--tess-lang` | `eng` | 仅 tesseract，如 `eng+chi_sim` |
| `--psm` | `6` | 仅 tesseract |

**版面**

| 参数 | 默认 | 说明 |
|---|---|---|
| `--no-geometry` | 关 | 关掉缩进重建（调试用：看引擎原样输出） |
| `--join-wrap` | 关 | 合并终端软换行（启发式，像素上无法可靠区分，误合并会毁行结构） |
| `--keep-gutter` | 关 | 不自动识别行号槽（默认识别到就用像素定位行号后的代码列） |
| `--box-art` | 关 | 从像素重建 `├──`/`└──`/`│` 与表格框线；只替换行首装饰，行数不变 |

**输出**

| 参数 | 默认 | 说明 |
|---|---|---|
| `--format` | `text` | `text` / `markdown` / `json` |
| `--lang` | 自动猜 | markdown 围栏语言（`rust`/`python`/`diff`/`bash`/…） |
| `--stats` | 关 | 把切块/几何决策打到 stderr |
| `-j, --jobs` | `1` | 同时识别的图片数；输出顺序与输入一致，单张失败不影响其余 |
| `--stream` | 关 | 边生成边写 stdout；仅整图 + `text` 格式，切块或其它格式自动回退一次性 |

**调试**

| 参数 | 说明 |
|---|---|
| `--mock-text` / `--mock-text-file` | 给布局层喂"已知文本"，单测缩进逻辑 |
| `SOFER_DEBUG=1`（环境变量） | 打印 `typo / advance / ratios` 等几何中间量 |

### 2.5 输出格式

**`text`**（默认）

```
~/proj $ tree -L 3 --dirsfirst src
src
├── cli
```

**`markdown`**

````
```diff
~/src/api (main) $ git diff --stat
 src/auth/session.ts | 14 ++++++++------
```
````

**`json`**

```json
{
  "engine": "llamacpp(http://127.0.0.1:8099)+3tiles(output budget)",
  "source": "shot.png",
  "ms": 1846,
  "width": 996,
  "height": 443,
  "lines": [
    { "text": "app.run(host=host, port=port)", "indent": 4, "x0": 62.0 }
  ],
  "language": "python"
}
```

| 字段 | 说明 |
|---|---|
| `engine` | 实际引擎；切块时带 `+Ntiles(原因)` |
| `ms` | 引擎耗时（切块为各块之和） |
| `lines[].text` | **去掉缩进后**的行内容 |
| `lines[].indent` | 几何层算出的行首空格数 |
| `lines[].conf` | 仅带置信度的引擎有（tesseract）；VLM 没有 |
| `lines[].x0` | 该行墨迹左边界（像素），便于二次对齐 |
| `language` | 围栏语言猜测结果 |

---

## 3. 架构

```
截图 ─► 引擎（llamacpp / tesseract / mock）─► 文本行（可能没有 bbox）
     └► 几何层（纯 CV：ink mask / 行投影 / 最左墨迹 / 格点）─► 行带 + advance
                                   │
                   切块（超预算时）─┤
                                   ▼
              清洗（围栏剥离 / 复读折叠 / 破折号 / 引号 / 全角）
                                   │
              组装（配对 / 行号槽 / 空行重建 / 像素列距 / 缩进吸附 / 软换行）─► 输出（text / md / json）
```

| 模块 | 职责 |
|---|---|
| `src/geom.rs` | 极性无关 ink mask、行投影切带、每带 x0/x1、格点拟合 advance、每带列段 |
| `src/tile.rs` | 按输出预算切块、按行带序号拼接、自适应重试的判定 |
| `src/assemble.rs` | 文本行 ↔ 行带配对（空白行不占带）、宽度对齐（DP）、行号槽（用像素定位代码列）、按行距重建空行、像素列距、缩进吸附、bbox 列距、软换行 |
| `src/repair.rs` | 文档分类（代码/diff/终端/表格/散文）、围栏剥离、复读折叠、`—`→`-`（diff 中还原 `--`）、智能引号、**上下文感知**的全角折叠（代码里折、中文里不折） |
| `src/text.rs` | 终端列宽：CJK/全角记 2、零宽记 0，几何与文本宽度比较共用同一单位 |
| `src/lang.rs` | 围栏语言猜测（shebang、终端会话优先判为 shell） |
| `src/render.rs` | text / markdown / json |
| `src/pipeline.rs` | 可复用管线（配置 → 引擎 → 切块/重试 → 组装），CLI 与桌面壳共用 |
| `src/engine/` | `llamacpp`（主力）、`tesseract`（兜底）、`mock`（测试） |

**关键约束**：引擎只负责给文本。缩进、列对齐、行结构全部由几何层从像素算。

---

## 4. 切块（`--tile auto`）

大图不是"看不清"，是**回答写不下**：235 字符的日志行 ≈ 134 token（1.75 字符/token），
`--max-tokens 3072` 大约只能写 20 行，超出部分被静默截断；调到 8192 又会超发（68 行 vs 真值 56 行）。

```
tokens_per_row ≈ (band_width / advance) / 1.75
rows_per_tile  = clamp(max_tokens × 0.9 / tokens_per_row, 8, 24)
```

三条硬规则：

1. **只在两条文本行之间的空白处切**，绝不切穿一行；每块上下各留 1 行重叠；
2. **按行带序号拼接**，不按文本去重（模型会把 `INFO  worker` 写成 `INFO worker`，文本匹配会失效）；
3. 相邻重复行按"去空白后相等"折叠，处理跨切点被重复吐出的那一行。

**自适应重试**：第一遍行数 < 行带数 × 0.7 且没切过块时，强制切块重跑一次，取更接近行带数的结果（只重试一次）。

实测（`huge-4k`，3940×2145，56 行）：整图 26/56 行（0.452）→ 切 3 块 **56/56 行（1.000）**，47 s → 124 s。
小图不触发（`tree` / `git-diff` / `ultra-wide` / `tiny-crop` 都是 `tiling: off`）。

---

## 5. 并发

`--jobs N` 让多张图并行识别，**输出顺序仍与输入顺序一致**；某张失败只记到 stderr，其余照常输出（进程退出码 1）。

**`--jobs` 是全局预算**：它限制的是"同时在飞的引擎调用数"，图片之间和同一张图的切块共用这个预算，
所以不会出现 `3 张图 × 3 块 = 9 个请求` 打爆 4 个 slot 的情况。

实测（M5）：

| 场景 | jobs=1 | jobs=3 | jobs=6 |
|---|---|---|---|
| 6 张图，llamacpp | 6.14 s | **3.04 s** | 3.44 s |
| 6 张图，tesseract | 1.62 s | — | 0.60 s |
| **单张 4K（切 3 块）** | 71.6 s | **48.7 s** | — |

VLM 的甜点区在 **jobs ≈ 3**：桌面端最多开 3 个 llama slot（`n_slots = 3`），再多人只是在排队，还多占上下文。
tesseract 是纯 CPU，可以直接开到核数。4K 那张只有 1.5× 提升——因为 3 个切块的视觉编码都压在同一块 GPU 上。

并发安全性：引擎进程之间无共享状态；切块的临时文件名带 `(pid, 序号)`，同进程多线程也不会互相覆盖。
切块场景报的 `ms` 是**整段墙钟**，不是各块之和（否则会按并发倍数虚高）。

**内存**（M5 16 GB，80 ms 采样峰值，RSS 含 mmap 的模型页）：

| 场景 | llama-server | CLI | 合计 |
|---|---|---|---|
| 空闲 | 1748 MB | — | 1748 MB |
| 6 张小图，jobs=1 | 2396 MB | 8 MB | 2401 MB |
| 6 张小图，jobs=3 | 2469 MB | 13 MB | 2482 MB |
| 4K 单图（切 3 块），jobs=1 | 2658 MB | 61 MB | 2718 MB |
| 4K 单图（切 3 块），jobs=3 | 2958 MB | 85 MB | 3015 MB |
| 6 张小图，tesseract jobs=6 | 1798 MB（空闲） | 18 MB | 2101 MB（含 tesseract 288 MB） |

内存几乎全在 server 侧：**每个在飞请求约 +100~300 MB**（KV 缓存），CLI 本身只有 8–85 MB。
16 GB 机器完全够；真正的瓶颈是 GPU 带宽，不是内存。

> 2026-10-31 用同一台 M5、b12018 复测 `-c 32768 -np 3`：空闲 **RSS 3.3 GB /
> footprint 2.6 GB**，上表的 1748 MB 更接近 `-c 8192` 一档，应是旧上下文/旧运行时
> 的数据。2 GiB 的 KV 是常驻大头；`-ctk q8_0 -ctv q8_0` 可减到 1.7 GB，
> `--sleep-idle-seconds` 可把空闲压到 ~150 MB。按 `-c`/`-np` 裁剪、权重 4 bit 和
> Ollama/LM Studio 等运行时的横向对比见 [docs/memory-footprint.md](docs/memory-footprint.md)。

## 6. 测试与基线

```bash
cargo test                        # 71 单元 + 25 集成
python3 bench/scenes.py           # 20 个合成场景
python3 bench/scenes2.py          # 复杂场景（双栏、图标、嵌套 diff…）
python3 bench/scenes3.py          # 极端几何（小图 / 4K / 19:1 / 1:11）
.venv/bin/python bench/baseline.py --variant 2x    # 走 CLI 出基线表
```

集成测试的做法：把**真值文本**当引擎输出（无 bbox，正是 VLM 的形态）喂给几何层，比缩进。
`mock` 引擎因此是"几何层单独评分"的基准，它掉分就是布局层的问题。

**当前基线（26 场景，@2x，M5）**

| engine | content_acc | text_acc | box_recall | sym_recall | indent_exact | rows_ratio | ms/img |
|---|---|---|---|---|---|---|---|
| mock（只测几何层） | **1.000** | 1.000 | 1.00 | 1.000 | **0.99** | 1.00 | 8 |
| llamacpp（GLM-OCR Q8） | **0.957** | 0.983 | 0.88 | **0.996** | 0.91 | 0.98 | 3884¹ |
| tesseract | 0.899 | 0.916 | 0.88 | 0.949 | 0.87 | 0.93 | 596 |

¹ 被 4K 切块（124 s）拉高；典型场景 0.8–1.5 s。其它变体：`@1x` 0.934、`@2x.jpg` 0.957、`@photo` 0.946。
完整报告与分场景矩阵见 [docs/baseline.md](docs/baseline.md)。

---

## 7. 平台与依赖

| 依赖 | macOS | Windows |
|---|---|---|
| 引擎进程 | `llama-server`（brew 或官方 release） | 同上（Vulkan / CUDA / CPU） |
| GPU | Metal | Vulkan / CUDA；无 GPU 自动回落 CPU |
| 兜底引擎 | `tesseract`（brew） | `winget install UB-Mannheim.TesseractOCR` |
| 语言包 | 系统自带 | tesseract 需另装 `chi_sim` 等 |

实测参考（M5，`tree@2x`，0.9B Q8）：GPU 0.07 CPU-秒/次、0.51 s 墙钟；纯 CPU 2.95 CPU-秒/次、0.75 s 墙钟。

**内存（Windows 侧，未实测，机制说明）**：Windows 同样会把不活跃页换到 `pagefile.sys`，
并在 Win10+ 对冷页做**内存压缩**（压缩后仍在 RAM，不落盘）。模型权重的 mmap 在 Windows 上也走
standby list，和 macOS 的 mapped file 等价。结论一致：**不要指望靠"换到磁盘"省内存**，
要压常驻就调 `-c`、`-ctk/-ctv`（GPU 时）并开 `--sleep-idle-seconds`；`-np` 只是把
`-c` 分给多个 slot，不会减少总 KV。另外**不要关闭页文件**——关掉之后大图或
`--jobs>1` 会直接 OOM 崩溃，而不是变慢。

---

## 8. 已知限制

1. **框线字符**：GLM 不输出 box-drawing（`box_recall = 0`），但 `text_acc` 仍在 0.92–1.00——字全对，框线全丢。`--box-art` 能从像素恢复 `tree`/表格线，但它依赖直线检测，默认关闭；合并单元格和模型丢失左偏移时只能部分恢复。
2. **软换行默认关闭**：像素层面无法区分"长命令恰好顶到右边距"和"真的被换行"（`tree` 是反例）。
3. **行号槽场景仍依赖模型识别行号**：行号列会把每行 x0 拉到同一位置，几何改为在行带内部找第二个墨迹段来定位代码列；行号本身的文本仍来自模型。
4. **Windows 覆盖面**：桌面端已在实际 Windows 机器上验证安装、模型下载/启动和自动更新；CLI、多显示器/高 DPI 和不同 GPU 驱动仍需更多覆盖。
5. **评测集以合成图为主**：26 个场景里只有 2 张是真的截图（TextEdit + `screencapture`）。
6. **Windows 自绘选区**：一次只抓鼠标所在显示器，不支持跨屏拖选；GDI 对受保护内容/HDR/部分硬件加速窗口可能拿到黑帧。
   macOS 产物是 arm64，Intel Mac 需单独构建并补 `runtime/macos-x64`。
7. **首次安装需要一次 1.0–1.4 GB 模型下载**（高精度 1.43 GB / 低精度 1.03 GB）：已在应用内做一键下载、断点续传和 SHA-256；完全离线分发可另做模型包。
8. **自动更新**：从 1.0.0 起在应用内检查和安装新版本。更新依赖 GitHub `latest.json` 可达，且安装器未做代码签名，SmartScreen 仍可能提示。
9. **快捷键形状**：系统全局快捷键要求组合中恰好有一个主键，因此纯修饰键（`Ctrl+Shift`）或两个普通键（`A+B`）不支持；支持 1 个主键、修饰键+主键、两个修饰键+主键。

维护这些限制的流程、以及改代码前必须知道的坑，见 [docs/maintenance.md](docs/maintenance.md)。

---

## 9. 桌面外壳（`app/`，可选）

根 crate 仍然是纯 CLI + 库；`app/` 是它的 Tauri 2 + React + TypeScript 外层：
**全局快捷键 → 区域选择（macOS 系统 / Windows 自绘） → 本地内核 → 剪贴板**，设置面板把 `pipeline::Config` 除测试用 mock 外的参数都暴露出来。

![首启一键安装](app/docs/setup-idle.webp)

| | |
|---|---|
| 快捷键 | 默认 `⌘⇧2` / `Ctrl+Shift+2`；支持 1 个主键、2 键或 3 键组合，设置里可录制 |
| 选区 | macOS `screencapture -i`；Windows 自绘浮层 + GDI 抓屏（鼠标所在显示器） |
| 输入 | 全局快捷键 / 拖拽图片到窗口 / 从剪贴板识别；三条路径共用同一套引擎、错误和剪贴板逻辑 |
| 结果 | 成功后直接写剪贴板；窗口隐藏时不抢焦点；窗口不在前台时在鼠标所在屏幕底部弹一条「已复制 · N 行」，约 2.5 秒淡出，不抢焦点 |
| 引擎 | 运行时随包；模型首次一键下载，自动校验、自动启动；可停止/移除/重下；macOS 可开「快速预览」，先用系统 OCR 出结果再由本地模型精修 |
| 识别 | 始终流式显示（`ocr:partial`）；结果被判定可能截断时保留截图并提供「重新识别（分块）」 |
| 远程部署 | 设置 → OCR → 部署到服务器：SSH 一键部署到 Linux，支持服务器下载（4 连接并行、可续传）或上传本机模型、CPU/Vulkan/CUDA；已有模型时主操作变为「连接」，不会重复下载；远程服务与隧道只监听回环 |
| 设置 | 引擎/模型精度/KV 量化/空闲释放/代理/镜像/URL/prompt 预设/max-tokens/jobs/tesseract/几何/切块/格式/历史/快速预览/界面缩放；分通用/OCR/输出/高级/关于五段 |
| 外观 | 默认无边框 1120×740、115% 界面缩放；macOS NSVisualEffectView / Windows Acrylic |
| 更新 | 启动检查 GitHub Releases；签名校验、自动下载、重启安装 |
| 历史 | 最近 50 条存在应用数据目录 `history.json`（旧 localStorage 首次启动自动迁移）；可搜索、置顶、导出 Markdown/JSON、复制/删除/清空 |

### 安装依赖怎么处理

| 依赖 | 体积 | 方式 |
|---|---|---|
| 应用 + Tauri | ~15–80 MB | 安装包 |
| `llama-server` 运行时（macOS arm64 / Windows x64） | ~29 / ~49 MB | **随包内置** |
| Visual C++ 运行库（Windows） | ~1.5 MB | **随包内置**（避免旧 `msvcp140` 导致 `0xc0000005`） |
| 截图 | 0 | macOS 系统功能；Windows 自绘浮层 + GDI |
| WebView2（Windows） | — | 安装器自动处理 |
| GLM-OCR 高精度 Q8_0 / 低精度 Q4_K_M + mmproj | **1.43 / 1.03 GB** | **应用内一键下载**（断点续传 + SHA-256） |
| tesseract（可选兜底） | ~50–100 MB | 不强制、不打包；系统装了才用 |

用户首次启动只需点一次 **下载并配置**：应用自动下载两个模型文件、校验、启动
`llama-server`，完成后再按快捷键即可。无需安装 llama.cpp、无需手动放模型、无需开服务。
应用可在 **设置 → 更新** 检查和安装后续版本；更新包经过 Tauri/minisign
签名校验，签名在 CI 中完成。详细来源/许可见 [app/THIRD_PARTY.md](app/THIRD_PARTY.md)。

```bash
cd app
pnpm install
pnpm tauri dev                 # 开发
pnpm tauri build --no-bundle   # 本地构建，不生成需要签名的更新包
pnpm tauri build               # 发布包；需要 updater 签名密钥（CI 已配置）
```

只预览 UI（不需要模型，走 mock 数据）：`pnpm dev` 后打开
`http://127.0.0.1:1420/?demo=result`
（另有 `setup` / `permission` / `error` / `history` / `settings`）。

改界面前先读 [桌面端 UI 交互与文案规范](docs/ui-guidelines.md)：状态机、阻塞态出口、
弹窗焦点、术语表和“AI 味”对照都在里面。

原生窗口：[app/docs/capture-window.webp](app/docs/capture-window.webp)；
模型下载/校验/服务启动、更新与 Windows 自绘选区的实测记录见 [app/README.md](app/README.md)；
版本变更见 [CHANGELOG.md](CHANGELOG.md)。

---

## License

MIT；随包分发的 llama.cpp runtime 与下载的 GLM-OCR 权重同样是 MIT。
第三方许可见 [app/THIRD_PARTY.md](app/THIRD_PARTY.md)。
