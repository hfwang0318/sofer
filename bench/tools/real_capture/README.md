# 真实截图（real captures）

`bench/images/real-editor-*.png` 是**真机截图**，不是 PIL 渲染。它们由 TextEdit 打开一个
Menlo 14pt 的文档后按窗口截图得到，用于补上"合成图太干净"的偏差。

## 为什么值得补

合成图里，几何层在 26 个场景上 `indent_exact` 都是 1.00。换成真机截图后第一次跑出来是 **0.38**，
暴露了两个合成数据永远测不到的问题（都已在 `src/geom.rs` 修掉）：

1. **窗口边框/阴影被当成墨迹** → 列原点被拉到 0，每一级缩进都偏 1。
   根因：ink 阈值取 `p90 × 0.35`，而文字只占几个百分点，p90 基本等于背景。
   改为 `p99 × 0.30`（看分布的亮端，而不是中间）。
2. **框线的竖笔画跨行相连** → 行投影里每一行都有墨，`tree` 那段塌成一个行带（16 行 → 8 带）。
   根因：`min_ink = max(2, w*0.0015)` 太低。改为 `max(4, w*0.004)`。

另外真图里还有合成图没有的东西：拼写检查的红色波浪线、光标、窗口边框、以及 TextEdit 自己
的字偶距（真机 Menlo 14pt 的 advance ≈ 16.6px，合成图是 15.6px）。

## 怎么复现

需要 **屏幕录制** 权限（`screencapture`）和一次性的窗口 id 查询。**不需要** 辅助功能/自动化权限——
一开始用 AppleScript 控制 TextEdit 会卡在 TCC 授权弹窗上，改用 `open -a` + 按窗口 id 截图绕开了。

```bash
# 1. 列出窗口（拿到 CGWindowID）
swiftc -O windowlist.swift -o /tmp/windowlist
/tmp/windowlist | grep 文本编辑

# 2. 用 Menlo 渲染一段文本
textutil -convert rtf -font Menlo -fontsize 14 code.txt -output code.rtf
open -a TextEdit code.rtf

# 3. 把焦点切走，光标消失后再截（否则光标会污染最左列）
open -a Finder
screencapture -l <WindowID> -o -x cap.png

# 4. 裁掉标题栏/工具栏/标尺，只留文本区域
#    （y > 205，四周留 8px，边缘内缩 3px 去掉窗口描边）
```

真值就是被打开的 `code.txt` / `term.txt` 的内容，逐字符已知。

## 已知偏差（不要当成模型的问题）

- 截图包含拼写检查的波浪线（`engine`、`utils` 被标红），会在字下面多出一行点状墨迹。
- 真机的行距/字宽和合成图不同，所以两个集合的绝对分数不能直接相加。
- 这两个场景只有 4K 显示器的 2x 截图，没有 1x 真实样本。
