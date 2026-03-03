# DeckRef 功能记录：从浏览器拖入图片（FileGroupDescriptorW + FileContents）

**状态**：已实现并确认可用  
**平台**：仅 Windows  
**场景**：从常见浏览器将网页上的图片拖入 DeckRef 窗口，与从资源管理器拖入本地文件一致，图片会保存到监听目录并加入堆叠。

**支持的浏览器**（按 OLE 格式兼容，无单独适配）：

- **Chromium 内核**：Chrome、Microsoft Edge、Brave、Opera、Vivaldi 等（网页图片多提供 FileGroupDescriptorW + FileContents）
- **IE（Internet Explorer）**：在提供相同或兼容 OLE 格式时可正常拖入
- **其他**：凡在拖放时提供 CF_HDROP（真实路径）或 FileGroupDescriptorW + FileContents（虚拟文件）的源，均可支持

---

## 实现概要

- **问题**：多数浏览器从网页拖出图片时不提供 CF_HDROP（文件路径），而提供虚拟文件格式 FileGroupDescriptorW + FileContents；仅支持 CF_HDROP 的窗口会显示「禁止」光标。
- **做法**：在 Windows 上自实现 OLE 拖放目标（IDropTarget），同时支持 **CF_HDROP** 与 **FileGroupDescriptorW/FileContents**，不区分具体浏览器，按数据格式接收；虚拟文件内容写入监听目录后走统一「拖入文件」逻辑。

## 涉及模块与要点

| 项目 | 说明 |
|------|------|
| **src/win_drop.rs** | Windows 专用模块：IDropTarget 实现、CF_HDROP 与 FileGroupDescriptorW/FileContents 解析、虚拟文件写入监听目录、OleInitialize、RegisterDragDrop/RevokeDragDrop |
| **src/app.rs** | 首帧再注册：`win_drop_pending_hwnd` 在首帧 run_ui 时调用 `register_drop_target`；每帧 drain `win_drop_rx` 后交 `handle_dropped_path` 处理 |
| **src/main.rs** | 从 CreationContext 取窗口句柄，调用 `app.set_win_drop_pending_hwnd(hwnd)`，不在此处注册 |
| **src/image_ops.rs** | `unique_path_for_drop(dest_dir, index)` 为虚拟文件生成唯一保存路径 |

## 关键实现细节

1. **注册时机**：在**首帧** run_ui 中再调用 `register_drop_target`，避免在 app 创建时注册被 winit 后续覆盖。
2. **替换已有目标**：调用 `RegisterDragDrop` 前先对该 HWND 执行 `RevokeDragDrop(hwnd)`，解除 winit 已注册的拖放目标，再注册本应用实现（否则会得到 0x80040101 DRAGDROP_E_ALREADYREGISTERED）。
3. **COM**：在 `register_drop_target` 内调用 `OleInitialize(None)`。
4. **虚拟文件**：支持 TYMED_ISTREAM 与 TYMED_HGLOBAL；单文件大小上限 50MB；保存路径使用 `unique_path_for_drop`，格式为 `drop_时间戳_序号.png`。
5. **与现有逻辑统一**：CF_HDROP 路径与 FileContents 写入后的路径均通过同一 channel 送入 app，由 `handle_dropped_path` 处理（已在监听目录则置顶，否则复制到监听目录并加入 `recent_drop_paths` 用于落下动画）。

## 依赖（Cargo.toml）

- `[target.'cfg(windows)'.dependencies]`：`windows`（Ole/Com/Shell/Memory/DataExchange/SystemServices 等）、`raw-window-handle`。

## 参考

- 技术背景与格式说明（为何浏览器多提供虚拟文件、与 CF_HDROP 区别）：`docs/CHROME_DRAG_DROP_REPORT.md`
- 性能与架构观察：`docs/ARCHITECTURE_PERFORMANCE_REPORT.md`

---

*本文档为功能实现记录，便于后续维护与需求迭代。*
