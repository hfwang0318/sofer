# 维护手册

面向**改这个仓库的人**。使用方法见 [README](../README.md)，设计取舍见 [选型报告](ocr4code-选型报告.md)，
指标定义见 [基线报告](baseline.md)，桌面端界面与提示语的规则见
[UI 交互与文案规范](ui-guidelines.md)。

---

## 1. 仓库地图

```
src/                 内核（纯 Rust，平台无关）
  geom.rs            像素 → 行带 + 列原点 + advance
  tile.rs            切块规划 / 拼接 / 重试判定
  assemble.rs        文本行 ↔ 行带配对、行号槽、列距、软换行
  repair.rs          破折号 / 引号 / 全角折叠
  lang.rs            围栏语言猜测
  render.rs          text / markdown / json
  engine/            llamacpp（主力）、tesseract（兜底）、mock（测试）
tests/golden.rs      集成测试：真值文本 → 几何层 → 比缩进
bench/
  scenes.py          20 个普通合成场景
  scenes2.py         复杂场景（双栏、图标、嵌套 diff、Powerline…）
  scenes3.py         极端几何（小图 / 4K / 19:1 / 1:11）
  images/ truth/     素材与逐字符真值（**生成物，不要手改**）
  baseline.py        走 CLI 的基线跑批
  tools/             打分器、各引擎 runner、真机截图工具
  results/           基线产物（**生成物**）
docs/                选型报告 / 基线 / 内存实测 / UI 规范 / 本手册
app/                 Tauri + React 桌面壳（见 app/README.md）
CHANGELOG.md         桌面端 v0.1.0–v0.1.10 变更记录
```

真机截图的采集方法见 [bench/tools/real_capture/README.md](../bench/tools/real_capture/README.md)。

---

## 2. 指标口径与陷阱

| 指标 | 定义 | 常见误读 |
|---|---|---|
| `content_acc` | 去空白后字符准确率 | 框线丢失会大幅拉低它。`ascii-table` 0.58 但 `text_acc` 是 1.00 |
| `text_acc` | 去掉 box-drawing 再算 | 判断"字读得准不准"看这个 |
| `box_recall` | 真值里框线字符的复原率 | 只在含框线的场景有意义（其余场景恒为 1.00，会虚高均值） |
| `sym_recall` | 代码符号多重集召回 | 代码可用性的最强预测项；`content_acc` 高但 `sym_recall` 低 = 符号被吃 |
| `indent_exact` | 行首空格完全正确的比例 | 分母是"能按文本匹配上的行"，覆盖率另算（`indent_cov`） |
| `rows_ratio` | 输出行数 ÷ 期望行数 | 评测分母**含空行**；内核 `--stats` 分母是**行带数**。1.00 不代表内容对 |
| `ms/img` | CLI 墙钟 | 含进程启动；切块场景会拉高均值 |
| `peak MB` | 峰值 RSS | 口径不一（子进程 RSS / Metal buffer / 服务进程）；模型内存不在 CLI 里 |

**判读顺序**：先看 `rows_ratio`（结构对不对）→ 再看 `sym_recall`（符号对不对）→ 最后 `content_acc`（字对不对）。

内存不是这张表里的指标：RSS 含 mmap 的 clean page，活动监视器看的是 footprint；
`-c 32768` 的 KV 本身就是 2 GiB。测法、量化与运行时横评见
[内存实测](memory-footprint.md)。

---

## 3. 改动守则

### 3.1 动几何层（`geom.rs` / `assemble.rs`）

**必须同时验证三样东西**，缺一不可：

1. `cargo test`（53 单元 + 24 集成）
2. **合成场景的 mock 基线**：26 个场景 `indent_exact` 应 ≈ 1.00
3. **两张真机截图**（`real-editor-code` / `real-editor-term`）：同样是 1.00

合成图全绿不代表真机能过。真实截图已经抓出过两个 bug：

| 症状 | 根因 | 现在的做法 |
|---|---|---|
| 每级缩进偏 1 | ink 阈值取 `p90×0.35`，文字只占几个百分点，p90 ≈ 背景，窗口边框被当成墨迹，列原点被拉到 0 | 阈值看分布亮端；**边框靠裁掉，不靠抬阈值**（抬阈值会丢 `_` 和 `-`） |
| `tree` 段 16 行塌成 8 个行带 | `min_ink` 太低，框线竖笔画跨行相连，每一行都有墨 | `min_ink = max(3, w×0.002)` |

**列原点不要用"全局最左墨迹"**：光标、窗口边框、选中把手都会把它带偏。目前的做法是把截图裁干净，
并在真机验证里覆盖这个点。

调试开关：

```bash
OCR4CODE_DEBUG=1 ocr4code ocr shot.png --engine mock --mock-text-file truth.txt --stats
# -> [debug] typo=15.60 adv=15.90 n_ratios=18 ratios=[...]
```

### 3.2 动切块（`tile.rs`）

- `tokens_per_row` 是从 `/tokenize` 实测来的（235 字符 → 134 token ≈ **1.75 字符/token**）。
  **换模型或大改 prompt 后必须重新标定**，否则行数预算会错。
- 拼接**只能按行带序号**。用文本去重会因模型规范化空格（`INFO  worker` → `INFO worker`）而失效——
  实测 0.893 vs 1.000。
- 改完必须跑 `huge-4k`（唯一会触发切块的场景），期望 **56/56 行、content 1.000**。
- `mock` 引擎必须加 `--tile off`（它无视图片只回放文本，切块会把文本复制三遍）。`baseline.py` 已内置这层保护。

### 3.3 动并发（`pool.rs` / `--jobs`）

- `pool::map` 保证**输出顺序 = 输入顺序**（结果写回各自下标的槽位，不靠排序）。
- `jobs = 1` 走内联路径，完全不建线程——默认行为和加并发之前一模一样。
- **临时图像文件名必须唯一**：需要落盘的引擎（tesseract）使用
  `ocr4code-input-{pid}-{序号}.{ext}`。只带 pid 会在 `--jobs > 1` 时同进程内互相覆盖
  （曾经就是这么写的）。切块不再落盘：`Engine::recognize_image` 直接接收内存像素。
- `Engine` trait 带 `Send + Sync` 约束。加新引擎时不要往里面塞 `Rc` / `Cell`。
- 并发数不是越大越好：llama-server 默认 4 个 slot，实测 6 张图 jobs=3（3.04 s）比 jobs=6（3.44 s）快。
  调 slot 数用 `-np`，同时注意 `-c` 会被 slot 平分。
- **`--jobs` 是全局预算**（`pool::Semaphore`），图片级和切块级共用。新增任何并发路径都必须从这个信号量取许可，
  否则会绕开预算。
- 切块路径的 `ms` 用**墙钟**（`Instant::now()` 包住整个 tile 阶段），不要累加各块耗时——并行时会虚高 N 倍。

### 3.4 动引擎 / prompt

- 默认 prompt 是实测选出来的：`OCR:` 会让宽表格**每格一行**（`rows_ratio` 6.0）；
  `OCR: preserve the original layout, spacing and line breaks exactly.` 才能拿到 rows 1.17、content 1.000，而且更快。
  改 prompt 必须重跑整张基线，不能只看一两个场景。
- GLM-OCR 的 GGUF 若缺 `chat_template`，默认 framing 会触发模板 token 复读死循环
  （[llama.cpp #31298](https://github.com/ggml-org/llama.cpp/issues/31298)）。当前走 server 的消息模板路径正常，但别绕过它。

---

## 4. 如何扩展

### 加一个测试场景

| 场景类型 | 加在哪 |
|---|---|
| 普通终端/代码截图 | `bench/scenes.py` 的 `SCENES` |
| 复杂排版（双栏、图标、嵌套 diff…） | `bench/scenes2.py` 的 `NEW` |
| 极端几何（尺寸/宽高比） | `bench/scenes3.py` 的 `EXTREME` |

每个场景要同时产出 `@2x.png`、`@1x.png`、`@2x.jpg`（脚本自动做），真值是**逐字符**的 `bench/truth/<name>.txt`。
加完跑 `python3 bench/scenes*.py` 再跑基线，确认新场景在 mock 下 `indent_exact ≈ 1.00`——否则是几何层的问题，不是引擎的。

### 加一个引擎

1. 在 `src/engine/` 新建模块，实现 `Engine` trait：

```rust
fn name(&self) -> &str;
fn recognize(&self, image: &Path) -> anyhow::Result<EngineOutput>;
```

2. 在 `src/engine/mod.rs` 的 `Spec` 里加一个变体 + `build()` 分支。
3. 在 `main.rs` 的 `EngineArg` 里加一个 clap 值。
4. 跑基线的三个变体，把 `bench/results/` 的数字补进 `docs/baseline.md`。

**可以只返回文本，不必给 bbox**——缩进由几何层负责。这是刻意的设计，别把平台 API 依赖塞进引擎。

### 换模型

只换 llama.cpp 那一侧，`src/` 不用改：

1. 确认有官方 GGUF（`ggml-org/*` 优先），且 `llama-server` 能加载 mmproj；
2. 起 server，`ocr4code doctor` 应显示 ok；
3. 重跑基线；重点看 `sym_recall`、`rows_ratio`、`huge-4k`；
4. 若换的模型输出更长/更短，重新标定 `CHARS_PER_TOKEN`（`tile.rs`）。

---

## 5. 基线流程

```bash
cargo build --release
python3 bench/scenes.py && python3 bench/scenes2.py && python3 bench/scenes3.py
for v in 2x 1x 2xjpg photo; do
  .venv/bin/python bench/baseline.py --engines mock,tesseract,llamacpp --variant $v \
    --md bench/results/baseline-$v.md
done
```

**判据**

| 检查项 | 期望 |
|---|---|
| `mock` 的 `indent_exact` | ≥ 0.95（低于此说明几何层退化） |
| `llamacpp` 的 `content_acc` | ≥ 0.95 |
| `llamacpp` 的 `sym_recall` | ≥ 0.99 |
| `huge-4k` | 切块后 56/56 行、content 1.000 |
| `tesseract` | 不低于基线的 −0.02 |

跑完记得把 `docs/baseline.md` 与 `README.md` 里的数字同步——两处的表都是手工维护的，
**唯一事实来源是 `bench/results/baseline-*.md`**。

---

## 6. 发布检查清单

### 根 crate

- [ ] `cargo test`（53 unit + 24 integration）
- [ ] `cargo fmt --all -- --check` 与 `cargo clippy --all-targets -- -D warnings` 通过
- [ ] `cargo build --release` 无 warning
- [ ] 四个变体基线跑完，达到 §5 的判据
- [ ] `huge-4k` 手工 smoke（`--stats` 看 `tiling:` 与 `warning:`）
- [ ] `README.md` / `docs/baseline.md` 数字已同步
- [ ] CI（`.github/workflows/ci.yml`，macOS + Windows）绿

### 桌面端 / Windows Release

- [ ] `cd app/src-tauri && cargo test`（70 tests）
- [ ] `cd app/src-tauri && cargo fmt -- --check && cargo clippy --all-targets -- -D warnings`
- [ ] `cd app && pnpm test:run`（81 tests）与 `pnpm e2e`（8 cases）通过
- [ ] `cd app && pnpm build` 通过
- [ ] `python3 scripts/check-versions.py` 通过
- [ ] `app/scripts/test-remote.sh`（Docker，约 2–3 分钟；会下载 1 GB 模型）
- [ ] 远程手工用例：取消续传、损坏恢复、非 Linux 拒绝、隧道端口冲突
      （步骤见 [app/README.md](../app/README.md) 的「本地 Docker 联调」）
- [ ] app CI（`.github/workflows/ci.yml` 的 `app` job，macOS + Windows）绿
- [ ] 更新 `.github/workflows/release.yml` 的版本/tag（或 workflow_dispatch）
- [ ] GitHub Secrets 已配置 `TAURI_SIGNING_PRIVATE_KEY` 与
      `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`
- [ ] Windows runner 先 `fetch-runtime.py`，再 `stage-vc-runtime.ps1`，
      `llama-server.exe --version` 冒烟通过
- [ ] NSIS 产物含 `runtime/llama/msvcp140.dll` 等 CRT
- [ ] Release 附件包含 `*-setup.exe`、`*.exe.sig`、`latest.json`
- [ ] `latest.json` 的版本、URL、signature 正确；`releases/latest/download/latest.json`
      可访问
- [ ] 在上一版安装上实际走一遍“检查更新 → 下载 → 重启安装”
- [ ] 更新前 `llama-server` 已停止；NSIS preinstall hook 存在
- [ ] 新增/变更的行为已写进 `app/README.md`、根 `README.md` 或 `CHANGELOG.md`

---

## 7. 故障排查

| 症状 | 先查什么 |
|---|---|
| `doctor` 显示 `[--] llama.cpp server` | server 没起 / 端口不是 8099；`OCR4CODE_LLAMA_URL` 可改地址 |
| 输出只有几行 | `--stats` 看 `rows_ratio`；`tiling: off` 但行数远小于行带数 → 预算估算错了，调 `--max-tokens` |
| 输出行数比真值多 | 模型超发（常见于把 `--max-tokens` 调很大）；看 `warning: output has N rows for M bands` |
| 缩进全是 0 | 是不是行号槽场景（`gutter=true` 属正常）；或用了 `--no-geometry` |
| 缩进整体偏 1–2 格 | 列原点被光标/边框带偏 → 把截图裁干净，或先点走光标再截 |
| tesseract 中文变乱码 | `--tess-lang eng+chi_sim`，且语言包必须能被 `TESSDATA_PREFIX` 找到 |
| markdown 围栏语言不对 | `--lang rust|python|diff|bash` 手动指定 |
| 4K 图很慢（~2 min） | 正常：3 块 × 20 行；判断是否需要切块可以 `--tile off` 对比 |
| `--jobs` 开着但没变快 | 看是不是超过 llama-server 的 slot 数；jobs=3 通常比 6 快 |
| 并发时输出顺序乱了 | 不应该发生：`pool::map` 按下标回写。若出现，先查是否有新的"结果拼装"绕过了它 |
| 结果里出现 `<|LOC_11|>` 之类 | prompt 用错了（别对 GLM-OCR 用 "Convert to markdown"），换回默认 prompt |
| Windows 更新报文件被占用 | v0.1.10 前需手动结束 `llama-server.exe`；v0.1.10 会先 stop + NSIS preinstall hook 兜底 |
| Windows `0xc0000005` | 先看是否旧 `msvcp140.dll`（14.16）；v0.1.6+ 安装包已内置 VS 2022 CRT |
| 代理下载 `unknown issuer` | 代理根证书需在系统信任库；v0.1.4+ 使用 native TLS |
| 镜像基址浏览器 404 | 基址本身不是文件；用 `<基址>/GLM-OCR-Q8_0.gguf` 验证，镜像必须提供两个文件 |

---

## 8. 已知债务

| 项 | 说明 | 影响 |
|---|---|---|
| 框线重建 | `--box-art` 已从像素恢复 `tree`/表格线，但默认关闭，且合并单元格/丢失左偏移只能部分恢复 | 默认输出仍是拍平文本；开启后才恢复 |
| Windows 覆盖有限 | 已有真机安装/更新验证，但多显示器混合 DPI/HDR/受保护内容仍未覆盖 | 自绘选区可能黑帧；CPU 推理耗时随机器变化 |
| 快捷键非任意键 | Tauri 全局快捷键要求一个主键 | 纯修饰键/两个普通键不可用；需要底层 hook 才能扩展 |
| 多栏阅读顺序 | 几何模型按行，双栏按栏输出 | `split-panes` 的 `indent_exact` 不代表质量 |
| 真机样本少 | 26 个场景里只有 2 张真截图 | 再补 50–200 张（深色/中英混排/微信压缩） |
| Powerline 图标 | 图标在像素上已经是 tofu，模型没有可映射的语义 | 需要像素模板匹配，而不是字符映射表 |
| Windows 滚动长截图 | 自绘选区一次只抓鼠标所在显示器，也没有滚动拼接 | 长日志需要用户分多次截图 |
| 快速预览冷启动 | Apple Vision 首次调用有系统级约 25 s 冷启动，之后 ~0.2 s | 首次开启「快速预览」时可能反而不如直接等模型 |
