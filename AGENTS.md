# DeckRef — Agent 协作说明

本文档是 **所有 Agent 的入口**。新对话或新 Agent 参与开发时，请先阅读本文档和 `docs/` 下的相关文档，以保持上下文一致。

## 项目是什么

- **DeckRef**：Rust + egui 的桌面端图片堆叠/参考板应用。
- 技术栈：eframe 0.29、egui、notify（文件监听）、image（图像）、drag（拖拽）。
- 源码：`src/main.rs`、`app.rs`、`watcher.rs`、`image_ops.rs`、`image_card.rs`、`error.rs`、`mode.rs`（应用模式：默认/灵感，可扩展）。

## 必读文档（变更前先看）

| 文档 | 用途 |
|------|------|
| `docs/INTERACTIONS.md` | **交互设计单一来源**：鼠标/触控/滚轮/按钮等。改交互必先看并同步更新。 |
| `docs/SETTINGS.md` | **设置项单一来源**：所有可配置选项、默认值、存储格式。新增/修改设置必先看并同步更新。 |
| `docs/ARCHITECTURE_PERFORMANCE_REPORT.md` | 架构与性能关键节点，优化前参考。 |
| `docs/WORKFLOW.md` | 开发流程、留痕规范、多 Agent 协作方式。 |
| `docs/GITHUB_SUBMISSION.md` | **提交到 GitHub 的 Agent**：推送前预审、代码安全、强制流程，执行 push 前必读。 |
| `docs/TESTING.md` | 测试约定与 Cursor 用法：如何跑测试、写测试、提交前必跑。 |
| `docs/AGENT_ROLES.md` | **各角色记忆**：7 个专项角色的职责、涉及模块、近期完成。新对话指定角色时让 Agent 先读对应小节 + CHANGELOG。 |

**可选参考**（不参与开发约束）：`docs/VISION.md` — 项目愿景与产品方向，仅在做产品/交互取舍时参考，不以之为硬性规范。

## 让彼此知道「改了什么」

1. **代码与文档同步**：改交互 → 更新 `docs/INTERACTIONS.md`；**新增或修改设置项** → 更新 `docs/SETTINGS.md`；改架构/性能相关 → 必要时更新 `docs/ARCHITECTURE_PERFORMANCE_REPORT.md`。
2. **功能留痕**：完成功能或重要修复后，在 **`CHANGELOG.md`** 中追加一条记录（日期 + 简述 + 涉及文件/模块）。
3. **规则约束**：`.cursor/rules/` 中的规则会对所有会话生效，请遵守其中关于「先查文档、后改代码、改完留痕」的约定。

## 快速定位

- 交互常量/逻辑：`src/image_card.rs`、`src/app.rs`
- 应用状态与主循环：`src/app.rs`
- 文件监听：`src/watcher.rs`
- 图像加载/尺寸：`src/image_ops.rs`

## 分支约定

- **美术效果**（视觉效果、渲染、阴影、动画等）在分支 **`feature/art-effects`** 上开发；相关 Agent 或开发者在做该类改动时请先切换到该分支再改，合并前需通过预审（见 `docs/GITHUB_SUBMISSION.md`）。
- 主分支（如 `main`）用于稳定/已合并功能；其它专项分支可按需在本文档或 `docs/WORKFLOW.md` 中补充说明。

## 角色记忆（长时间开发后接上上下文）

- 7 个专项角色（基础功能开发者、代码提交者、代码性能优化者、项目设置键专项设计者、易用性功能开发者、平面美术开发者、测试工程师）的「职责 + 近期做过什么」写在 **`docs/AGENT_ROLES.md`**。
- 新开对话并指定「你是 XXX 角色」时，在首条消息中让 Agent **先看 @docs/AGENT_ROLES.md 里该角色小节和 @CHANGELOG.md 最近几条**，即可加载该角色的「记忆」。
- 该角色完成功能后，除更新 CHANGELOG 与相关 doc 外，在 AGENT_ROLES.md 对应角色的「近期完成」中补一行或引用 CHANGELOG。详见 `docs/AGENT_ROLES.md` 第四节。

---

*维护者：开发时优先查阅并更新上述文档，保证后续 Agent 与人类开发者都能快速对齐。*
