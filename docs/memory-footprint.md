# 常驻内存：实测、量化与运行时选型

> 面向“这个本地 OCR 工具为什么常驻 2 GB+、能不能压下去”的一次可复现调查。
> 结论先说：**2 GB 不是运行时框架的开销，而是 32k 上下文的 KV cache**；换 Ollama /
> LM Studio / vLLM 不会更低，真正有效的是 llama.cpp 自己的三个开关——空闲休眠、
> KV cache 量化、按并发裁剪上下文。权重换 4 bit 能省磁盘和一部分 RSS，但在 macOS
> 上对“活动监视器里的内存”帮助很小，而且低分辨率截图会掉点。

实测环境：macOS 27 / Apple M5 / 16 GB；`llama.cpp b12018`（与 `app/` 内置运行时同版本）；
模型 `GLM-OCR-Q8_0.gguf` + `mmproj-GLM-OCR-Q8_0.gguf`（官方 GGUF，MIT）。
精度口径：25 个 `@2x` 合成场景（不含 `huge-4k`，它单列）；`huge-4k` 用 3 块切块。
内存口径：`ps` 的 RSS 与 `vmmap -summary` 的 **Physical footprint** 同时记录。

---

## 0. 结论先行

| 问题 | 结论 |
|---|---|
| 为什么常驻 2 GB+？ | `-c 32768` 的 KV cache 就是 **2 GiB**（64 KiB/token），加 Metal 缓冲/权重页约 0.6 GB |
| 换热门部署项目能省吗？ | **不能**。Ollama/LM Studio/Jan/GPT4All/LocalAI/llamafile 都是 llama.cpp 的壳；vLLM/SGLang 是 GPU 预分配，更重 |
| 空闲时能降到多少？ | `--sleep-idle-seconds`：**3.3 GB RSS → 105 MB**，`/health` 仍 200；下次请求唤醒 +0.2–0.7 s |
| KV cache 量化？ | `-ctk q8_0 -ctv q8_0`：32k 下 footprint **2.6 G → 1.7 G**，25 场景精度**逐位相同**；macOS/Metal 速度基本不变，**CPU 上约 2× 慢** |
| 权重 4 bit？ | Q4_K_M 在 `@2x` 基本无损（内容 0.9552→0.9585），`@1x` 掉 0.5 pp（0.9341→0.9287），4K 切块仍 56/56；app 低精度包直接用 HF `mradermacher` 的 Q4_K_M（24/25 场景与自量化逐位一致）。Q5_K_M 的 `@2x` 更接近现状，但 `@1x` 并不比 Q4 稳。macOS footprint 几乎不变，磁盘 950→639/549 MB，RSS 约 -200~400 MB |
| mmproj 也要量化吗？ | **不要**。Q5_K_M 把 `indent_exact` 从 0.912 打到 0.862、`rows_ratio` 从 0.98 打到 1.18 |
| 最推荐的一行？ | `-ctk q8_0 -ctv q8_0 -cram 0 --sleep-idle-seconds 300`（macOS/GPU）；Windows CPU 只加 `--sleep-idle-seconds` 和 `-cram 0` |

---

## 1. 口径：RSS 和“活动监视器内存”不是一回事

模型权重是 **mmap 的只读文件页**，属于 clean page：RSS 会算它，但 macOS 的
Physical footprint（活动监视器“内存”列）不算，因为内核可以直接丢弃。KV cache 和
Metal compute buffer 是 dirty/shared memory，两者都算。所以同一个进程经常出现
“RSS 3.3 GB、footprint 2.6 GB”。

本文同时给两个数；**判断“用户看到多少”看 footprint，判断“占了多少物理页”看 RSS**。
80 ms 采样峰值用 `ps`，footprint 用 `vmmap -summary`。

关键公式（GLM-OCR：16 层 × 8 个 KV head × head_dim 128，f16 KV）：

```
KV/token = 2(K,V) × 16层 × 8头 × 128维 × 2 Byte = 65,536 B = 64 KiB
c=32768  → 2,097,152 KiB = 2 GiB
```

实测从 `-c 4096` 到 `-c 32768` 的 footprint 增量是 1.73 GiB / 28,672 token ≈ 63.3 KiB/token，
与公式一致。

---

## 2. 当前配置的实测

启动参数与 `app/src-tauri/src/engine/` 一致：`-ngl 99 -c 32768 -np 3`，空闲指加载完成
后 2 s、无请求；active 指 6 张小图、`jobs=3`（或表内注明）。

| 配置 | 空闲 RSS | 空闲 footprint | active 峰值 footprint | 说明 |
|---|---:|---:|---:|---|
| `-c 4096 -np 1` | 1598 MB | 866 MB | — | KV 256 MiB |
| `-c 8192 -np 1` | 1855 MB | 1.1 GB | 1.5 GB | KV 512 MiB |
| `-c 12288 -np 1` | 2111 MB | 1.3 GB | 1.8 GB | KV 768 MiB |
| `-c 32768 -np 1` | 3277 MB | 2.6 GB | 3.0 GB | KV 2 GiB |
| **`-c 32768 -np 3`（当前）** | **3278 MB** | **2.6 GB** | **2.9 GB** | 3 个 slot 共享 2 GiB KV |
| `-c 32768 -np 3 -ctk q8_0 -ctv q8_0` | 2386 MB | 1.7 GB | 2.0 GB | KV 1 GiB |
| `-c 32768 -np 3 -ctk q4_0 -ctv q4_0` | 1927 MB | 1.2 GB | 1.4 GB | KV ~0.5 GiB |
| `-c 32768 -np 3 -ctk q8_0 -ctv q8_0 --sleep-idle-seconds 5` | **105 MB** | **146 MB** | — | 休眠后；唤醒请求 +0.2–0.7 s |

> README 里“空闲 1748 MB”与本次 `-c 32768 -np 3` 的 3.3 GB 对不上。按实测，
> 1748 MB 更接近 `-c 8192` 一档；那张表应该是更早的上下文/运行时版本测的。
> 本页所有数字都给了可复现参数，建议以本页为准重跑一次 README 表。

### 2.1 为什么 `-np 3` 不比 `-np 1` 多占

llama-server 的总 KV 池由 `-c` 决定；`-np` 只是把总池分给 N 个 slot。本机日志是
`n_ctx_slot=11008, kv_unified='false'`，三个 slot 合计仍是 `-c` 向上取整后的 33,024
token。所以 `-c 32768 -np 3` 与 `-c 32768 -np 1` 的空闲 footprint 都是 2.6 GB；
变的只是每 slot 可用 10,922 token。**`-np` 不是内存旋钮，`-c` 才是。**

### 2.2 另一个会缓慢长大的东西：prompt cache

`-cram` 默认 8192 MiB，`--cache-idle-slots` 默认开启（参数定义见
[llama.cpp server README](https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md)）。
连续跑 24 张不同截图后：

| 配置 | 起始 RSS | 24 次后 RSS | 增量 |
|---|---:|---:|---:|
| 默认 `-cram 8192` | 2078 MB | 2286 MB | **+207 MB** |
| `-cram 0 --no-cache-idle-slots` | 2058 MB | 2056 MB | **-1 MB** |

OCR 的 prompt 每次都是新图，前缀缓存命中率极低；**建议 `-cram 0`**（注意 b12018
只认短选项 `-cram`，`--cram` 会报 invalid argument）。

---

## 3. 热门本地部署项目横评

一句话：这些项目分三类，**没有一类能在“同一个模型、同一个 32k 上下文”下比
llama.cpp 更省内存**。

| 项目 | 内核/形态 | 对本项目的内存结论 |
|---|---|---|
| **llama.cpp / llama-server** | 自己的 C++ 推理；GGUF；mmap | 已经是基线，且提供 sleep/KV 量化/上下文裁剪，**不需要换** |
| **Ollama** | 打包 llama.cpp + Go 守护进程 | 同权重同 KV；默认 `num_ctx=4096`，连一个 4K 切块的 ~6k vision prompt 都放不下；`keep_alive` 默认 5 分钟卸载——这个能力 llama.cpp 现在也有（[Ollama FAQ](https://docs.ollama.com/faq)） |
| **LM Studio** | llama.cpp / MLX + GUI | JIT + TTL 默认 **60 分钟**（[TTL 文档](https://lmstudio.ai/docs/developer/core/ttl-and-auto-evict)）；GUI/Electron 另算；不能作为内嵌 sidecar |
| **Jan / GPT4All** | llama.cpp + Electron/Python 壳 | 同权重同 KV，再加壳的内存；不适合随包分发（[Jan](https://github.com/janhq/jan)、[GPT4All](https://github.com/nomic-ai/gpt4all)） |
| **LocalAI** | 多后端（llama.cpp/vLLM/MLX） | 按模型拉后端镜像；多了 Go 服务和容器层，**更重**（[LocalAI](https://github.com/mudler/LocalAI)） |
| **llamafile / KoboldCpp** | llama.cpp 单文件/单 exe | 内存等价，只是分发形式不同；没有额外省内存机制（[llamafile](https://github.com/Mozilla-Ocho/llamafile)） |
| **vLLM / SGLang / TensorRT-LLM** | GPU serving；PagedAttention/RadixAttention | 为吞吐设计，启动就按 `gpu_memory_utilization` **预分配 90%+ 显存**（[vLLM](https://docs.vllm.ai/en/latest/configuration/engine_args.html)）；不以 macOS Metal / Windows CPU 为目标；单用户场景只会更大 |
| **MLC-LLM** | TVM 编译；WebGPU/Metal | 没有现成 GLM-OCR 权重，要自己转换/调编译；省不了这个量级的 KV |
| **MLX / mlx-vlm** | Apple 专属 | 本仓库选型报告实测 4 bit 2.2 GB，和 llama.cpp Q8 的 1.8–2.4 GB 同档；且 Windows 无法复用 |
| **Tesseract / RapidOCR** | 传统 CNN | 50–300 MB，但 `content_acc` 0.899 vs 0.957、`sym_recall` 0.949 vs 0.996；只能当兜底/省内存档，不能替代 VLM |

**为什么“换运行时”不成立**：模型文件一样、token 数一样，KV 的字节数就一样。
Ollama/LM Studio 的“低内存”来自**更小的默认上下文**或**空闲卸载**，不是更省的
kernel；前者会截断 6k vision prompt，后者 llama.cpp 已经原生支持。Ollama 也暴露了
`OLLAMA_KV_CACHE_TYPE=q8_0`，但它就是同一个 llama.cpp KV 量化开关，不会比
`-ctk/-ctv q8_0` 更省；Ollama 文档自己也建议 q8_0、并把 q4_0 标为“高上下文下
精度损失更明显”（[FAQ](https://docs.ollama.com/faq)）。

---

## 4. 量化实验

### 4.1 权重精度（`@2x`，25 场景，`-c 16384 -np 1`）

官方仓库只有 Q8_0 / F16 / mmproj-Q8_0，没有 Q4/Q5。实验阶段先用官方 F16
本地量化（非二次量化）：

```bash
# 官方仓库只有 Q8_0 / F16 / mmproj-Q8_0，没有 Q4/Q5
hf download ggml-org/GLM-OCR-GGUF GLM-OCR-f16.gguf \
  --local-dir ~/models/GLM-OCR-GGUF

# 下表 Q4_K_M / Q5_K_M / Q6_K / Q4_0 / Q3_K_M 的来源（M5 上约 5 s）
llama-quantize ~/models/GLM-OCR-GGUF/GLM-OCR-f16.gguf \
  ~/models/GLM-OCR-GGUF/GLM-OCR-Q4_K_M.gguf Q4_K_M 8
```

随后验证了 HF 上现成的
[`mradermacher/GLM-OCR-GGUF`](https://huggingface.co/mradermacher/GLM-OCR-GGUF)
（revision `11fad2c9…`，`GLM-OCR.Q4_K_M.gguf`，548,515,136 B，
SHA-256 `bb27f245…de1`，MIT）：25 个 `@2x` 场景里 **24 个与自量化完全一致**，
只有 `nerd-symbols` 高 0.62 pp；聚合 0.9587 vs 0.9585。`@1x` 反而略好
（content 0.9305 vs 0.9287，text 0.9478 vs 0.9460，sym 0.9742 vs 0.9622），
`@photo` 完全相同（0.9674）。因此 **app 的低精度包直接使用 HF 文件**，
不需要用户自己量化；自量化只作为可复现的对照保留在这里。

| 量化 | 文件 | content_acc | text_acc | sym_recall | indent_exact | rows_ratio | ms/img |
|---|---:|---:|---:|---:|---:|---:|---:|
| Q8_0（官方） | 950 MB | 0.9552 | 0.9819 | 0.9955 | 0.910 | 0.983 | 2341 |
| Q6_K | 735 MB | 0.9550 | 0.9817 | 0.9955 | 0.912 | 0.983 | 2457 |
| Q5_K_M | 639 MB | 0.9549 | 0.9816 | 0.9956 | 0.910 | 0.983 | 2861 |
| **Q4_K_M** | **549 MB** | **0.9585** | 0.9808 | 0.9940 | 0.912 | 0.983 | 2896 |
| Q4_0 | 528 MB | 0.9523 | 0.9790 | 0.9940 | 0.910 | 1.004 | 2711 |
| Q3_K_M | 450 MB | 0.9513 | 0.9689 | 0.9943 | 0.908 | 0.985 | 2872 |

Q4_K_M 的均值反而略高，是 `tree` 场景的框线字符多认回来一些（0.767→0.869），
**不是普遍提升**；符号召回和 `text_acc` 略降。分场景最大回退是 `conflict`
1.000→0.990、`pytest-traceback` 0.911→0.904，没有灾难性崩坏——这与选型报告里
PaddleOCR 4 bit 的复读完全不是一回事。

> 这批量化的权重没有用 importance matrix。若要正式上 Q4，建议先用 50–200 张真实
> 代码/终端截图跑 `llama-imatrix` 再量化，重点保护视觉投影和符号密集层；本次的
> `@1x` 掉点还有机会再压一些，但需要重新跑基线验证。

### 4.2 鲁棒性：低分辨率和拍屏

| 变体 | Q8 content / text / sym | Q5_K_M content / text / sym | Q4_K_M content / text / sym |
|---|---|---|---|
| `@2x`（25 场景） | 0.9552 / 0.9819 / 0.9955 | 0.9549 / 0.9816 / 0.9956 | 0.9585 / 0.9808 / 0.9940 |
| `@1x`（25 场景） | 0.9341 / 0.9596 / 0.9690 | 0.9265 / 0.9433 / 0.9644 | 0.9287 / 0.9460 / 0.9622 |
| `@photo`（4 场景） | 0.9460 / 0.9460 / 0.9602 | 0.9674 / 0.9674 / 0.9602 | 0.9674 / 0.9674 / 0.9735 |
| `huge-4k`（3 块） | 56/56，1.000 | 未测 | 56/56，全部 1.000（HF 文件同样 56/56） |

`@1x` 的分场景掉点并不均匀：Q5 的 `tree` -6.2 pp、`real-editor-term` -5.9 pp；
Q4_K_M 的 `real-editor-term` -5.5 pp、`tree` -1.7 pp。也就是说，**Q5 并不比 Q4_K_M
更稳**，两者都比 Q8 差 0.5–0.8 pp（均值），最差场景差 5–6 pp。Q5 的优势只在
`@2x` 常规截图和“少下 311 MB”。

**结论**：如果只处理 Retina `@2x` 图，Q5_K_M 几乎就是现状，Q4_K_M 也在噪声内；
如果经常处理 `@1x`（微信/Slack 压缩图），4/5 bit 的退化是真实的，Q8 仍然最稳。
`@photo` 只有 4 张样本，不能据此下强结论。

### 4.3 KV cache 量化

| KV 类型 | 32k footprint | 25 场景 content | sym | 速度（Metal） | 速度（CPU, `-ngl 0`） |
|---|---:|---:|---:|---|---|
| f16（默认） | 2.6 GB | 0.9552 | 0.9955 | 基线 | 2.18 s/图 |
| **q8_0** | **1.7 GB** | **0.9552**（逐场景相同） | 0.9955 | 基本不变 | **4.31 s/图（约 2× 慢）** |
| q4_0 | 1.2 GB | 0.9593* | 0.9938 | 约 +5% | 未测，预计更慢 |

\* q4_0 那一行用的是 Q4_K_M 权重，和同权重的 f16 KV（0.9585）比是 +0.08 pp，属于噪声；
长上下文（>8k）没有单独验证，**不建议作为默认**。

**关键平台差异**：q8 KV 在 Metal 上几乎不掉速，但在 CPU 上要每 token 反量化，
实测 2.18 s → 4.31 s。`app/` 的 Windows 内置运行时是 **CPU-only**（`engine/server.rs` 在
非 macOS 传 `-ngl 0`），所以 **不要把 q8 KV 无条件下放到 Windows 默认配置**。

### 4.4 mmproj 不要量化

`mmproj-GLM-OCR-Q8_0.gguf`（484 MB）占下载量的 1/3，很诱人。但 Q5_K_M 重量化后：

| mmproj | content | text | sym | indent_exact | rows_ratio |
|---|---:|---:|---:|---:|---:|
| Q8_0 | 0.9585 | 0.9808 | 0.9940 | 0.912 | 0.983 |
| Q5_K_M | 0.9579 | 0.9803 | 0.9947 | **0.862** | **1.176** |

内容指标几乎没变，但**行结构变差**：多出 18% 的行、缩进准确率掉 5 pp。视觉编码器
的量化误差会变成“多吐/少吐一行”，对代码截图是硬伤。**保持官方 Q8 mmproj。**

### 4.5 权重 4 bit 到底省了什么

在 `-c 32768 -np 3` 下，六种权重的空闲 footprint **全是 2.6 GB**：

| 权重 | RSS | footprint |
|---|---:|---:|
| Q8_0 | 3333 MB | 2.6 GB |
| Q6_K | 3153 MB | 2.6 GB |
| Q5_K_M | 3087 MB | 2.6 GB |
| Q4_K_M | 3012 MB | 2.6 GB |
| Q4_0 | 3025 MB | 2.6 GB |
| Q3_K_M | 2982 MB | 2.6 GB |

因为权重是 clean mmap 页，不计入 footprint，而 footprint 被 2 GiB KV 主导。权重
量化真正省的是：**磁盘/下载**（1.43 GB → 1.03 GB，含 mmproj）、**RSS**（约 350 MB）、
以及 **Windows CPU 上的物理内存**（`-ngl 0` 实测 Q4_K_M 比 Q8 少 200–500 MB，速度
持平 2.19 s）。所以：

- macOS 上想降“活动监视器内存” → **量化 KV，不是量化权重**；
- Windows CPU 上想降物理内存 → 权重 Q4_K_M 有效，但要自己托管/分发非官方 GGUF。

### 4.6 一张表：Q8 / Q5 / Q4 的内存差

同一套启动参数下（空闲 = 加载后 2 s；active = 6 张图 jobs=3；休眠 = 300 s 空闲后）：

| 权重 | 磁盘（含 mmproj） | 32k + q8 KV 空闲 RSS / footprint | 32k + q8 KV active RSS / footprint | 12k `-np 1` + q8 KV 空闲 RSS / footprint | 休眠后 RSS / footprint |
|---|---:|---:|---:|---:|---:|
| Q8_0（现状） | 1.43 GB | 2359 MB / 1.7 G | 2632 MB / 2.0 G | 1751 MB / 1.0 G | 105 MB / 146 M |
| Q5_K_M | 1.12 GB | 2157 MB / 1.7 G | 2223 MB / 1.8 G | 1531 MB / 1.0 G | 98 MB / 147 M |
| Q4_K_M | 1.03 GB | 2159 MB / 1.7 G | 2200 MB / 1.8 G | 1464 MB / 1.0 G | 141 MB / 147 M |

相对 Q8 的差：

| 权重 | 磁盘 | 32k 空闲 RSS | 32k active RSS | 12k 空闲 RSS | footprint |
|---|---:|---:|---:|---:|---:|
| Q5_K_M | **-311 MB** | **-202 MB** | **-409 MB** | **-220 MB** | 空闲不变；active -0.2 G |
| Q4_K_M | **-401 MB** | **-200 MB** | **-432 MB** | **-287 MB** | 空闲不变；active -0.2 G |

注意两点：

1. **Q5 和 Q4_K_M 的 RSS 几乎一样**（32k 下 2157 vs 2159 MB），因为 mmap 的权重页
   只常驻一部分；文件小 91 MB 不等于 RSS 小 91 MB。
2. **休眠后三者都是 ~100–150 MB**，权重已卸载；Q4/Q5 省的那几百 MB 只在“模型醒着”
   时存在。真正把常驻从 GB 降到 100 MB 级的是 `--sleep-idle-seconds`，不是量化。

**如果一定要选一个“损失一点精度换内存/磁盘”的权重：选 Q4_K_M。** 在这批测试里它
`@2x` 内容最好（0.9585）、`@1x` 也比 Q5 略稳（0.9287 / 0.9460 vs 0.9265 / 0.9433）、
磁盘最小（549 MB）、`huge-4k` 仍 56/56；Q5 只在 `sym_recall` 上高 0.16 pp，却多 90 MB
且 `@1x` 更差。Q4_0 的 `pytest-traceback` 单场景 -4.1 pp，Q3 只再小 99 MB 但 `text_acc`
-1.3 pp，都不划算。

### 4.7 Windows 上精度会变吗

会，但通常不是“Windows 这个 OS”造成的，而是**后端和配置**不同：

| 会变的 | 不会变的 |
|---|---|
| Metal vs CPU/Vulkan/CUDA 的 kernel | 同一个 GGUF 权重、tokenizer、chat template |
| 浮点累加顺序、fast-math、SIMD 路径 | prompt、temperature=0、max_tokens |
| Q4_K/Q5_K 的反量化实现（NEON vs AVX2/AVX512） | 图片内容、几何层、后处理 |
| KV 类型（f16/q8_0）、`-c`/`-np`、是否 GPU | 解码出的“意图” |

因此：

- **Q8 + f16 KV 的聚合指标通常只差零点几 pp**；但贪心解码在近似并列的 token 上仍可能
  翻一个，代码/框线场景会放大成整行差异。
- **Q4_K_M / Q5_K_M 的跨后端差异更大一些**：反量化 kernel 不同，`@1x`、`tree`、
  `real-editor-term` 这类尾部场景可能差 0.5–1 pp，方向不确定（可能更好也可能更差）。
- 本仓库的 Windows 内置运行时是 **CPU-only**，而本页数据是 macOS Metal；这本身就
  不是同一后端。不能把这里的 Q4/Q5 数字直接当成 Windows 的发布基线。
- `--sleep-idle-seconds`、`-cram`、`-c` 只影响内存/速度，不影响精度。

**Windows 落地前必须实机跑一遍**（同一份模型 + 同一个 prompt）：

```bash
# CPU（内置运行时）和 GPU（Vulkan/CUDA 外部运行时）各跑一次
python3 bench/baseline.py --engines llamacpp --variant 2x
python3 bench/baseline.py --engines llamacpp --variant 1x
# 重点看：tree / real-editor-term / pytest-traceback / light-ambiguous / tiny-crop
```

对比时看**分场景**，不要只看均值：均值差 0.2 pp 可能只是某个场景多吐了一行。

---

## 5. 推荐配置

> app 当前默认：**低精度 Q4_K_M**、空闲 5 分钟释放、`-cram 0`、macOS KV q8、
> Windows f16 KV；`-c/-np` 跟随 `jobs` 与 `max_tokens` 计算。下面保留手动部署命令，
> 便于 CLI 或外部 server 复现。

### A. 默认（macOS / Metal，质量优先）

```bash
llama-server -m ~/models/GLM-OCR-GGUF/GLM-OCR-Q8_0.gguf \
  --mmproj ~/models/GLM-OCR-GGUF/mmproj-GLM-OCR-Q8_0.gguf \
  --host 127.0.0.1 --port 8099 -ngl 99 -c 32768 -np 3 \
  -ctk q8_0 -ctv q8_0 -cram 0 --sleep-idle-seconds 300
```

- 活动时 footprint 1.7 GB（原 2.6 GB），峰值约 2.0 GB；
- 空闲 5 分钟后 146 MB；
- 精度与 Q8 + f16 KV 逐场景相同；
- 首次请求唤醒 +0.2–0.7 s。

### B. 省内存（单并发，适合 8 GB 机器）

```bash
llama-server ... -c 11008 -np 1 -ctk q8_0 -ctv q8_0 -cram 0 --sleep-idle-seconds 300
```

- 空闲 footprint 约 1.0 GB（`-c 11008`），峰值预计 ~1.4 GB（未单独采样）；
- `jobs=1` 时 4K 切块只能串行（README 实测 71.6 s vs 48.7 s）；
- 仍能容纳一个 ~6k 切块 + 3k 输出。

### C. Windows CPU（内置运行时的默认形态）

```bash
llama-server ... -ngl 0 -c 32768 -np 3 -cram 0 --sleep-idle-seconds 300
```

- **不要加 `-ctk/-ctv q8_0`**：CPU 上约 2× 慢；
- 空闲休眠是唯一无痛的大头（3 GB → 100 MB 级）；
- 选“低精度 Q4_K_M”可再省 200–500 MB，速度持平；
- 有 Vulkan/CUDA 外部运行时再考虑 q8 KV。

### D. 极限（不推荐默认）

```bash
Q4_K_M 权重 + -c 32768 -np 3 -ctk q4_0 -ctv q4_0 -cram 0 --sleep-idle-seconds 300
```

- 空闲 footprint 1.2 GB；`@2x` 精度与 Q8 同级，`@1x` 有掉点，长上下文未验证；
- 只适合“内存比精度更贵”的用户。

> 注意：`-c` 不能随便再压。一个 4K 切块的 vision prompt 约 6k token，加
> `--max-tokens 3072`，单请求峰值约 9k；`-c 8192` 连一个切块都放不下。
> 要压上下文，必须同时把 `jobs` 降到 1，并按 `-np 1 -c 12288` 成对设置。

---

## 6. 代码落点

> 1–6 已落地（模型精度、镜像、代理、sleep/KV/cram、`-c/-np` 跟随 jobs、设置项）；
> 7（唤醒隐藏延迟）尚未做。

1. **高/低精度模型选择**：`Settings.model_quality` + `ModelQuality` 枚举；
   高精度走官方的 `GLM-OCR-Q8_0.gguf`，低精度走 HF 的 `GLM-OCR.Q4_K_M.gguf`；
   两套 manifest 独立，共用 mmproj；切换时停掉旧 server，未下载则弹引导。
2. **镜像勾选**：`Settings.use_mirror` 切换 `huggingface.co` / `hf-mirror.com`；
   不再让用户填自定义基址（内网仍可用 `SOFER_MODEL_BASE`）。
3. **代理拆分**：`proxy_url` + `proxy_username` + `proxy_password`；
   保存时用 `compose_proxy_url()` 做 userinfo 百分号编码（`@` → `%40`），
   `normalize_proxy_url()` 在交给 ureq 前解码。密码输入框 `type=password`。
4. **`app/src-tauri/src/engine/` 的 `start_managed()`**（约 730 行）
   - 追加 `--sleep-idle-seconds <N>`（`N=300` 或设置项）；
   - 追加 `-cram 0`（短选项，b12018 不认 `--cram`）；
   - macOS 追加 `-ctk q8_0 -ctv q8_0`；Windows 暂不加（CPU 掉速）。
5. **把 `-np`/`-c` 和 `jobs` 绑定**
   - 现在 `jobs` 只影响 CLI 并发，server 永远 `-np 3 -c 32768`；用户把 jobs 调成 1
     仍然付 2 GiB KV。
   - 建议 `np = clamp(jobs,1,3)`、`c = np * 11008`（向上取整到 256 的倍数）；
     改 jobs 时重启 server（`start_managed()` 已有生命周期管理）。
6. **设置项**：`idleSleepSecs`、`kvCacheQuant`（仅 macOS/GPU 默认开）、
   `memorySaver`（jobs=1 + c=12288）。TS 侧 `types.ts` / `defaults.ts` /
   `SettingsView.tsx` 同步。
7. **唤醒隐藏延迟**：按下全局快捷键、开始选区时，向 server 发一个 1-token 的
   completion 触发唤醒；用户拖框的 1–2 s 正好覆盖重载，感知不到 +0.7 s。
8. **健康检查**：休眠时 `/health` 仍返回 200（实测），现有轮询不会误判离线。
9. **README**：§5 的内存表按本页重测；§2.2 的启动命令加省内存变体。

---

## 7. 复现命令

```bash
# 1) 看空闲 footprint
llama-server -m .../GLM-OCR-Q8_0.gguf --mmproj .../mmproj-GLM-OCR-Q8_0.gguf \
  --host 127.0.0.1 --port 8099 -ngl 99 -c 32768 -np 3 &
PID=$(lsof -nP -iTCP:8099 -sTCP:LISTEN -t)
ps -o rss= -p $PID
vmmap -summary $PID | grep "Physical footprint"

# 2) 看 active 峰值（另开终端）
sofer ocr bench/images/*@2x.png --engine llamacpp \
  --llama-url http://127.0.0.1:8099 -j 3 --format json >/dev/null

# 3) 量化（从 F16，避免二次量化）
llama-quantize GLM-OCR-f16.gguf GLM-OCR-Q4_K_M.gguf Q4_K_M 8

# 4) 走基线
python3 bench/baseline.py --engines llamacpp --variant 2x \
  --scenes $(ls bench/images/*@2x.png | sed 's/.*\///;s/@2x.png//' | grep -v huge-4k | paste -sd,)
```

---

## 8. 边界

- 精度是 **25 个合成场景 + 4 张拍屏**，真实截图覆盖仍然薄；`@1x` 已经显示 4 bit
  的退化，真实微信/Slack 图要再测。
- KV q4 只在 1–3k token 的短上下文验证过；长上下文（>8k）误差会累积，不要默认开。
- Windows 侧的数字来自 **Mac 上 `-ngl 0` 的 CPU 代理**，不是 Windows 实机；
  x64 CPU 的 q8 KV 速度可能不同，落地前按 README §9.3 的清单复测。
- `--sleep-idle-seconds` 的唤醒延迟与模型大小、磁盘有关；本机 Q8 约 +0.7 s，
  Q4_K_M 约 +0.2 s。
- 所有数字都是 M5 / 16 GB / macOS 27；内存压力、热降频会让绝对值漂移，
  但“KV 占大头、休眠最有效、q8 KV 无损”三个结论不依赖具体机器。
