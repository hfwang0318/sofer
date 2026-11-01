# 浏览器预览 E2E 回归

Playwright + Chromium，只跑 `pnpm dev` 的浏览器预览（`src/lib/bridge.ts` 的 mock 总线），
不涉及真实截图、屏幕录制权限、模型下载或原生窗口，可离线重复运行。

## 运行

```bash
cd app
pnpm exec playwright install chromium   # 首次运行需要一次
pnpm e2e                                # 无头运行，自动起/复用 127.0.0.1:1420
pnpm e2e:ui                             # 交互式调试
```

- `playwright.config.ts` 用 `webServer` 自动执行 `pnpm dev`；本地已占用 1420 时复用，
  CI 环境不复用并自行启动。
- 失败产物（trace、截图）写在 `e2e/.artifacts/`，已被 `e2e/.gitignore` 忽略。
- 断言只用角色与可见文本，不使用 CSS 选择器；每个用例单独打开页面，互不依赖。

## 覆盖

| demo | 断言 |
|---|---|
| `?demo=result` | 结果文本、`10 行`、`864 毫秒`、复制按钮变成“已复制”、剪贴板内容 |
| `?demo=setup` | 向导主操作可点、跳过回到截图页、横幅可重新打开；下载进度条与取消后的续传提示 |
| `?demo=permission` | 三步说明与系统设置位置、“稍后再说”关闭后仍有横幅和“去授权”、授权后的就绪分支 |
| `?demo=error` | `role=alert` 中的原因与下一步、不泄漏 `E_*` 错误码、重试截图与检查设置 |
| `?demo=history` | 3 条演示记录、单条删除、清空需二次确认（首次点击不清空） |
| `?demo=settings` | 修改后出现“有未保存的更改”、保存后回到“已保存”并写入 localStorage |

## 已知缺口

- `?demo=history` 目前没有搜索/过滤控件（`HistoryView.tsx` 只有复制、删除、清空），
  因此“搜索过滤”用例无法编写；本套件覆盖列表渲染、删除与清空二次确认。
- 结果面板在 `result.copied` 已为真时（预览默认自动复制），角标显示“已复制”，
  但复制按钮仍显示“复制”，点击后才同步。属文案状态不一致，不影响功能。
- 1420 是 Vite 固定端口：本地若已有别的 checkout 的 dev server 在跑，
  Playwright 会复用它，可能测到另一份源码；CI 中 `reuseExistingServer` 为 false，会自行启动。
- `.github/workflows/ci.yml` 不在本次改动范围内，未新增 e2e job。若要接入，可加一个
  `macos-latest` job：`pnpm install --frozen-lockfile` →
  `pnpm exec playwright install chromium` → `pnpm e2e`。
