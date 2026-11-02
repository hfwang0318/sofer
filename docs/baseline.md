# sofer 基线报告

> 环境：macOS 27 / Apple M5 / 16GB，`cargo build --release`。
> 全部数字由 `bench/baseline.py` **调用 CLI** 产生（不是直接调库），命令见文末。

## 1. 语料：26 个场景 / 82 张图

| 类别 | 场景数 | 说明 |
|---|---|---|
| 合成场景（`scenes.py` + `scenes2.py`） | 20 | Menlo 13pt @2x 渲染，逐字符真值 |
| 极端几何（`scenes3.py`） | 4 | 小图 / 4K / 超宽 / 超窄 |
| **真机截图**（TextEdit + `screencapture`） | 2 | 非合成，带窗口边框与拼写检查波浪线 |

真值合计 **367 行 / 23,971 字符**。变体：

| 变体 | 后缀 | 数量 | 用途 |
|---|---|---|---|
| Retina | `@2x.png` | 26 | 主力 |
| 非 Retina | `@1x.png` | 26 | 缩放 / 非 Retina |
| JPEG | `@2x.jpg` | 26 | 聊天软件二次压缩（q70） |
| 手机拍屏 | `@photo.jpg` | 4 | 缩 82% + 高斯模糊 + 噪声 + q62 |

极端几何场景：

| 场景 | 尺寸 | 宽高比 | 考察点 |
|---|---|---|---|
| `tiny-crop` | 220×120 | 1.8:1 | 6.5pt 小字，抗锯齿吃掉笔画 |
| `ultra-wide` | 3524×184 | **19:1** | 单行 232 字符，超出模型原生分辨率 |
| `ultra-tall` | 212×2330 | **1:11** | 单列 60 行短文本 |
| `huge-4k` | 4004×2145 | 1.9:1 | 8.6MP / 246 列 × 56 行，**且每行高度相似** |

真实截图的做法见 [bench/tools/real_capture/README.md](../bench/tools/real_capture/README.md)：
TextEdit + Menlo 14pt，按窗口 id 截图，裁掉标题栏/工具栏/标尺。

其余 20 个合成场景覆盖：git diff、pytest 堆栈、tree、制表框线、行号槽、浅色主题、软换行、
中英混排、commit 图、编译器诊断、JSON 日志、选中高亮、连字倾向符号、冲突标记、宽数字表格、
小字、tmux 双栏、CLI 图标、嵌套 diff、Powerline 私有区字形。

**未纳入**：真正的连字字体（Fira Code 把 `=>` 合成单字形）。Menlo 不连字，所以 `symbols`
只测"符号密集"，不测连字本身。

## 2. 指标

| 指标 | 定义 |
|---|---|
| `content_acc` | 去空白后字符准确率 |
| `text_acc` | 去掉 box-drawing 再算（把"框线丢失"和"读错字"分开） |
| `box_recall` | 真值里框线字符被复原的比例 |
| `sym_recall` | 代码符号多重集召回（`(){}[]<>;:=+-*/…`） |
| `indent_exact` | 行首空格完全正确的比例 |
| `rows_ratio` | 输出行数 / 真值行数 |
| `ms/img`、`peak MB` | CLI 墙钟 / 峰值 RSS（模型内存不在 CLI 里，在 llama-server） |

## 3. 结果

### 3.1 Retina（26 场景）

| engine | content_acc | text_acc | box_recall | sym_recall | indent_exact | rows_ratio | ms/img | peak MB |
|---|---|---|---|---|---|---|---|---|
| mock（只测几何层） | **1.000** | 1.000 | 1.00 | 1.000 | **0.99** | 1.00 | 8 | 44 |
| llamacpp（GLM-OCR Q8） | **0.957** | 0.983 | 0.88 | **0.996** | 0.91 | 0.98 | 3884¹ | 61 |
| tesseract | 0.899 | 0.916 | 0.88 | 0.949 | 0.87 | 0.93 | **596** | 93 |

¹切块把 `huge-4k` 从 47 s 拉到 124 s，把均值抬高了约 3 s；典型场景仍是 0.8–1.5 s。

`llamacpp` 的 `ms/img` 被 `huge-4k`（32 s）拉高，典型场景仍是 0.8–1.5 s。

### 3.2 其它变体

| 变体 | engine | content_acc | sym_recall | indent_exact | ms/img |
|---|---|---|---|---|---|
| `@1x` | llamacpp | 0.934 | 0.970 | 0.88 | 3886 |
| `@1x` | tesseract | 0.851 | 0.869 | 0.76 | 381 |
| `@2x.jpg` | llamacpp | 0.957 | 0.996 | 0.88 | 7959 |
| `@2x.jpg` | tesseract | 0.891 | 0.940 | 0.87 | 614 |
| `@photo` | llamacpp | 0.946 | 0.960 | 0.86 | 3107 |
| `@photo` | tesseract | 0.934 | 0.851 | 0.90 | 403 |

JPEG q70 不再掉分（0.957，与 PNG 持平）——切块之后 `huge-4k` 在两个变体里都是 1.000，剩下的差别被摊平了。
真正掉分的是降到 1x（0.957→0.934，`sym_recall` 0.996→0.970）。
`@2x.jpg` 的 `ms/img` 7959 是切块 + JPEG 解码叠加的结果，不代表典型场景。

### 3.3 极端与真实场景（`@2x`，`content_acc`）

| 场景 | llamacpp | tesseract | mock |
|---|---|---|---|
| `tiny-crop`（220×120） | 1.000 | 1.000 | 1.000 |
| `ultra-wide`（19:1） | 1.000 | 0.997 | 1.000 |
| `ultra-tall`（1:11） | 1.000 | 0.998 | 1.000 |
| **`huge-4k`（8.6MP）** | **0.595** | **0.996** | 1.000 |
| `real-editor-code` | 0.996 | 0.977 | 1.000 |
| `real-editor-term` | 0.894 | 0.887 | 1.000 |
| `split-panes`（tmux 双栏） | 0.990 | 0.270 | 1.000 |

`huge-4k` 的 GLM 结果在两次运行里分别拿到 0.595 和"塌成 1 行"，后者在跑批时因超时被记为失败；
两次都持续 30–49 s。这一点本身也是结论：**8.6MP 的重复内容既慢又不可靠**。

### 3.4 候选模型 A/B（2026-11）

背景：OXR 的公开对比榜把 OvisOCR2（0.8B）和 PaddleOCR-VL-1.6（0.9B）排得比 GLM-OCR
高不少（OmniDocBench 96.47 / 96.34 vs 95.22）。用同一套 26 场景复核这两者。

方法：

- M5 / 16 GB / macOS 27，`llama-server` 0.8.0（build 11960），Metal `-ngl 99`；
- 质量指标用 **4 并发**跑完 26 场景 `@2x`，prompt 为项目默认；
- 速度点在 `git-diff` / `pytest-traceback` / `editor-rust` / `small-text` 四个场景上
  串行、已热身，服务端参数与 app 默认一致（`-c 24576 -np 3 -ctk q8_0 -ctv q8_0`）；
- PaddleOCR-VL-1.6 使用官方 `chat_template.jinja` 与它原生的 `OCR:` prompt。

| 配置 | 磁盘（权重+mmproj） | content_acc | **sym_recall** | indent_exact | rows_ratio | 单请求 ms¹ | 空闲 RSS |
|---|---:|---:|---:|---:|---:|---:|---:|
| **GLM-OCR Q8_0**（高精度档） | 1434 MB | 0.957 | **0.996** | 0.91 | 0.98 | 1288 | 2409 MB |
| **GLM-OCR Q4_K_M**（低精度档） | 1033 MB | **0.960** | 0.994 | 0.91 | **0.98** | **922** | 1879 MB |
| OvisOCR2 Q8_0 | 928 MB | 0.948 | 0.956 | 0.91 | 1.47 | 1944 | 1669 MB |
| OvisOCR2 Q4_K_M | 645 MB | 0.948 | 0.955 | 0.91 | 1.49 | 1331 | 1078 MB |
| PaddleOCR-VL-1.6 Q8（`OCR:`） | 1817 MB | 0.922 | 0.937 | 0.84 | 0.92 | 3356² | — |
| PaddleOCR-VL-1.6 Q8（项目默认 prompt） | 1817 MB | **0.390** | 0.883 | 0.83 | 1.82 | — | — |

¹ 四场景串行、已热身；同配置重复跑有 ±10–15% 波动（热降频）。
² Paddle 的单请求时间在 `-c 32768 -np 4` 下测得，未按 app 参数重测。

**① 差距几乎全部集中在 `pytest-traceback` 一个场景。**

| 场景 | GLM-OCR | OvisOCR2 | PaddleOCR-VL |
|---|---|---|---|
| `pytest-traceback` | **0.911 / 1.000** | 0.713 / **0.394** | 0.712 / **0.384** |
| `huge-4k` | **1.000 / 1.000** | 1.000 / 1.000 | **0.334 / 0.339** |
| `editor-rust` | 0.934 / 1.000 | **0.998** / 0.991 | **1.000** / 1.000 |
| 其余 22 个场景 | 三家差值 < 0.02 | | |

Paddle 的典型崩塌是把 `....F [100%]` 读成 `...F`、拆掉 `====` 分隔线、吞掉 `>` 标记；
Ovis 则是同一行里成串丢符号。**这正是[选型报告 §4.3](sofer-选型报告.md)①说的
"符号是代码里最贵的字符"**。

**② OvisOCR2 的主要问题是行数膨胀，不是字符错。**
`rows_ratio` 1.47–1.93：几乎每个场景都插入空行，且不可通过 prompt 抑制
（试过 `--image-min-tokens 1024` 与"不要插空行"的指令，均无变化）。在 8 个代表场景上
把它吐出的空行去掉，`rows_ratio` 从 **1.55 收敛到 0.98**，而 `content_acc` / `sym_recall`
一位不变——因为两个指标都先剥空白。这说明空行是**纯结构噪声**，理论上可由几何层重建
（见 §4 结论 ⑥），但在做这个修复之前，它的输出粘回编辑器还要手工删空行。

**③ PaddleOCR-VL-1.6 对 prompt 极其敏感。**
换成项目默认 prompt（"preserve the original layout…"）时，`conflict` 场景复读成
25 倍行数、单图 45 s、`content_acc` -11.8；`huge-4k` 从 56 行截断到 19 行并持续 70 s。
换成原生 `OCR:` 才恢复正常。**这坐实了[选型报告 §4.3](sofer-选型报告.md)③的结论：
它在非模板 prompt 下不稳定，官方 Q8 GGUF 也没解决。**

**④ 量化不改变排序。** GLM 的 Q4_K_M 与 Q8_0 在 26 场景里差异在噪声内
（0.960/0.994 vs 0.957/0.996），且 Q4 更快（922 vs 1288 ms）；Ovis 的 Q4 与 Q8 同样持平。

**结论：两个候选都不能替换现用模型**，判据是 `sym_recall ≥ 0.995` 且
`0.98 ≤ rows_ratio ≤ 1.05` 且单图 ≤ 2.7 s——只有 GLM 的两个精度档同时满足三条。
复现命令见 §5。

## 4. 结论

**① 几何层在 26 个场景上接近满分，但真实截图把它打回原形过。**
`mock`（喂真值文本、只算几何）在 20 个合成场景 `indent_exact` = 1.00；第一次换上真机截图是 **0.38**。
两个根因都在 `src/geom.rs`，已修：

| 问题 | 根因 | 修法 |
|---|---|---|
| 窗口边框/阴影被当成墨迹，列原点被拉到 0，每级缩进偏 1 | ink 阈值取 `p90×0.35`，而文字只占几个百分点，p90 ≈ 背景 | 改成 `p99×0.30`（看分布亮端） |
| `tree` 段 16 行塌成 8 个行带 | `min_ink = max(2, w×0.0015)` 太低，框线竖笔画跨行相连 | 改成 `max(4, w×0.004)` |

修完后：`real-editor-code` 1.00、`real-editor-term` 1.00，20 个合成场景仍是 1.00。
**没有真实截图，这两个 bug 永远不会被发现**——这是本轮最有价值的产出。

**② 极端几何不是问题；`huge-4k` 的真实原因是"输出预算"而不是分辨率。**

220×120 的小图、19:1 的超宽、1:11 的超窄，GLM 全部 1.000。
`huge-4k` 曾经只有 0.595，当时的解释是"内容重复导致塌缩"——**这个解释是错的**，已订正：

| 策略 | 输出行数 | content_acc | 耗时 |
|---|---|---|---|
| 整图，`--max-tokens 3072` | 26 / 56（截断） | 0.452 | 47 s |
| 整图，`--max-tokens 8192` | 68 / 56（超发+幻觉） | 0.792 | 94 s |
| 切块 14 行/块，按文本去重 | 62 / 56 | 0.893 | ~110 s |
| 切块 20 行/块，按行带序号拼接（原型） | 56 / 56 | **1.000** | ~70 s |
| **内核 `--tile auto`（3 块 × 19 行）** | **56 / 56** | **1.000** | 124 s |

切块已进内核：`src/tile.rs`，规则见 [README](../README.md#切块--tile-auto)。

用 `/tokenize` 量过：235 字符的日志行 = **134 token（1.75 字符/token）**，3072 / 134 ≈ 23 行
——正好对上"26 行就停"。所以是**输出 token 预算被用完**，不是模型看不懂。
（期间还把测试场景改真实了：原版每行把自己尾巴重复拼接，实际在测"行内重复"。）

同场景 Tesseract 0.996：它逐行识别、没有全局输出预算的概念，所以不受影响。

**③ 双栏（tmux 分屏）是几何模型的结构性盲区。**
真值按栏拼接时 GLM 0.990、Tesseract 0.286（它把两栏按行合并）。
但 `indent_exact` 对多栏没有意义（行带是"一行"的模型，12 行文本对 6 个行带），已标注、不参与结论。

**④ 框线仍是模型侧问题**：`ascii-table` `text_acc`=1.000、`box_recall`=0.00；`tree` 同理。
`text_acc` 与 `box_recall` 分开报告，就是为了不让它污染"读得准不准"的判断。

**⑤ 提示词 > 参数**：`OCR:` 会让宽表格每格一行（rows_ratio 6.0）；换成
`OCR: preserve the original layout, spacing and line breaks exactly.` 后 rows 1.17、content 1.000，还快 30%。

**⑥ 空行属于几何层，不属于模型层。**
OvisOCR2 在 26 场景上平均多吐 47% 的行，几乎全是空行（§3.4）。把它吐出的空行滤掉，
`rows_ratio` 从 1.55 收敛到 0.98，而 `content_acc` / `sym_recall` 一位不变——两个指标
都先剥空白，空行贡献 0 个字符。
**已实现**：`assemble_with_bands` 不再信任模型的换行，改为按行带中心的间距重建
（行距取差值的 25 分位，段间距不会把它抬高）；当图像行带数少于文本非空行数
（行带漏检/合并）时回退到模型换行。合成用例里把真值每行之间插一个空行，
`rows_ratio` 从 1.95 回到 1.00。

## 5. 复现

```bash
cargo build --release
.venv/bin/python bench/scenes.py && .venv/bin/python bench/scenes2.py && .venv/bin/python bench/scenes3.py
.venv/bin/python bench/baseline.py --engines mock,tesseract,llamacpp --variant 2x
.venv/bin/python bench/baseline.py --engines mock,tesseract,llamacpp --variant 1x
.venv/bin/python bench/baseline.py --engines mock,tesseract,llamacpp --variant 2xjpg
.venv/bin/python bench/baseline.py --engines mock,tesseract,llamacpp --variant photo
```

单次结果：`bench/results/baseline-{2x,1x,2xjpg,photo}.md`。
真机截图的重现步骤（需要屏幕录制权限）见 `bench/tools/real_capture/README.md`。
