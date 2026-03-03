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

## 今日开发进展（2025-03-03）

- **代码同步至 GitHub**：将本次功能与文档推送到 [Xiangbao-pig/Deckref](https://github.com/Xiangbao-pig/Deckref)。
- **本次提交包含**：
  - **Windows 浏览器拖入**：自实现 OLE 拖放目标（`src/win_drop.rs`），支持 **CF_HDROP** 与 **FileGroupDescriptorW/FileContents**，从 Chrome、Edge 等网页拖入图片即可保存到监听目录并加入堆叠；技术说明见 `docs/FEATURE_CHROME_DROP_IMPL.md`、`docs/CHROME_DRAG_DROP_REPORT.md`。
  - **剪贴板粘贴**：支持 **Ctrl+V**（Mac：Cmd+V）从剪贴板粘贴图片并保存到监听目录（依赖 `arboard`），作为浏览器拖入无效时的备用方案。
  - **依赖**：新增 `arboard`；Windows 目标下新增 `windows`、`raw-window-handle`（OLE/COM 拖放）。
  - **文档**：更新 `docs/INTERACTIONS.md`、`docs/WORKFLOW.md`、`AGENTS.md`。

更细的变更按日期见 [CHANGELOG.md](CHANGELOG.md)。
