# DeckRef 开发流程与留痕指南

面向 Cursor 新手与多 Agent 协作：如何让「很多 Agent 开发后的功能变化彼此都知道」，以及「完成功能后如何留痕」。

---

## 一、核心思路：文档是唯一事实来源

- **问题**：不同对话、不同 Agent 之间没有共享记忆，容易重复改、改冲突、不知道别人改过什么。
- **做法**：把「项目约定」和「最近做了什么」写进**仓库里的文档**，让每个新对话、新 Agent 都**先读这些文档**再动手。

本仓库的约定：

| 文档 | 谁用 | 作用 |
|------|------|------|
| **AGENTS.md** | 所有 Agent / 新人 | 入口：项目是什么、必读文档列表、如何留痕。 |
| **.cursor/rules/** | Cursor 自动注入 | 规则：先查文档、改完必更 CHANGELOG、交互与 docs 一致。 |
| **docs/INTERACTIONS.md** | 做交互的人/Agent | 交互的单一来源，改交互必同步。 |
| **docs/ARCHITECTURE_PERFORMANCE_REPORT.md** | 做性能/架构的人/Agent | 架构与性能关键点。 |
| **CHANGELOG.md** | 所有人/Agent | 功能与重要修复的留痕，按时间追加。 |
| **docs/WORKFLOW.md** | 人类开发者 | 本文档：流程说明与 Cursor 技巧。 |

---

## 二、让多个 Agent「彼此知道」变更

### 1. 用 AGENTS.md 做统一入口

- 每个新对话或新 Agent 被要求「先看 AGENTS.md」。
- AGENTS.md 里写清：项目简介、必读文档、留痕规矩、常用文件位置。
- 这样无需依赖对话历史，只要读仓库就能对齐上下文。

### 2. 用 Cursor Rules 统一行为

- 在 **`.cursor/rules/`** 里放 `deckref-conventions.mdc`，并设为 **alwaysApply: true**。
- 规则里写明：改交互前读 INTERACTIONS.md、改完更新文档、完成功能后写 CHANGELOG。
- 所有会话都会自动带上这些规则，所以所有 Agent 都会按同一套「先查 doc、再改代码、改完留痕」执行。

### 3. 用 CHANGELOG 记录「做了什么」

- 每完成一个功能或重要修复，在 **CHANGELOG.md** 里追加一条（日期 + 简述 + 可选：涉及文件）。
- 后续任何 Agent 或人，只要打开 CHANGELOG 就能看到近期变更，不必翻 git 历史或猜。

### 4. 文档与代码同步

- 交互行为以 **docs/INTERACTIONS.md** 为准；代码和文档不一致时，以文档为准并改代码或更新文档。
- 这样「后来者」读文档就能知道当前交互设计，不会基于过期理解乱改。

---

## 三、完成功能后如何留痕（标准步骤）

1. **写代码**，完成功能或修复。
2. **更新相关文档**  
   - 若改了交互 → 更新 `docs/INTERACTIONS.md`（操作/条件/效果表等）。  
   - 若改了架构/性能关键路径 → 视情况更新 `docs/ARCHITECTURE_PERFORMANCE_REPORT.md`。
3. **在 CHANGELOG.md 留一条**  
   - 在「未发布」或对应版本下追加：  
     `- **YYYY-MM-DD** 一句话描述。涉及：文件或模块。`
4. （若使用 Git）**提交**：一次提交可包含「代码 + 文档 + CHANGELOG」，提交信息里可简要复述变更。

这样，之后任何 Agent 或开发者都能通过 CHANGELOG + 文档 快速知道「最近改了什么、约定是什么」。

---

## 四、Cursor 使用技巧（新手向）

### 1. 主动 @ 文档，减少偏差

- 新开一个功能前，在对话里写：`请先看 @AGENTS.md 和 @docs/INTERACTIONS.md`。
- 或具体到文件：`按 @docs/INTERACTIONS.md 的规范加一个 xxx 操作`。  
这样模型会把文档纳入上下文，按文档约定改。

### 2. 用 Rules 固化习惯

- 项目级规则：`.cursor/rules/*.mdc`（已为 DeckRef 配好「先查 doc、改完留痕」）。
- 需要时可为某类文件加规则，例如 `globs: **/image_card.rs`，只在该类文件被打开时生效。

### 3. 一个功能一个对话 / 一个分支

- 一个大功能单独开一个 Chat 或分支，做完后：更新文档 + 写 CHANGELOG + 再开新对话做下一项。
- 新对话开头说一句「请先读 AGENTS.md 和 CHANGELOG 最近几条」，就能延续上下文。

### 4. 用 CHANGELOG 做「交接」

- 换人、换 Agent、隔几天再开发：先看 **CHANGELOG.md** 最近几条 + **AGENTS.md**，再看具体 docs，能最快恢复「项目最近在做什么、约定是什么」。

### 5. 少依赖「记忆」，多依赖「文档」

- 不要指望 Agent 记得很久以前的对话；把重要结论、约定、接口变更都写进 `docs/` 或 CHANGELOG，后续所有人都能复用。

---

## 五、小结

- **多 Agent 彼此知道变更**：AGENTS.md 入口 + Cursor Rules 统一行为 + 文档（INTERACTIONS/架构）为单一来源 + CHANGELOG 记录每次功能/修复。
- **功能完成后留痕**：更新对应 docs → 在 CHANGELOG.md 追加一条 → 提交时带上文档与 CHANGELOG。
- **新手技巧**：多用 @ 引用文档、用 Rules 固化流程、一功能一留痕、新对话先读 AGENTS + CHANGELOG。

## 六、分支使用：美术效果分支

- **分支名**：`feature/art-effects`（专门做视觉效果、渲染、阴影、动画等）。
- **创建并切换**（在项目根目录执行）：
  ```bash
  git checkout -b feature/art-effects
  ```
  若当前不在 `main`，可先 `git checkout main` 再执行上式。
- **日常使用**：做美术/视觉效果相关改动前先 `git checkout feature/art-effects`，在该分支上提交；审完后再合并到 `main`（如通过 PR 或本地 `git merge feature/art-effects`）。
- **与 Agent 协作**：在 AGENTS.md 的「分支约定」中已注明美术效果在 `feature/art-effects`，Agent 在做该类改动时应先切换到此分支再改。

维护好这套流程，后续无论是你还是其他 Agent，都能快速对齐并延续开发。
