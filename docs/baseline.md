# sofer 基线报告

> 环境：macOS 27 / Apple M5 / 16GB，`cargo build --release`。
> 全部数字由 `bench/baseline.py` **调用 CLI** 产生（不是直接调库），命令见文末。

## 1. 语料：40 个场景 / 125 张图

| 类别 | 场景数 | 说明 |
|---|---|---|
| 合成场景（`scenes.py` + `scenes2.py`） | 20 | Menlo 13pt @2x 渲染，逐字符真值 |
| 极端几何（`scenes3.py`） | 4 | 小图 / 4K / 超宽 / 超窄 |
| **真机截图**（TextEdit + `screencapture`） | 2 | 非合成，带窗口边框与拼写检查波浪线 |
| 定向回归（`scenes4.py`） | 14 | 表格 / 树 / 装饰线 / 行号槽。**这是按实现规则写的定向回归集，不是 hold-out** |

真值合计 **546 行 / 26,935 字符**。变体：

| 变体 | 后缀 | 数量 | 用途 |
|---|---|---|---|
| Retina | `@2x.png` | 40 | 主力 |
| 非 Retina | `@1x.png` | 40 | 缩放 / 非 Retina |
| JPEG | `@2x.jpg` | 40 | 聊天软件二次压缩（q70） |
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

## 5. 图片预处理（v1.1）

进模型前的处理链现在是：EXIF 方向 → 透明合成白底 → 解码像素上限（默认 5000 万）
→ 内容框裁边（裁掉 ≥35% 面积才执行）→ 行高 <16 px 时 2× 放大。
几何层始终在归一化后的原图坐标里工作；`preprocess::Transform` 把切块映射到模型副本。
原图字节只有在上述步骤都没改像素时才原样转发。

`bench/baseline.py --extra=<flag>` 可以逐项关闭做消融。下表为 26 场景、Q8_0、
`temperature=0` 的结果（content / text / sym / indent / rows）。

| 变体 | 预处理 | content | text | sym | indent | rows | ms/img |
|---|---|---:|---:|---:|---:|---:|---:|
| 2x | 旧流程 | 0.957 | 0.983 | 0.996 | 0.91 | 0.98 | 5760 |
| 2x | 裁边阈值 2%（首版） | 0.955 | 0.972 | 0.994 | 0.91 | 0.98 | 5481 |
| 2x | 关闭裁边 | 0.957 | 0.983 | 0.996 | 0.91 | 0.98 | 3384 |
| 2x | **裁边阈值 35%（默认）** | **0.957** | **0.983** | **0.996** | **0.91** | **0.98** | **3265** |
| 1x | 旧流程 | 0.934 | 0.959 | 0.970 | 0.88 | 1.01 | 3886 |
| 1x | **2× 放大（默认）** | **0.955** | **0.980** | **0.995** | **0.91** | **0.98** | **5642** |
| 1x | 不放大 | 0.933 | 0.957 | 0.968 | 0.89 | 1.01 | 2994 |
| photo | 旧流程 | 0.946 | 0.946 | 0.960 | 0.86 | 1.05 | 3107 |
| photo | 默认 | 0.946 | 0.946 | 0.960 | 0.86 | 1.05 | 2303 |

结论：

- **小裁边有害**：2% 阈值会把 `tree`、`light-ambiguous`、`pytest-traceback` 等图像裁掉
  2–20% 的边距，模型对画面尺寸本身敏感，`text_acc` 掉 1.1 pp。把阈值提到 35%
  后，只有内容占比低于 65% 的大边距截图（本语料是 `real-editor-code/term`）才会
  重编码，26 场景指标与旧流程逐项相同，`ms` 因这两张图变小而略降。
- **1× 放大是明确收益**：`content +2.1 pp`、`sym +2.5 pp`、`indent +3 pp`，基本追平
  原生 `@2x`。改善分布很广：`tiny-crop +29.2 pp`、`powerline +12.6 pp`、
  `huge-4k +4.3 pp`、`pytest-traceback +3.3 pp`；代价是同批次 `ms` +88%、峰值内存
  +110 MB（放大图与几何中间量同时驻留）。
- **拍屏不受影响**：`@photo` 的行高高于 16 px，裁剪比例也低于阈值，走原图透传。
- `ms/img` 受 llama-server 冷热影响很大（同一代码两次可差 40%），只能比较同一批次的
  消融行；质量指标在 `temperature=0` 下可重复。

1× 放大后的 tile 预算仍按原图行带计算；若一张 1× 图放大后单块面积超过
`MAX_TILE_AREA_PX`，编码器会在内部再降采样一次。当前语料没有触发这个组合，
加入更大的 1× 4K 场景时需要重新量。

### 5.1 大图降采样与无重叠切块（v1.2）

大图慢的两个固定成本是**视觉编码**（与像素面积成正比）和**解码**（与输出
token 数成正比）。在不降精度的前提下能动的只有两处：

| 措施 | 条件 | 40 场景指标 | `huge-4k` |
|---|---|---:|---:|
| 关闭降采样（对照） | — | 0.956 / 0.975 / 0.988 / 0.88 / 0.99 | 61–69 s |
| **大图降采样（默认）** | > 4 M px 且行高 > 24 px → 24 px | 0.956 / 0.975 / 0.988 / 0.88 / 0.99 | **41–43 s** |
| 缩到 22 px（**不采用**） | 同上 | 0.956 / 0.975 / 0.988 / 0.88 / 0.99 | 49 s |
| 全局缩到 22 px（**不采用**） | 所有 2× 图 | 0.954 / 0.972 / 0.988 / 0.89 / 0.99 | — |
| 无重叠切块 | 追加到上面 | 0.956 / 0.975 / 0.988 / 0.88 / 0.99 | 含在上行 |

**阈值是相对模型算出来的，不是硬编码像素。** `mmproj` 元数据是
`patch_size=14` + `mm.patch_merger`（2×2），所以

```text
token_px = patch_embd.dims[0] × patch_merger.dims[0]     # 14 × 2 = 28
```

实测 2.11 M px 的图像正好 2676 个 token（≈789 px/token），与 28×28 吻合。
`src/gguf.rs` 只读 GGUF 头部的张量形状就能推出这个值，不需要跑推理；桌面端
从 mmproj 自动读取，CLI 用 `--vision-token-px` 覆盖。所有阈值都是它的比例：

| 阈值 | 比例 | GLM-OCR（token 28 px） |
|---|---:|---:|
| 升采样（2×） | < 0.57 | < 16 px |
| 降采样触发与目标 | > 0.86 → 0.86 | > 24.1 px |
| 缩放幅度上限 | 0.9× | 0.9× |

换一个 `patch=16、无 merger` 的模型，阈值会自动变成 9.1 / 13.8 px，而不是
沿用 GLM-OCR 的绝对值。`0.57 / 0.86 / 0.9` 这三个比例仍是本语料标定的先验，
不是定理；真正模型无关的只有 `token_px` 和"放大不会创造信息"。

`@2x` 截图是 13 px 字体：墨迹高 26 px、字符宽约 16 px，即字符宽 ≈0.57 个
token。这一档在所有扫描尺度中精度最高：

| 场景 | 0.70× | 0.77× | 0.85× | **1.00×（标准）** | 1.15× |
|---|---:|---:|---:|---:|---:|
| pytest-traceback | 0.883 | 0.888 | 0.839 | **0.951** | 0.908 |
| tree | 1.000 | 0.989 | 1.000 | **1.000** | 1.000 |
| psql-table | 0.895 | 0.941 | 0.961 | **0.961** | 0.895 |
| git-diff | 0.997 | 0.997 | 0.996 | **0.996** | 0.986 |
| editor-rust | 0.934 | 0.934 | 0.934 | **0.934** | 0.934 |
| log-columns | 0.960 | 1.000 | 1.000 | **1.000** | 0.920 |

结论：

- **放大没有收益**：1.15× 在 6 个场景里 3 个变差、3 个持平。模型不会因为
  多给 token 变准，插值反而引入伪影。所以「小图升采样」只在像素密度真的不足
  时成立——`ink < 16 px`（1× 截图是 13 px，低于半个 token 高度）时 2×
  放大有实测收益（`content +2.1 pp`），这也是现有阈值的由来，不需要连续化。
- **缩小有门槛**：≤0.9× 基本无损（把代码图拼成 4.4–4.7 M px 的大画布后缩到
  0.9×，`pytest-traceback` 0.949 vs 0.950、`editor-rust` 0.932 vs 0.932）；
  0.85× 已经让 `pytest-traceback` 掉 11 pp，0.77× 掉 6.3 pp。注意曲线不单调，
  所以按幅度保守取值，而不是按面积。
- **面积条件只是成本代理**：它回答"值不值得冒险"（小图的视觉编码本来就不贵），
  不是精度判据。真正的安全边界是**缩放幅度 ≤0.9×**。
- **`huge-4k` 有冗余**：3940×2145、ink 26 px 的长行日志缩到 0.75× 仍 1.00；
  但同尺寸的代码图没有这种冗余，所以目标定在 0.86 token（24.1 px，实际
  0.926×），而不是更激进的 22 px（0.846×）。
- **切块不再重叠**：切点就是行带墨迹边界，每块只发自己的行。原先每块上下各留
  1 行、3 块多生成 4 行（约 7% 输出 token）；去掉后 `stitch` 变成顺序拼接，
  只按"去空白后相等"折叠跨切点的重复行。
- 两者都只改模型副本：`geom` 仍在原图坐标里工作，`--stats` 的
  `preprocess.scale` 会显示 0.9。

**两个权重档都跑过全量**（40 场景，`--variant 2x`，同一批代码）：

| 模型 | content_acc | text_acc | box_recall | sym_recall | indent_exact | rows_ratio | ms/img |
|---|---:|---:|---:|---:|---:|---:|---:|
| Q8_0（高精度） | 0.956 | 0.975 | 0.94 | 0.988 | 0.88 | 0.99 | 3279 |
| Q4_K_M（桌面默认） | 0.953 | 0.973 | 0.94 | 0.981 | 0.89 | 0.99 | 2754 |

- **降采样开关在两个模型上都是 40/40 场景逐项相同**（`--no-downscale-large`
  对照），所以这项改动没有精度代价。
- **切块无回归**：`tiling-table` 切 2 块与 `--tile off` 的结果完全一致
  （0.808 / 44 行），说明去掉重叠行没有丢边界行。
- 与上一轮 26 场景的 `improve-final-2x` 基线逐项相同（含 `huge-4k` 1.000）。
- Q4 的固有差距集中在 `tree`（0.949 vs 1.000）和 `psql-table`
  （0.895 vs 0.961），平均 `content -0.32 pp`；这与上一轮 Q4 复验记录的
  `tree 0.949` 一字不差，是模型差异，不是本轮改动引入的。8 个有差异的场景里
  还有 3 个 Q4 略好（`rules-mixed +0.3 pp`、`nerd-symbols +0.7 pp`）。

> 注意：`geom.line_height` 是**墨迹高度**（cap height + descenders），不是行距。
> 13 px 字体的 2× 截图是 26 px，而不是行距的 37 px；按行距设阈值会误伤全部
> 普通截图。拼接大图会让这个中位数略降（实测 24.4 px），所以阈值和幅度都要
> 留余量。

同一轮还排除了这些「看起来该快」的选项（M5，`huge-4k@2x`，同批次）：

| 尝试 | 结果 |
|---|---|
| Q4_K_M 代替 Q8_0 | 3 块 decode 72 tok/s vs 86，**更慢**；首次大图 100 s vs 37 s |
| `-fa on` + f16 KV | 热态 59 s vs 36 s，更慢 |
| mmproj 换 Q5_K_M | 38 s vs 37 s，无变化 |
| Q3 作 draft 投机解码 | 接受率 99.8%，decode 80 tok/s vs 119，**更慢** |
| ngram 投机（`--spec-type ngram-simple`） | 3 路 55 tok/s，更慢 |
| `-np 5` + 5 块 | 同图重跑 24 s（-17%），但首次新图 47 s vs 37 s，**更慢** |

根因：0.9B 模型本身的 decode 已经很快，投机/多槽批处理摊不掉额外开销；
图像编码在 M5 GPU 上只比 CPU 快 1.54×（8.3 s vs 12.7 s / 2.1 M px），
说明 llama.cpp 的视觉编码 kernel 没能吃满 GPU，这部分目前只能靠减少像素。

## 6. 版面重建（模型不变，唯像素计算）

这一轮没有换模型、没有改 prompt、没有增加请求次数；提升全部来自"模型给文字、
像素给结构"的边界内处理。

| 场景 | 改进前 `content_acc` | 改进后 | 主因 |
|---|---:|---:|---|
| `ascii-table` | 0.581 | **0.966** | 模型丢掉整行 `┌──┬──┐`，185/441 个字符是框线 |
| `tree` | 0.767 | **1.000** | `├──` 被写成 `|` 或直接省略 |
| `real-editor-term` | 0.894 | **0.953** | 10 行 tree 被合并成少数行带 + 树形丢失 |
| `pytest-traceback` | 0.911 | **0.951** | `====`/`____`/`----` 长度被截短 |
| 其余 22 个场景 | — | 不变 | — |
| **26 场景均值** | **0.957** | **0.984** | `text_acc` 0.983→0.987，`box_recall` 0.88→0.99，`rows_ratio` 0.98→1.00 |

五项机制：

1. **规则行带跳过**（`assemble::align_by_width`）：文本行数少于行带数时，
   `column_runs` 只有一个贯穿全宽 run 的行带视为横线，允许 DP 以近乎零代价
   跳过。此前规则行会把后面所有行整体错位一行。
2. **表格框线重建**（`boxdraw::rebuild_rule_rows`）：从检测到的水平/垂直笔画
   生成 Unicode `┌┬┐├┼┤└┴┘`，只在数据行带与文本行一一对应且连续时插入；
   规则行带已有文本（引擎/真值自己画了框线）时不动，保证幂等。
3. **树形重建默认开启**（`boxdraw::rewrite_tree_line`）：分支笔画经 band 中部
   约束确认后生成 `├──`/`└──`/`│`；已经是正确 Unicode 前缀的行不改写。
4. **合并行带再分割**（`geom::band_rows_split`）：高度超过中位数 1.5 倍的行带，
   在其自身墨迹谷值（低于峰值 25% 且连续 ≥2 行）处切分。
5. **装饰线长度重建**（`assemble::resize_decoration_tokens`）：四个以上同一
   装饰字符的 token，长度按 `墨迹宽度 / advance` 重算；`--- a/file`（3 个连字符
   的 diff 头）和 `++++----`（diff stat 混合条）不处理。
6. **像素行号槽**（`assemble::detect_pixel_gutter`）：模型省略行号时，用固定的
   窄墨迹 run + 间隔识别行号列，从第二个 run 恢复代码缩进。行号字符本身没有
   可靠的纯计算来源，仍保持缺失。

边界与风险：

- `--box-art` 默认开启，`--no-box-art` 可关闭。所有重建都以"笔画确认"为前提；
  mock（输入真值文本）在 26 场景上仍为 1.000，说明幂等。
- **只在原生分辨率重建**：1× 截图被 2× 放大送模型时，几何仍在原图坐标，代码里的
  `|` 与分支笔画同宽，笔画检测会误判（`real-editor-code@1x` 0.996 → 0.643）。
  pipeline 因此在 `transform.scale != 1.0` 时跳过 `boxdraw`。
- **照片上跳过**：背景噪声（无关像素在众数灰度附近的 σ）> 1.8 视为拍屏，不重建。
  实测 PNG 截图 0.2、JPEG 重编码 1.1、`@photo` 2.1；跳过前 `git-diff@photo`
  0.997 → 0.965。
- 合并行带分割只作用于异常高的行带，普通几何逐项不变。
- 变体结果：2× 0.957 → **0.984**；1× 0.955 → **0.963**；`@photo` 0.946 → **0.951**。
- `powerline`（0.915）的私用区分隔符、`editor-rust`（0.934）的行号字符、
  `ascii-table` 数据行缺失的 15 个 `│`（0.966 的剩余差）属于当前方法不处理的
  硬缺口，不在本轮目标内。

复现：

```bash
cargo build --release
.venv/bin/python bench/baseline.py --engines mock,llamacpp --variant 2x \
  --md bench/results/improve-final-2x.md
```

## 7. 扩展评测与稳定性（v1.3）

2026-11 把语料从 26 个场景扩到 40 个，并新增 `bench/compare.py` 做配对比较。
新增的 14 个场景（`scenes4.py`）是**定向回归集**：它们按本轮实现支持的形态编写，
与 Dev-26 共享渲染器和字体，**不是 hold-out**。

### 7.1 分层结果（2× PNG，Q8_0，`temperature=0`）

| 集合 | 场景数 | before | after | Δ | 配对 bootstrap 95% CI | 改善 / 持平 / 回退 | 符号检验 |
|---|---:|---:|---:|---:|---|---|---:|
| Dev-26 | 26 | 0.9569 | 0.9845 | **+2.76 pp** | [+0.15, +6.46] pp | 4 / 22 / 0 | p=0.125 |
| Targeted-14 | 14 | 0.8024 | 0.9031 | **+10.06 pp** | [+3.20, +17.69] pp | 6 / 8 / 0 | p=0.031 |
| All-40（描述性） | 40 | 0.9028 | 0.9560 | **+5.31 pp** | [+2.15, +9.04] pp | 10 / 30 / 0 | p=0.002 |

Targeted-14 的提升集中在结构场景：`photo-table` +41.0、`deep-tree` +31.5、
`light-tree` +29.2、`tree-gutter` +23.8、`rules-mixed` +9.8、`tiling-table` +5.6。
`nested-table`（模型把 9 行表格压成 6 行）、`light-editor`（模型省略行号）、
`md-doc`（模型把 Markdown 分隔行改成语义写法）与 before 完全相同，属于模型
能力上限，不是本轮规则能修复的范围。

### 7.2 1× 与 mock

| 检查 | 结果 |
|---|---|
| 1× Targeted-14 | 0.8014 → 0.8084（+0.70 pp），1 改善 / 13 持平 / 0 回退。只有不依赖 `boxdraw` 的装饰线重建生效；1× 放大路径按设计跳过笔画重建 |
| mock 40 场景（喂真值文本） | `content_acc` 1.000，`indent_exact` 1.00，`box_recall` 1.00，说明重建对已正确的文本幂等 |
| 确定性 | `ascii-table` / `tree` / `yaml-config` / `tiling-table` 各重复 3 次，输出 SHA-256 完全相同 |

### 7.3 模型精度：Q8 vs 桌面默认 Q4_K_M

上面的主结果是 Q8_0。桌面端默认发布的是 Q4_K_M（共用同一个 Q8 mmproj），
所以又用 Q4 服务（端口 18100）对 8 个关键场景做了 before/after：

| 场景 | before | after | Δ |
|---|---:|---:|---:|
| `photo-table` | 0.5300 | 0.9400 | +0.4100 |
| `ascii-table` | 0.5810 | 0.9660 | +0.3850 |
| `deep-tree` | 0.6640 | 1.0000 | +0.3360 |
| `rules-mixed` | 0.7140 | 0.8010 | +0.0870 |
| `tree` | 0.8690 | 0.9490 | +0.0800 |
| `real-editor-term` | 0.8940 | 0.9530 | +0.0590 |
| `tiling-table` | 0.7520 | 0.8080 | +0.0560 |
| `nested-table` | 0.4780 | 0.4780 | 0 |
| **均值** | **0.6853** | **0.8619** | **+17.66 pp**（CI [+7.56, +29.40] pp，7 改善 / 0 回退） |

Q4 的起点更低（`tree` 0.869 vs Q8 0.767 的两端都不同），重建后的绝对分也略低
（`tree` 0.949 vs Q8 1.000），但 **8 个场景 0 回退**。因此 `box-art` 不需要按
模型精度分别开关；两档默认都开启。

又用桌面端的实际服务参数复跑了一遍：
`-c 33024 -np 3 -ctk q8_0 -ctv q8_0 -cram 0 --sleep-idle-seconds 300`
（`jobs=3`、`max_tokens=3072`，即 `total_ctx = 3 × 11008`）。8 个场景的
`content_acc` 与未开 KV 量化的 Q4 **逐项完全相同**，0 回退。也就是说这批场景
上 KV q8 没有翻转任何 token；结论不需要限定为“未量化的 Q4”。

### 7.4 本轮发现并修复的两个回归

- `tree-gutter`：行号 + 树形组合时，`rewrite_tree_line` 只检查行首是否有
  `├──`，没有先把行号拆出来，输出 `├── 3 ├── cli` 的双重前缀。改为先分离
  行号、右对齐到树列，再生成前缀。mock 从 0.762 回到 1.000。
- `yaml-config`：行间一个 3×3 px 的孤立墨点被当成行带，导致 13 行文本对应
  14 个行带，后续所有行整体错位一行。`geom` 现在丢弃高度 <4 px 的行带。
  mock 从 `indent_exact=0.54` 回到 1.000。

### 7.5 结论边界

- Dev-26 与 Targeted-14 上均**未观察到回退**；定向修复成立。
- Targeted-14 是定向回归集，**不能**用它声称“对未见文档泛化稳定”。
- 仍未覆盖的分布外形态：真实拍屏（透视、摩尔纹、反光；现有 `@photo` 是同
  一张平面渲染的合成退化）、比例字体（HTML/PDF/Office；语料全部是 Menlo
  等宽）、CJK 表格/树布局、表格拒绝路径（合并单元格、缺行、被裁断）、
  字体/字号/DPI 变化。
- 要声称泛化稳定，需冻结 after 二进制后，用独立新场景或新的真机截图再跑
  一轮，并保持场景与阈值不再改动。

### 7.6 复现

```bash
# 生成 40 个场景
.venv/bin/python bench/scenes.py && .venv/bin/python bench/scenes2.py \
  && .venv/bin/python bench/scenes3.py && .venv/bin/python bench/scenes4.py

# after（当前工作树）与 before（干净 worktree）
cargo build --release
git worktree add /tmp/sofer-before 4c1c83e
cargo build --release --manifest-path /tmp/sofer-before/Cargo.toml

.venv/bin/python bench/baseline.py --engines llamacpp --variant 2x --md /tmp/all-after.md
.venv/bin/python bench/baseline.py --engines llamacpp --variant 2x \
  --cli /tmp/sofer-before/target/release/sofer --md /tmp/all-before.md

# 配对比较：分层跑三次
.venv/bin/python bench/compare.py /tmp/all-before.log /tmp/all-after.log
```

`baseline.py` 的每行输出是 `scene acc=... indent=... rows=...`，
`compare.py` 解析两份日志，输出逐场景 Δ、均值差、bootstrap CI 和符号检验。

## 8. 复现

```bash
cargo build --release
.venv/bin/python bench/scenes.py && .venv/bin/python bench/scenes2.py && .venv/bin/python bench/scenes3.py && .venv/bin/python bench/scenes4.py
.venv/bin/python bench/baseline.py --engines mock,tesseract,llamacpp --variant 2x
.venv/bin/python bench/baseline.py --engines mock,tesseract,llamacpp --variant 1x
.venv/bin/python bench/baseline.py --engines mock,tesseract,llamacpp --variant 2xjpg
.venv/bin/python bench/baseline.py --engines mock,tesseract,llamacpp --variant photo
```

单次结果：`bench/results/baseline-{2x,1x,2xjpg,photo}.md`。
真机截图的重现步骤（需要屏幕录制权限）见 `bench/tools/real_capture/README.md`。
