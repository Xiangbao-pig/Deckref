# Changelog

所有 notable 的功能与重要修复都会记录在此，便于多 Agent 与人类开发者快速了解近期变更。

格式：**日期** + 一句话描述 + 可选「涉及：文件/模块」。

---

## 未发布

- **2025-03-02** 同步代码至 GitHub：提交拖放、快捷抓图、监听与文档等改动；新增 `README.md`，完善 `.gitignore`（排除 `.cursor/`）。仓库：https://github.com/Xiangbao-pig/Deckref
- **2025-03-03** 拖出触发改为「指针离开窗口边缘」：拿起后仅在鼠标移出窗口边界时转为系统文件拖放，更符合直觉。涉及：`src/app.rs`、`docs/INTERACTIONS.md`。
- **2025-03-03** 拖放修复与拖出：窗口启用 `with_drag_and_drop(true)`，从浏览器拖入图片不再显示禁止标志；长按拿起后移动超过约 18px 会发起系统文件拖放，可拖到文件夹或其它软件。涉及：`src/main.rs`、`src/app.rs`、`docs/INTERACTIONS.md`。
- **2025-03-03** 快捷抓图优化：拖入已在监听目录的图片时只置顶不复制；新拖入的图片增加落下动画（从上方落定、缩放收敛）。涉及：`src/image_ops.rs`、`src/image_card.rs`、`src/app.rs`、`docs/INTERACTIONS.md`。
- **2025-03-03** 快捷抓图：从浏览器或任意软件将图片拖入窗口即可保存到监听目录并加入堆叠。支持本地文件（复制保留格式）与纯字节数据（解码保存为 PNG）。涉及：`src/image_ops.rs`、`src/app.rs`、`docs/INTERACTIONS.md`。
- **2025-03-03** 建立 Agent 协作与留痕流程：新增 `AGENTS.md`、`.cursor/rules/deckref-conventions.mdc`、`docs/WORKFLOW.md`、`CHANGELOG.md`。约定「先查文档、改完更新 CHANGELOG、交互以 INTERACTIONS.md 为准」。

---

## 已发布

（此前若有版本发布，可在此按版本/时间罗列；当前仅保留「未发布」即可。）
