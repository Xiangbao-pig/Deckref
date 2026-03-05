# DeckRef

**当前版本 0.1.3** — Rust 桌面应用：图片堆叠浏览与快捷抓图（eframe + egui + 文件监听）。

- **监听目录**：指定文件夹，新放入的图片自动加入堆叠。
- **双模式**：**默认**（自选监听路径）与 **灵感**（Windows 下展示「最近使用的文件」中的图片，保存到 `Pictures/Ref/Inspiration`）；切换时保留另一模式缓存，不重扫磁盘。
- **地址栏**：面包屑导航（点击段落跳转）、▾ 子目录与 **最近到访** 下拉（5 条持久化），支持 **输入/粘贴路径** 跳转；**后退/前进** 导航；启动时**恢复上次关闭时的文件夹**。
- **拖入即存**：从浏览器或任意软件将图片拖入窗口，保存到监听目录并加入堆叠（本地文件复制保留格式，纯数据保存为 PNG）；Windows 下支持 Chrome/Edge 网页图 OLE 拖放。
- **剪贴板**：**Ctrl+V**（Mac：Cmd+V）粘贴图片并保存，可作为浏览器拖入的备用。
- **拖出到系统**：长按拿起卡片后，将指针移出窗口边缘即可拖到文件夹或其它软件；拖出又拖回仅置顶不复制。
- **设置持久化**：反转平移方向、贴图拖拽出窗口等保存到配置目录，窗口尺寸与上次路径一并恢复。
- **启动**：目录扫描与元数据在后台线程异步加载，避免卡顿。
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
| [docs/SETTINGS.md](docs/SETTINGS.md) | 设置项说明与存储约定 |
| [docs/BUILD_WINDOWS.md](docs/BUILD_WINDOWS.md) | Windows 打包与分发 |
| [docs/BUILD_MAC.md](docs/BUILD_MAC.md) | macOS 运行与打包 |

更细的变更按日期见 [CHANGELOG.md](CHANGELOG.md)。

## 许可证

本项目采用 [MIT 许可证](LICENSE)。
