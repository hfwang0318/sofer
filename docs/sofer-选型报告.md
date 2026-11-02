# 代码 / 终端截图 OCR 本地工具 · 现状调查与选型

> 目标：**macOS + Windows 双平台**、离线本地运行，把**代码截图、终端输出、报错堆栈**转成可直接粘回编辑器或喂给 LLM 的文本。
> 实测环境：macOS 27 / Apple M5 / 16GB（Windows 侧为同代码路径，未实测，见 §9.3）。
> 调查时间：2026-10。结论基于公开资料 + 本机实测（见 §4，可复现）。

---

## 0. 结论先行

> 产品定位是 **macOS + Windows 双平台**，所以原则是：**内核必须跨平台，任何单平台 API 只能当"加速档"，不能当依赖**。

| 决策点 | 结论 |
|---|---|
| 整体形态 | 不是"跑一个 OCR 模型"，而是 **预览 → 主力模型出文本 → 纯 CV 出几何 → 语法先验修复 → 结构化输出** 的管线 |
| 外壳 | **Tauri 2**（2.5–10 MB，Rust 侧做截图/热键/托盘），Electron 仅在团队无 Rust 时选 |
| **主力模型** | **GLM-OCR Q8 GGUF（llama.cpp，MIT，1.3 GB）**：`sym_recall` **0.998**、中英混排逐字正确、无复读；2.7 s / 2.4 GB；**两端同一个文件** |
| 预览档（可选） | mac **Vision** / win **`Windows.Media.Ocr`**：<200 ms 先给个占位结果，**不落盘、不当最终文本** |
| 版面重建 | 自研且**平台无关**：行投影切带 + 等宽格点拟合 + 缩进重建。**纯 CV 即可，不需要任何引擎的 bbox**，实测 6/7 场景缩进 1.00（§4.6） |
| 被否方案 | **MLX 系**（Apple 专属，Windows 无法复用）；**PaddleOCR-VL-1.6 4bit**（复读崩溃、丢 box 字符、无 bbox） |
| 产品切口 | 现有工具在「等宽语义 + 版面保真 + ANSI/结构还原 + 给 LLM 的输出协议」上集体空白（§2.3） |

一句话：**主力就一个模型（GLM-OCR），文本由它给、版面由纯 CV 给、平台原生 OCR 只做 200ms 的占位预览**。不要把一个平台的 API 写进主路径，也不要维护多套文本输出。

> ⚠️ 本文所有数字均来自 macOS/M5 实测；Windows 侧为**等价的同代码路径**，但**精度与耗时必须在 Windows 机器上复测**（§9.3 给了复测清单）。

## 1. 需求拆解：为什么这不是"普通 OCR"

普通 OCR 的目标是"字对"；代码/终端的目标是**"粘回去能跑/能读"**。中间差着四类信息，而它们**不在像素里**或**不被通用 OCR 建模**：

| 信息 | 现状 | 后果 |
|---|---|---|
| 前导缩进 | 空格没有 ink，任何 OCR 都不会"看到"它 | Python/YAML 直接语法错误 |
| 列对齐 | 表格/对齐赋值依赖空格数 | 输出错行、无法 diff |
| ANSI 颜色语义 | 渲染成像素后已丢失，只剩最终前景/背景 | `git diff` 的 +/- 归属、报错红行全部不可知 |
| soft wrap | 终端软换行在像素上与硬换行**不可区分** | 一条长命令被拆成多行，粘回去是错的 |

另外还有等宽字符歧义（`l/I/1`、`0/O`、`rn/m`、`` ` `` 与 `'`）、box-drawing（`├──`、`┌─┬─┐`）、Nerd Font 私有区字形（U+E000–F8FF 无 Unicode 语义）、连字字体把 `=>` `!=` 渲染成单字形。**这些是产品的真正战场，不是模型参数量的战场。**

已有一个学术佐证：[CRec（SoftwareX 2026）](https://doi.org/10.1016/j.softx.2026.102735) 把问题定义为"可编译性恢复"，在 1,597 个不可编译样本上把 CodeT5-OCRfix 的可编译修复率从 29.12% 提到 47.84% —— 说明**纯 OCR 输出离"可用"还有很远，后处理是独立且必要的一层**。

---

## 2. 工具现状

### 2.1 macOS 生态

| 工具 | 离线 | 代码/等宽处理 | 输出 | 可脚本化 |
|---|---|---|---|---|
| Apple Live Text / Vision | ✅ | 无等宽概念，丢缩进 | 纯文本 | ✅ API 完善 |
| [TextSniper](https://textsniper.app/) | ✅ | 官方 FAQ 自认"符号多时（如代码）会误读" | 纯文本 | ❌ |
| Shottr / CleanShot X / Snipaste / PixPin | ✅ | 同上，通用取词 | 纯文本 | ❌ |
| [Snapline](https://snap-line.app/)（2026） | ✅ | 主打 "Copy as Code"：按 x 坐标重建缩进 | 纯文本/代码 | 厂商称有 MCP |
| Raycast OCR 扩展 | ✅ | 调 Vision | 纯文本 | 部分 |

### 2.2 Windows / Linux / 开源

| 项目 | 形态 | 关键能力 |
|---|---|---|
| [Umi-OCR](https://github.com/hiroi-sora/Umi-OCR) | Qt 桌面 + CLI + HTTP | **唯一显式做"代码截图：单栏-保留缩进"预设**的开源方案；仅 Win/Linux |
| PowerToys Text Extractor | 系统工具 | 通用取词 |
| normcap / gImageReader / OCRmyPDF | CLI/桌面 | Tesseract 系，代码场景弱 |

### 2.3 空白点（差异化切口）

1. **没有任何主流工具处理 ANSI 颜色语义**：`git diff` 的红绿、`ls` 的类型色、报错高亮全部丢失，用户只能自己猜。
2. **没有专门的 "terminal screenshot → text" 产品**；`screenshot-to-code` 是"截图→UI 代码"，是另一个任务。
3. **没有 ` ```lang ` 围栏 / JSON 结构化报错输出**：工具都吐"裸文本"，而这正是喂给 LLM 时最需要的。
4. **没有 Code-OCR 公开基准**（§3.3），所以没人知道自己的模型在代码上到底行不行。

> 结论：赛道不空，但**"离线 + 等宽感知 + 结构还原 + 面向 LLM 的输出"这个组合是空的**。

---

## 3. 模型现状

### 3.1 梯队（2026-10）

| 模型 | 参数 | 许可 | 结构输出 | 公开成绩 | 本地推理（GGUF / llama.cpp） |
|---|---|---|---|---|---|
| PaddleOCR-VL-1.6 | 0.9B | Apache-2.0 | md/json + bbox | OmniDocBench v1.6 **96.34** | ✅ **官方 GGUF**（2026-11 起）+ llama.cpp 原生支持；实测见 §3.4 |
| **GLM-OCR** | 0.9B | **MIT** | md/json | v1.5 94.62 | ✅ **官方 GGUF（Q8 0.9 GB + mmproj）**，两端通用 |
| Qwen3-VL-4B / 8B | 4B/8B | Apache-2.0 | HTML + bbox | OCRBench 88.1（8B）；olmOCR-Bench 8B 仅 64.6 | ✅ GGUF 生态全，但 4B 起内存/延迟高 |
| dots.mocr | 3B | MIT | bbox + HTML/SVG | olmOCR-Bench 83.9 | ⚠️ 社区 GGUF 仅 F16/Q8，生态薄 |
| Chandra 2 | ~9B | **OpenRAIL-M**（商用受限） | md/HTML | olmOCR-Bench 85.8 | ❌ 体积与许可都不合适 |
| MinerU 2.5-Pro | 1.2B | Apache-2.0 + 附加条款 | md + 版面 bbox | v1.6 95.75 | ⚠️ PyTorch + llama.cpp 混合，工程重 |
| DeepSeek-OCR-2 | 3B | Apache-2.0 | md + grounding | v1.6 90.25；独立测试偏弱 | ⚠️ 社区 GGUF（社区 PR 支持 mtmd） |
| olmOCR 2 | 7B | Apache-2.0 | md | 82.4 | ❌ 仅 vLLM |
| Tesseract / RapidOCR(PP-OCRv6) | 传统 CNN | Apache-2.0 | 行/词 bbox | — | ✅ ONNX，CPU 毫秒级，两端同文件 |

> 选型视角：**能出官方 GGUF 的只有 GLM-OCR 和 PaddleOCR-VL-1.6**。其余要么只有 MLX
> （Apple 专属），要么只有社区二次量化（许可与质量都要自己兜）。但"能跑"只是入场券，
> 代码场景的实际质量见 §3.4。

### 3.2 榜单为什么不能直接用来选型

- **OmniDocBench / olmOCR-Bench 的语料是论文、公文、扫描件**，没有终端截图、没有深色主题、没有等宽字体。
- 榜单里唯一与代码沾边的类别是 `code_txt`，但**不单列分数**。
- Terminal-Bench 2.1 的 `code-from-image` 考的是"agent 能否实现图里的伪代码"，不是 OCR 指标。
- 结论：**榜单只能筛出"不差"的候选，不能决定"在代码上谁更好"** —— 必须自建评测（§4）。

### 3.3 量化的坑

唯一可查的量化实测来自 PaddleOCR-VL-1.6 的 MLX 模型卡：4bit 反超 5bit 的"字段准确率"其实是**模板漂移**（换 prompt 排序就反转）。本机实测也验证了这点（§4.3）。

### 3.4 2026-11 复测：OXR 与两个更小的候选

**OXR（SoMarkAI，1.3B）不能作为候选**，五条硬约束：

| 约束 | OXR | 本项目 |
|---|---|---|
| 运行时 | vLLM + out-of-tree 插件，官方只在 Linux + NVIDIA 上验证 | llama.cpp（Metal / Vulkan / CPU），双平台同一 GGUF |
| 显存 | 24 GB 起（推荐 48 GB） | 16 GB 统一内存 / Windows 纯 CPU |
| 结构 | 1.3B BF16 **加** 独立 DFINE 版面模型，直接输出版面与阅读顺序 | 模型只出文本，版面由 `geom`/`assemble` 从像素重建 |
| 许可 | "OXR Open Source License"：融资或营收 > 200 万美元需单独商业许可，且禁止竞品托管 API | 桌面 App 分发，要求 MIT / Apache |
| 可验证性 | 无技术报告，训练数据、上下文长度未公开 | 本报告 §3 的准入门槛 |

另外它没有官方或社区 GGUF，llama.cpp 目前不支持该架构——即使许可没问题，也进不了
app 的模型包流程。

**两个真正对口的候选**：OvisOCR2（0.8B / Apache-2.0 / 社区 GGUF）与
PaddleOCR-VL-1.6（0.9B / Apache-2.0 / 官方 GGUF）。26 场景实测见
[baseline.md §3.4](baseline.md)：两者都没有在任何一项上同时超过 GLM-OCR——

| | GLM-OCR | OvisOCR2 | PaddleOCR-VL-1.6 |
|---|---:|---:|---:|
| `sym_recall` | **0.996** / 0.994 | 0.956 / 0.955 | 0.937 |
| `rows_ratio` | **0.98** | 1.47 / 1.49 | 0.92 |
| 单请求 ms | 1288 / **922** | 1944 / 1331 | 3356 |
| 空闲 RSS | 2409 / **1879 MB** | 1669 / **1078 MB** | — |

（GLM 与 Ovis 行的两个数是 Q8 / Q4_K_M；Paddle 只有 Q8。Q4 的 `sym_recall` 与 Q8 在噪声内。）

- **OvisOCR2** 的 `sym_recall` 低 3.9 pp，且 `pytest-traceback` 单场景低 60 pp（0.394 vs
  1.000）；每图还多吐 47% 的空行。它唯一的优势是省 ~800 MB 内存——不足以抵消。
- **PaddleOCR-VL-1.6** 的 `sym_recall` 低 5.9 pp、`huge-4k` 截断到 19/56 行，且在非模板
  prompt 下复读（单图 45 s，`rows_ratio` 25.7）。官方 Q8 GGUF 没有解决老问题。

**结论：维持 GLM-OCR。** OXR 因为跑不起来被排除，另两个因为"在代码场景更差"被排除；
若将来出现"常驻必须 < 1.2 GB"的硬需求，OvisOCR2 Q4 是唯一能进预算的候选，
但需要先实现空行的几何重建（见 [baseline.md §4](baseline.md) 结论 ⑥）。

---

> 本节是最初的选型实验（8 场景，独立 Python harness）。内核完成后的可复现基线（20 场景 × 3 变体、走 CLI）见 [docs/baseline.md](baseline.md)。

## 4. 本机实测（macOS / M5 / 16GB / 离线）

### 4.1 方法

7 张合成但真实的场景图（Menlo 13pt @2x，Dracula 深色 + 浅色主题，含真实 ANSI 配色语义），每张都有**逐字符 ground truth**：

| 场景 | 考察点 | 行数 |
|---|---|---|
| `git-diff` | 红绿语义、嵌套缩进、反引号模板串、`${}` | 23 |
| `pytest-traceback` | 4 空格缩进 3 层、`>E` 标记、`{'GET'}` | 20 |
| `tree` | box-drawing `├── │ └──` + 4 空格缩进 | 15 |
| `ascii-table` | 制表符框线 + 列对齐 | 11 |
| `editor-rust` | 行号沟槽 + 8 空格缩进 + `&&` `->` `..` | 20 |
| `light-ambiguous` | 浅色主题 + `Il1O0` `rn/m` 歧义文件 | 10 |
| `softwrap` | 52 列软换行后的长日志 | 10 |

指标：`content_acc`（去空白后字符准确率）、`sym_recall`（代码关键符号召回：`(){}[]<>|;:=+-*/&^%$#@!~` 等）、`indent_exact`（版面重建后行首空格完全正确的比例，按文本对齐避免空行错位）、`rows_ratio`（重建行数 / 真值行数）、延迟、峰值内存。

### 4.2 结果（@2x PNG，7 场景均值）

| 引擎（运行时） | content_acc | sym_recall | indent_exact | rows_ratio | ms/图 | 峰值内存² | 跨平台 |
|---|---|---|---|---|---|---|---|
| Qwen3-VL-4B (MLX 4bit) | **0.903** | 0.987 | 0.69 | 0.96 | 5368 | 4.1 GB | ❌ 仅 Apple |
| **GLM-OCR Q8 (llama.cpp)** | 0.881 | **0.998** | 0.72 | 1.05 | 2691 | 1.8–2.4 GB | ✅ **mac+win** |
| GLM-OCR 0.9B (MLX 4bit) | 0.875 | **0.992** | 0.67 | **1.00** | 1763 | 2.2 GB | ❌ 仅 Apple |
| Tesseract (psm 6, TSV) | 0.861 | 0.906 | **0.86** | 0.96 | **511** | **42–57 MB** | ✅ |
| RapidOCR (PP-OCRv6 ONNX) | 0.838 | 0.858 | **0.86** | 0.92 | 614 | 1.06 GB¹ | ✅ |
| Apple Vision (accurate) | 0.827 | 0.883 | 0.84 | 0.90 | **110** | **47–69 MB** | ❌ 仅 macOS |
| Apple Vision (`RecognizeDocumentsRequest`) | 0.778 | 0.842 | n/a | n/a | 188 | 47–69 MB | ❌ 仅 macOS |
| PaddleOCR-VL-1.6 (MLX 4bit) | 0.231* | 0.792 | 0.76 | 0.74 | 3247 | 1.9 GB | ⚠️ GGUF 有，社区版 |

\* 被 `git-diff` 的复读崩溃拉到负值（详见 4.3）。

¹ RapidOCR 那个 1.06 GB 是 **Python + onnxruntime 的进程 RSS**（ORT 的 arena 预分配占大头），不是模型本身；模型只有十几 MB。产品里用 Rust `ort` 不会有这层开销，实际应在 100–300 MB 量级 —— 这也是 §9.1 坚持"不引入 Python 运行时"的原因。
² 内存口径不统一，横向比较要小心：Tesseract/Vision 是**子进程 `ru_maxrss`**（`/usr/bin/time -l`）；RapidOCR 是进程 RSS；MLX 是 `mx.get_peak_memory()`（Metal buffer 峰值，不含 Python 堆）；llama.cpp 是**服务进程 RSS 峰值**（采样 10 Hz，模型走 mmap）。同一个引擎换口径能差 20%，但**数量级结论（几十 MB vs 1–2 GB）是可靠的**。

分场景 `content_acc`：

| 引擎 | git-diff | pytest | tree | table | editor | light | softwrap |
|---|---|---|---|---|---|---|---|
| Qwen3-VL-4B | 0.988 | 0.848 | **1.000** | 0.578 | 0.934 | 0.970 | **1.000** |
| GLM-OCR | 0.986 | **0.901** | 0.750 | 0.581 | 0.934 | 0.970 | **1.000** |
| Tesseract | 0.978 | 0.784 | 0.767 | 0.578 | **0.994** | 0.931 | 0.992 |
| RapidOCR | 0.971 | 0.708 | 0.761 | 0.578 | 0.989 | 0.866 | 0.994 |
| Vision | 0.974 | 0.697 | 0.750 | 0.565 | 0.966 | 0.889 | 0.949 |
| Paddle-VL | 崩溃 | 0.695 | 0.761 | 0.578 | **0.996** | **0.985** | **1.000** |

> `table` 场景所有引擎都在 0.57 左右：真值里有大量 box-drawing 框线，**除 Qwen3-VL 外基本输出不了**（部分还会掺入 markdown 管符），因此该列不可横向比较，实际质量看 §4.3 的原始输出。

### 4.3 五条关键发现

**① 缩进必须靠几何重建，不能靠模型"数空格"。**
- 所有引擎的原始输出都把行首缩进压平（`content_acc` 里看不出来，因为去空白比较）。
- 用词级 bbox + 等宽格点拟合重建后：Vision 的 `indent_exact` 达到 **0.84**、Tesseract/RapidOCR **0.86**。
- VLM（无 bbox）只能靠自己吐空格，`indent_exact` 0.67–0.72 —— **少了 12–19 个百分点**。
- 同一层还负责**修碎片**：Vision 会把一行按大间隙拆成多段（`ascii-table` 10 行被拆成 27 段、`softwrap` 10 行拆成 21 段、`light-ambiguous` 18 段），按 y 带聚类后全部回到 1:1。
- 结论：**bbox 是这类产品最值钱的资产**，没有 bbox 的引擎天然瘸一条腿。

**② 传统引擎 + 版面重建 ≈ 小 VLM 的内容准确率，但快 3–25 倍。**
Tesseract 0.861（0.5 s）/ Vision 0.827（0.11 s）vs GLM-OCR Q8 0.881（2.7 s）。差距集中在"符号"上（`sym_recall` 0.86–0.91 vs 0.998），这部分可以用**代码语法先验 + 双引擎投票**补 —— 而延迟差距是补不回来的，所以**快档必须保留**。

**③ 公开榜 SOTA ≠ 代码场景可用（PaddleOCR-VL 的反例）。**
`OCR:` prompt 下，`git-diff` 输出退化成 2,665 字符的 `++++++...` 复读（`content_acc` 为负）；换 `Convert to document to markdown.` 又泄漏 `<|LOC_11|>` grounding token。同时它把 `dirsfirst` 读成 `dirfirst`（漏字符）、`├──` 整段吞掉。**1B 级模型 + 4bit 量化在密集红绿文本上不稳定，必须有复读哨兵。**

**④ 模型间的强项差异极大，"谁更好"取决于场景。**
- `tree`：Qwen3-VL 完美复刻 `├──` 与缩进（1.000）；GLM-OCR 直接拍平成 `| --- cli`（0.750）；Paddle 全吞（0.761）。
- `pytest-traceback`：GLM-OCR 最好（0.901），Vision 最差（0.697，把 `FnOnce() -> V` 读成 `Fnonce() > V`）。
- `git-diff`：所有引擎都能到 0.97+，**颜色不是难点，复读和符号才是**。
- `softwrap`：VLM 1.000 说明它们能"看懂"长行；Vision 0.949 会拆行。
→ **单一引擎一定会踩到自己的盲区，多引擎投票/升级是刚需。**

**⑤ `RecognizeDocumentsRequest`（macOS 26 新 API）不适合这个场景。**
它会给段落/表格/列表结构，但本机实测：**丢行首空格、合并/切断代码行、表格列错位**，`content_acc` 比老 `VNRecognizeTextRequest` 低 5 个点。结构化 ≠ 等宽保真。

---

### 4.4 跨平台路径复测：同一个 GGUF，llama.cpp 跑赢 MLX 4bit

把质量档从 MLX 换成 **llama.cpp + GLM-OCR Q8_0 GGUF**（`ggml-org/GLM-OCR-GGUF`，模型 1.3 GB + mmproj 0.3 GB，MIT），用 `llama-server` + Metal（`-ngl 99`）复跑同一批图：

| 场景 | content_acc | sym_recall | ms | completion tok |
|---|---|---|---|---|
| git-diff | **0.996** | 0.988 | 4415 | 271 |
| pytest-traceback | 0.911 | 1.000 | 3078 | 189 |
| tree | 0.767 | 1.000 | 967 | 68 |
| ascii-table | 0.581 | 1.000 | 2531 | 166 |
| editor-rust | 0.935 | 1.000 | 3980 | 174 |
| light-ambiguous | **0.985** | 1.000 | 2026 | 214 |
| softwrap | **0.997** | 1.000 | 1840 | 193 |
| **均值** | **0.881** | **0.998** | **2691** | — |

结论：
1. **GGUF-Q8 比 MLX-4bit 更好**（0.881 vs 0.875，符号召回 0.998 vs 0.992），说明"为跨平台牺牲精度"这件事**不成立** —— 反而不用量化到 4bit。
2. 内存 2.4 GB、单图 1–4.4 s，16 GB 机器完全够；Windows 上换成 Vulkan/CUDA/CPU 后端即可，**模型文件与 prompt 完全不用改**。
3. 唯一要注意的坑：GLM-OCR 的 GGUF 若缺 `chat_template`，默认 framing 会触发**模板 token 复读死循环**（[llama.cpp #31298](https://github.com/ggml-org/llama.cpp/issues/31298)）。本测试直接给 `OCR:` 纯文本 prompt 正常，说明走 server 的消息模板路径会正确处理，但**落地时仍需显式校验**。

### 4.5 Tesseract 的短板（误差解剖）

Tesseract 的 42–57 MB / 0.5 s 确实漂亮，`content_acc` 也只差 2 个点。但**误差的分布极不均匀**——它丢掉的恰好是代码里最贵的字符。

**(a) 符号错误是系统性的，不是随机的**

对 4 个场景做字符级替换分析（`difflib` opcodes）：

| 场景 | 典型替换 |
|---|---|
| git-diff | `` ` `` → `* `／`°`；空格 → `4` |
| editor-rust / light-ambiguous / pytest | `-` → `—`（em dash，跨全部场景出现） |
| light-ambiguous | `0` → `Q`（×2）；`-` → `—` |

后果直接且致命：`--stat` 变 `——stat`（每个 CLI flag 失效）、模板串 `` `x` `` 变 `*x*`、`8080` 变 `8O8O`。这就是 `sym_recall` 0.906 vs GLM-OCR 0.998 的来源——**9 个点的差距全在符号上**。

**(b) box-drawing 不在字符集里，结构直接毁**

`tree` 场景实测输出：

```
f— cli            ← ├──
| [_ main.py      ← │   ├──
L_— options.py    ← │   └──
```

不是"识别率 76%"，而是**树形语义 100% 丢失**。这类字符没进 `eng` 的字符表，只能靠后处理映射表硬修，而映射的前提是先能认出来。

**(c) 小字号悬崖**

同一张 `pytest-traceback` 图，从 @2x 降到 @1x（模拟非 Retina / 聊天软件压缩）：

| 引擎 | @2x sym_recall | @1x sym_recall |
|---|---|---|
| Tesseract | 0.906 | **0.345** |
| Vision | 0.424 | 0.320 |

Tesseract 对字号最敏感（LSTM 的分割对笔画宽度依赖强）。桌面截图多为 2x，但**微信/Slack 转发的图经常被压到 1x**，这是真实场景。

**(d) 中文要另装语言包，且两条路都有硬伤**

新造一张中英混排图（中文注释 + Python 代码 + 中文报错）：

| 配置 | 结果 |
|---|---|
| `-l eng`（默认只有这个） | `# BOARS, AT 8080 twA` —— 中文全变乱码 |
| `-l chi_sim` | 中文对了，但**英文标点全角化**：`def main（host=...）`、`app.py` → `app.-py` |
| `-l eng+chi_sim` | 代码对了，但**中文每个字之间被插空格**：`# 启动 服务 ， 监 听 8080 端口` |
| **GLM-OCR** | **逐字正确**，连第 4 行的 4 空格缩进都对 |

两个附带坑：① brew 的 tessdata 在 Cellar 里只读，`cp` 静默失败；② **语言包缺失时 `tesseract -l chi_sim` 不报错、直接输出空**（我第一次就踩了）。产品里必须显式校验 `--list-langs`。

**(e) 没有版面/语义后处理能力**

它不会输出 markdown 表、不会按语法把 `l` 改成 `1`、不会判断"这是 diff，`+` 行是新增"。而且**调参没救**：`psm 3/4/6/11` × `dpi 144` 全组合实测差异 <1%（psm 6 已是最优）。

**(f) 速度优势是相对 VLM 的，不是绝对**

0.5 s vs Vision 的 **0.11 s** —— Tesseract 比系统 OCR 慢 5 倍。它的"快"只成立于"对比 GLM-OCR 的 2.7 s"。

**结论**：Tesseract 适合当**跨平台兜底档 / 极简离线模式**（纯 ASCII、高对比、≥12px、无框线，正好是"正常代码截图"的主体），不适合当主档；且必须配一组后处理规则（`—`→`-`、`°`→`` ` ``、全角标点折叠、box 映射）。它的一个真实优势是**有 per-word confidence**，可以驱动"低置信升级"，这点比 `Windows.Media.Ocr` 强。

### 4.6 关键验证：缩进可以不依赖任何引擎的 bbox

GLM-OCR 唯一的硬短板是"没有 bbox → 缩进只能靠模型自己吐空格"（0.72）。但缩进本质上是**几何问题，不是语义问题**：

```
灰度 → 与背景色做差（极性无关）→ 行投影切带 → 每带取最左 ink 的 x → 等宽格点拟合 → indent = round(Δx / advance)
```

**完全不调用任何 OCR**，纯 numpy + PIL。实测（7 场景，与真值逐行位置比对）：

| 场景 | 检测到的行带 | 真值行 | 缩进正确率 |
|---|---|---|---|
| git-diff | 22 | 22 | **1.00** |
| pytest-traceback | 18 | 18 | **1.00** |
| tree | 14 | 14 | **1.00** |
| ascii-table | 10 | 10 | **1.00** |
| light-ambiguous | 10 | 10 | **1.00** |
| softwrap | 10 | 10 | **1.00** |
| editor-rust | 20 | 20 | 0.00 ← 行号沟槽 |

唯一的 0 分是"行号沟槽"：每行 x0 都在第 0 列（因为行号），需要先剥掉沟槽再算缩进（§6 第 3 步已列）。这不是算法失败，是少一条规则。

**含义**：主力模型**不需要**提供 bbox，也就不需要绑定 Vision / Windows OCR。缩进交给 CV 层，文本交给模型，两者解耦——这是双平台架构能成立的关键。

## 5. 选型结论（双平台）

### 5.1 推荐架构：单模型 + 几何层 + 可选预览

```
                    ┌────────────────────────────────────────────┐
   截图 ────────────►│ ① 预览档（<200ms，可选、可关）               │
                    │   mac: Vision      win: Windows.Media.Ocr    │
                    │   作用：让用户看到"有反应了"，不参与最终结果   │
                    └──────────────────┬─────────────────────────┘
                    ┌──────────────────▼─────────────────────────┐
                    │ ② 主力引擎：llama.cpp + GLM-OCR Q8 GGUF      │
                    │   mac: Metal  win: Vulkan / CUDA / CPU       │
                    │   同一个 .gguf、同一个 prompt、同一份代码     │
                    └──────────────────┬─────────────────────────┘
                    ┌──────────────────▼─────────────────────────┐
                    │ ③ 几何层（纯 CV，平台无关）                  │
                    │   行投影切带 → 最左 ink → 格点拟合 → 缩进     │
                    │   soft-wrap 合并 → 行号槽剥离                │
                    └──────────────────┬─────────────────────────┘
                    ┌──────────────────▼─────────────────────────┐
                    │ ④ 文本修复层                                │
                    │   语法先验纠 l/1/0/O → 符号归一 → box 映射    │
                    │   低置信行二次投票（可选）                    │
                    └──────────────────┬─────────────────────────┘
                    ┌──────────────────▼─────────────────────────┐
                    │ ⑤ 输出层                                    │
                    │   裸文本 / ```lang / 结构化 JSON / ANSI 语义  │
                    │   剪贴板 · CLI · 本地 HTTP · MCP              │
                    └────────────────────────────────────────────┘
```

**为什么砍掉"双引擎并行正文"**：三套文本输出（Vision / Tesseract / GLM）意味着三套误差特征、三套回归测试、两端表现还不一致。§4.6 证明了"缩进不需要引擎 bbox"之后，**保留多引擎的唯一理由（拿几何信息）也消失了**。

### 5.2 平台 × 引擎矩阵

| 能力 | macOS | Windows | 是否主路径 |
|---|---|---|---|
| **主力（最终文本）** | **GLM-OCR Q8 + llama.cpp** | **同一个 .gguf，同一条代码路径** | ✅ **唯一真源** |
| 预览档（<200ms，可选） | Apple Vision | `Windows.Media.Ocr` | ⚠️ 只做占位，不落盘 |
| 降级档（无 GPU/低配） | Tesseract（42–57 MB，纯 ASCII 够用，**须后处理修 `—`/`` ` ``/box**） | 同左（自带 tessdata，校验语言包） | ⚠️ 降级 |
| 备选降级档 | RapidOCR / ONNX RT（CoreML EP 实测无收益，走 CPU） | 同左（DirectML EP 亦无明显收益） | ⚠️ 降级 |
| 文本直取 | AX API（Warp 无树、iTerm2 有 NUL、Ghostty 数据错） | UIA TextPattern（终端有卡死风险） | ⚠️ 仅整屏 |
| 剪贴板/热键/托盘 | 原生 | 原生 | — |

**为什么 Windows 原生 OCR 只做预览、不做主力**：
- 它**没有 confidence**，做不了"低置信自动回退"，只能靠启发式（空串/乱码率）判断；
- 语言包是**可选功能**，企业精简镜像常缺失，`TryCreateFromUserProfileLanguages()` 会直接返回 `null`；
- 没有公开的"代码/终端截图"评测，能力未知。

**为什么不让"各平台各用一套"**：两端的输出质量差异会变成"客服问题"。主力只有一个模型，两端输出才能逐字节一致，回归测试也只需要一套基准。

### 5.3 运行时为什么选 llama.cpp

| 运行时 | mac 加速 | win 加速 | 结论 |
|---|---|---|---|
| **llama.cpp** | Metal | CUDA/Vulkan/CPU | ✅ **一套 GGUF + 一套 C API，双平台维护成本最低** |
| MLX | ✅ | ❌ | ❌ 直接出局（本次改进的核心） |
| ONNX Runtime | CoreML EP | CPU + DirectML（已转 sustaining） | ⚠️ 只用于轻量 CNN（RapidOCR），VLM 不放这里 |
| OpenVINO / Paddle Inference / ncnn | 部分 | 部分 | ❌ 单平台或转换成本过高 |

### 5.4 不采用清单

| 方案 | 原因 |
|---|---|
| MLX（含 mlx-vlm） | Apple 专属，Windows 无法复用（本报告第一版曾以此为主，**已修正**） |
| PaddleOCR-VL-1.6 | 官方 Q8 GGUF 复测后仍有复读、丢符号（`sym_recall` 0.937）、超大图截断（§3.4）；MLX 4bit 版本另有 grounding token 泄漏 |
| OvisOCR2 | `sym_recall` 0.955、`pytest-traceback` 0.394、每图多 47% 空行；省下的 ~800 MB 不足以抵消（§3.4） |
| OXR | vLLM + 24 GB 显存、无 GGUF、许可含竞品条款（§3.4） |
| DeepSeek-OCR / olmOCR / Chandra | 无官方 GGUF / 许可受限（OpenRAIL-M）/ 体积过大 |
| Windows.Media.Ocr 作质量档 | 无 confidence、语言包不可控 |
| 把 UIA/AX 当主路径 | 拿不到"用户框选的那一块"，且终端会卡死 |
| 把 Tesseract/RapidOCR 当主力 | `sym_recall` 只有 0.86–0.91，`-`→`—`、`` ` ``→`*`、box 字符读不出（§4.5）；留作"无 GPU 机器"的降级档 |

### 5.5 关键取舍

1. **一致性 > 单平台极致**：宁可 mac 上放弃 110 ms 的 Vision、走 2.7 s 的 GGUF，也不做两套引擎行为——但这里选择**保留快档做预览、质量档做最终结果**，两边行为仍一致。
2. **不追 SOTA 榜单，追"结构保真"**：`rows_ratio`、`indent_exact` 比 `content_acc` 更能预测"能不能用"。
3. **ANSI 颜色语义是一等输出**：`diff` 行标记 `+/-`、错误行标 `error`、注释/字符串分类 —— 竞品全空。
4. **输出面向 LLM**：默认 ```lang 围栏 + 保留缩进；结构化模式给 `{file, lines:[{text, indent, kind}]}`。

## 6. 重建层要做的具体算法

1. **行聚类**：按 y 中心分带（容差 0.55×行高），同行按 x 排序后拼接 —— 修掉 Vision 把一行拆成多段的问题（实测 `ascii-table` 27 段 → 1 行/行、`softwrap` 21 段 → 10 行、`light-ambiguous` 18 段 → 10 行）。
2. **等宽格点拟合**：用长行的 ink 宽度中位数做先验，再在 ±15% 内做一维格点最小二乘，求出 `advance`；纯格点拟合会退化（任何约数都完美拟合），必须先验锚定。
3. **缩进重建**：`indent = round((x0 - left)/advance)`；行号沟槽单独识别并剥离后重算。
4. **soft-wrap 合并**：`shouldWrapToNextLine` 或"下一行 x0 ≈ 块首 x0 且上一行接近右边距"判定，合并为逻辑行。
5. **符号归一**：box-drawing → ASCII（可选保留）、PUA → 图标名、连字展开表（`=>` `!=` `===`）。
6. **语法先验纠错**：对目标语言做轻量 token 化，用括号配对/缩进层级约束修正 `l/1/I`、`0/O`（例如 `if (` 后不可能出现大写 `I`）。
7. **复读哨兵**：n-gram 重复率 > 阈值即截断并回退到 Vision 结果（Paddle 的翻车就是没有这层）。

---

## 7. 风险与待验证

| 风险 | 说明 | 缓解 |
|---|---|---|
| 合成图偏差 | 本评测用 PIL 渲染，非真实截图 | 需补 50–200 张真实截图（iTerm2/VS Code/Chrome/微信转发压缩） |
| 4bit 量化不稳定 | Paddle 已翻车；GLM 未测 bf16 对照 | 关键路径固定 prompt + 字段级自测 + 复读哨兵 |
| Qwen3-VL OOM | 全尺寸 1444×813 直接在 16 GB 上 Metal OOM | 输入限 ≤1400 px（已实测可跑） |
| 中英混排 | 本评测以英文为主 | 补 CJK 宽字符（2 列宽）对齐测试 |
| **Windows 全链路未实测** | 报告数字全部来自 macOS | 按 §9.3 五项在 Windows 复测，快档降级路径必须跑通 |
| Windows 语言包缺失 | `Windows.Media.Ocr` 依赖可选语言包，企业镜像常缺 | 自动降级到 RapidOCR/ONNX，并在 UI 提示 |
| Tesseract 符号系统性错误 | `-`→`—`、`` ` ``→`*`、`0`→`Q`，`sym_recall` 差 9 点 | 若采用，必须加符号后处理 + 低置信升级；不作主档 |
| Tesseract 中文两条路都有硬伤 | 单语言包标点全角化；双语包给中文插空格 | 中文场景一律走 VLM 或 Vision（`zh-Hans`） |
| 权限 | 截图/辅助功能需 TCC 授权 | 首次引导 + 降级为"拖入图片" |

---

## 8. 路线图

**MVP（1–2 周）**：全局快捷键 → **GLM-OCR GGUF** 出文本 → **纯 CV 几何层**出缩进 → ```lang 围栏写剪贴板 + 文本历史。做到"Python 缩进零错"（§4.6 已验证 6/7 场景 1.00）。
**v1（+2 周）**：ANSI 语义还原、soft-wrap 合并、box/PUA 归一、CLI + 本地 HTTP（`POST /ocr` 返回 JSON）。
**v2**：行号槽剥离 + soft-wrap 合并 + 复读哨兵；**Windows 端复测 GLM-OCR 在 Vulkan/CPU 下的耗时**并定降级阈值；真实截图评测集（≥100 张）纳入 CI。
**v3**：MCP server（给 agent 用）、"结构化报错"模式（`{error_type, file, line, traceback}`）。

---

## 9. 双平台工程与分发

### 9.1 进程与依赖结构

```
myapp (Tauri 2)
├── Rust core：截图选区 / 热键 / 托盘 / 剪贴板 / HTTP / 版面重建
├── engine-vision  (macOS, Swift shim, dylib)      ─┐  同一 trait
├── engine-winocr  (Windows, WinRT, Rust windows crate) ─┤  "NativeOcr"
├── engine-onnx    (RapidOCR, ort crate, 两端同一 onnx) ─┤  "PortableOcr"
└── llama-server   (child process, 两端同一 GGUF)        ─┘  "QualityOcr"
```

- **不引入 Python 运行时**：ONNX 用 Rust `ort`，llama.cpp 用 sidecar 进程；否则 PyInstaller 的双平台打包/签名成本会吃掉整个项目。
- **引擎抽象成 trait**：`fn recognize(&self, img) -> Vec<Observation>`；`Observation { text, bbox, confidence: Option<f32> }`。Android/iOS 将来也能接。

| 依赖 | macOS | Windows | 坑 |
|---|---|---|---|
| 外壳 | WKWebView | **WebView2（需检测是否存在）** | 缺失时用 MessageBox 明示，别让用户看白屏 |
| 截图 | ScreenCaptureKit | Windows.Graphics.Capture / BitBlt | macOS 需 TCC 屏幕录制权限，首次要重启 app |
| 全局热键 | NSEvent / Carbon | `RegisterHotKey` | 两端都不需要管理员权限 |
| 快档 OCR | Vision（Swift shim） | WinRT `Windows.Media.Ocr` | Win 语言包可能缺失，需降级到 ONNX |
| 质量档 | llama.cpp（Metal） | llama.cpp（Vulkan/CUDA/CPU） | **同一 gguf 文件**，模型无需分平台 |
| 重建/输出 | 纯 Rust | 同左 | 保证两端输出一致的关键 |

### 9.2 分发与签名（Windows 不签名会出事）

| 项 | macOS | Windows |
|---|---|---|
| 签名 | Developer ID + Hardened Runtime + `notarytool` + `stapler` | Authenticode；个人开发者用 **Azure Trusted Signing** |
| 不签的后果 | Gatekeeper 拦截（用户可右键绕过） | SmartScreen "未知发布者"；**更狠的是 Defender 直接判 `Wacatac.C!ml` 并连 exe/快捷方式/卸载项一起删**（真实案例），带 llama.cpp/onnxruntime 二进制更容易命中 |
| 权限 | TCC 屏幕录制，签名变更可能失效 | 热键/托盘/截屏免 UAC |
| 模型分发 | 快档零体积；质量档**首启下载** + SHA256 + 断点续传（随包 +1 GB 会让公证和更新都变慢） | 同左；大 exe 更易误报，故更倾向首启下载 |

许可：GLM-OCR **MIT**（保留版权声明即可商用）、llama.cpp MIT、Tauri MIT/Apache-2.0；社区二次量化的 GGUF 要逐个核对 LICENSE。

### 9.3 Windows 落地前必须复测的 5 件事

本报告全部实测在 macOS/M5 上完成，Windows 侧是**同代码路径但未实测**。上线前至少补：

1. `Windows.Media.Ocr` 在同一批图上的 `content_acc` / `sym_recall` / `indent_exact`（本报告已给出 mac 的基准值，可直接对比）。
2. 语言包缺失时 `TryCreateFromUserProfileLanguages()` 返回 `null` 的降级路径是否顺畅。
3. `llama.cpp` 在 **Vulkan / 纯 CPU** 下的单图耗时（决定质量档是否要默认关闭）。
4. 深色终端 + 中英混排在 Windows 字体栈（Consolas/Cascadia Mono）下的表现。
5. Defender 对带 llama.cpp 的安装包是否误报（提前跑 WDSI 提交）。

## 附录：复现

```bash
# ── 环境（本机已验证，macOS）─────────────────────────────
uv venv .venv --python 3.12
uv pip install --python .venv/bin/python mlx-vlm rapidocr onnxruntime rapidfuzz
brew install llama.cpp

# ── 评测集 ─────────────────────────────────────────────
cd bench
python3 scenes.py                       # 7 场景 ×3 变体 + ground truth

# ── 快档（平台原生）────────────────────────────────────
swiftc -O tools/vision_ocr.swift -o /tmp/ocr_vision
/tmp/ocr_vision images/git-diff@2x.png > preds/vision-accurate.git-diff.2xpng.json

# ── 便携引擎（两端同一套）──────────────────────────────
.venv/bin/python tools/run_classic.py tesseract "images/*@2x.png"
.venv/bin/python tools/run_classic.py rapidocr  "images/*@2x.png"

# ── 质量档：llama.cpp + GLM-OCR Q8 GGUF（双平台同文件）──────
llama-server -m ~/models/GLM-OCR-GGUF/GLM-OCR-Q8_0.gguf \
  --mmproj ~/models/GLM-OCR-GGUF/mmproj-GLM-OCR-Q8_0.gguf \
  --host 127.0.0.1 --port 8099 -ngl 99 -c 8192
.venv/bin/python tools/run_server.py --tag llamacpp    # 走 /v1/chat/completions

# ── 打分与汇总 ─────────────────────────────────────────
.venv/bin/python tools/cer_eval.py truth/git-diff.txt \
  results/preds-eval/llamacpp.git-diff.2xpng.json
.venv/bin/python tools/aggregate.py 2xpng
```

**Windows 复现**：把 `brew install` 换成 llama.cpp 官方 Windows release（Vulkan/CUDA/CPU 版），
`vision_ocr` 换成 WinRT OCR 的等价 runner，其余（bench 图、真值、`cer_eval.py`、`aggregate.py`）
**完全不用改** —— 这正是 §5 把引擎和重建层解耦的目的。

产物：`bench/images/`（图）、`bench/truth/`（真值）、`bench/results/preds-eval/`（各引擎原始输出）、
`bench/results/results-2xpng.md`（汇总表）、`bench/tools/`（评测脚本）。

**参考**
- PaddleOCR-VL-1.6 <https://huggingface.co/PaddlePaddle/PaddleOCR-VL-1.6>
- GLM-OCR（MIT）<https://huggingface.co/zai-org/GLM-OCR> · 官方 GGUF <https://huggingface.co/ggml-org/GLM-OCR-GGUF>
- Umi-OCR <https://github.com/hiroi-sora/Umi-OCR>
- CRec（代码 OCR 可编译性）<https://doi.org/10.1016/j.softx.2026.102735>
- Apple Vision `RecognizeDocumentsRequest` <https://developer.apple.com/documentation/vision/recognizedocumentsrequest>
- Windows.Media.Ocr <https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr.ocrword>
- Windows AI TextRecognizer（NPU，增强档）<https://learn.microsoft.com/en-us/windows/ai/apis/text-recognition>
- mlx-vlm <https://github.com/Blaizzy/mlx-vlm> · llama.cpp <https://github.com/ggml-org/llama.cpp>
- Windows Terminal a11y 限制 <https://raw.githubusercontent.com/microsoft/terminal/main/doc/terminal-a11y-2023.md>
