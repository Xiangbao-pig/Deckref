# DeckRef

Rust 桌面应用：图片堆叠浏览与快捷抓图（eframe + egui + 文件监听）。

- **监听目录**：指定文件夹，新放入的图片自动加入堆叠。
- **拖入即存**：从浏览器或任意软件将图片拖入窗口，保存到监听目录并加入堆叠（本地文件复制保留格式，纯数据保存为 PNG）。
- **拖出到系统**：长按拿起卡片后，将指针移出窗口边缘即可拖到文件夹或其它软件。
- **交互**：悬停放大、中键平移、滚轮缩放视图、长按拿起/松手放下、单卡滚轮缩放、一览打散等。详见 [docs/INTERACTIONS.md](docs/INTERACTIONS.md)。

## 构建与运行

```bash
cargo run --release
```

启动时需指定监听目录（或通过后续设置选择）。

## 文档

| 文档 | 说明 |
|------|------|
| [CHANGELOG.md](CHANGELOG.md) | 功能与修复记录 |
| [docs/INTERACTIONS.md](docs/INTERACTIONS.md) | 交互方式说明 |
| [docs/WORKFLOW.md](docs/WORKFLOW.md) | Agent 协作与开发流程 |
| [AGENTS.md](AGENTS.md) | Agent 使用约定 |

## 今日开发进展（2025-03-02）

- **代码同步至 GitHub**：将当前功能与文档完整推送到私有仓库 [Xiangbao-pig/Deckref](https://github.com/Xiangbao-pig/Deckref)。
- **本次提交包含**：
  - 拖放与拖出：窗口支持接收外部拖入、长按拿起后移出窗口边缘触发系统拖放。
  - 快捷抓图：拖入图片保存到监听目录（本地文件复制、纯数据保存为 PNG）；已在目录内的图仅置顶不复制；新加入图片带落下动画。
  - 依赖：新增 `rand`，其它见 `Cargo.toml` / `Cargo.lock`。
  - 文档：`CHANGELOG.md`、`docs/`（INTERACTIONS、WORKFLOW、VISION 等）、`AGENTS.md`；`.gitignore` 排除 `target`、`.cursor`、敏感文件等。

更细的变更按日期见 [CHANGELOG.md](CHANGELOG.md)。
