# Changelog

所有 notable 的功能与重要修复都会记录在此，便于多 Agent 与人类开发者快速了解近期变更。

格式：**日期** + 一句话描述 + 可选「涉及：文件/模块」。

---

## 未发布

- **2025-03-03** 最近到访列表持久化：关闭软件时将「最近到访」的 5 个文件夹路径写入配置目录 `recent_folders.txt`（每行一路径），下次启动时从该文件加载并合并当前目录，下拉菜单中继续显示历史地址。涉及：`src/window_state.rs`（`load_recent_folders`、`save_recent_folders`）、`src/app.rs`（`new()` 中加载、`Drop` 中保存）、`docs/INTERACTIONS.md`。
- **2025-03-03** 导航栏「最近到访」下拉：默认模式地址栏最右侧增加 ▾ 按钮，点击显示最近到访的 5 个文件夹，点击任一项快速切换；该 5 个文件夹的卡片做缓存（`recent_folder_cards`），再次点选时直接恢复、无需重新加载；新访问顶替最旧项。涉及：`src/app.rs`（`recent_folders`、`recent_folder_cards`、`push_recent_folder`、`apply_watch_path` 缓存逻辑）、`docs/INTERACTIONS.md`。
- **2025-03-03** 地址栏支持输入/粘贴路径：默认模式下面包屑末端增加「▤ 输入或粘贴路径」，点击后进入单行编辑框，可输入或粘贴路径，Enter 确认跳转（有效目录），Esc 取消。涉及：`src/app.rs`（`address_bar_editing` / `address_bar_edit_text`）、`docs/INTERACTIONS.md`。
- **2025-03-03** 默认模式地址栏易用性：根目录改为「此台电脑」（Windows 下点击可选盘符）；增加 ◀ 后退 / ▶ 前进 导航；**启动时恢复上次关闭时的文件夹**（持久化于配置目录 `last_path.txt`）。涉及：`src/app.rs`、`src/window_state.rs`（`load_last_default_path` / `save_last_default_path`）、`docs/INTERACTIONS.md`。
- **2025-03-03** 默认模式可交互地址栏：监听路径改为 Windows 风格面包屑导航（如 `C: › Users › … › Ref`），点击段落可切换到该目录；当前目录旁 ▾ 可展开子目录列表并快速切换。涉及：`src/app.rs`（`path_breadcrumbs`、`navigate_watch_path`）、`docs/INTERACTIONS.md`。
- **2025-03-03** 修复拖出又拖回仍创建副本：Windows 下同一次拖放会同时提供 CF_HDROP（路径）与 FileContents（字节），此前两者都处理导致再写一份 `drop_*.png`。现改为若 HDROP 已返回路径则不再处理 FileContents，避免重复保存。涉及：`src/win_drop.rs`。
- **2025-03-03** 启动时异步加载：目录扫描、dimensions、metadata 在后台线程（`deckref-loader`）完成，通过 mpsc 将 `(path, dimensions, metadata)` 发往主线程，主线程每帧 `poll_loading_cards` 消费并调用 `ImageCard::from_path_with_metadata` 构造卡片，避免启动卡顿；合并 metadata 为单次 IO（`list_existing_image_paths_with_metadata`），spawn 失败时回退到同步 `load_existing_images`。涉及：`src/watcher.rs`、`src/image_card.rs`、`src/app.rs`、`docs/ARCHITECTURE_PERFORMANCE_REPORT.md`。
- **2025-03-03** 防止循环保存：从窗口拖出的图若未保存到别处又拖回窗口时，只置顶不复制；通过记录最近拖出的路径（规范路径、上限 20）在拖入时判定。涉及：`src/app.rs`（`recent_drag_out_paths`）、`docs/INTERACTIONS.md`。
- **2025-03-03** 左侧模式按钮垂直居中：两个模式按钮置于左侧边栏垂直居中，避免被导航栏挡住。涉及：`src/app.rs`、`docs/INTERACTIONS.md`。
- **2025-03-03** 模式切换性能：切换模式时保留另一模式的卡片列表与纹理缓存，再次切回时直接恢复、不重扫磁盘、不重新解码图片，减少卡顿。涉及：`src/app.rs`（`cached_default_cards` / `cached_inspiration_cards`）、`docs/INTERACTIONS.md`。
- **2025-03-03** 灵感模式（Windows）：打开灵感模式时默认展示 **Windows 最近使用的文件**中的图片（最多 30 张），从 `%APPDATA%\Microsoft\Windows\Recent` 解析 .lnk 获取本地图片路径；保存的图片仍写入 Ref/Inspiration 并加入列表。涉及：`src/win_recent.rs`、`lnk` 依赖、`src/app.rs`、`docs/INTERACTIONS.md`。
- **2025-03-03** 新增设置文档 `docs/SETTINGS.md`：记录所有可配置选项、默认值、存储格式与维护约定；AGENTS.md 与项目约定中补充「新增/修改设置时须同步更新 SETTINGS.md」。涉及：`docs/SETTINGS.md`、`AGENTS.md`、`.cursor/rules/deckref-conventions.mdc`。
- **2025-03-03** 灵感模式：新增「默认」与「灵感」两种模式，左侧边栏两个圆角小方块切换。灵感模式监听 `Pictures/Ref/Inspiration`，在该模式下保存的图片放入该目录；模式模块化（`src/mode.rs`）便于后续扩展更多模式。涉及：`src/mode.rs`、`src/app.rs`、`src/lib.rs`、`docs/INTERACTIONS.md`。
- **2025-03-03** 设置持久化：新增轻量设置模块，支持「反转平移方向」与「贴图拖拽出窗口」的保存与恢复；设置保存在配置目录 `settings.txt`（与窗口状态同目录），关闭再打开软件后自动加载。涉及：`src/settings.rs`、`src/window_state.rs`、`src/app.rs`、`src/lib.rs`、`docs/INTERACTIONS.md`。
- **2025-03-03** 易用性：记住上次关闭时的窗口大小，下次打开按相同尺寸恢复。涉及：`src/window_state.rs`、`src/main.rs`、`src/app.rs`、`docs/INTERACTIONS.md`。
- **2025-03-03** 同步代码至 GitHub：Windows OLE 拖放（Chrome/Edge 网页图拖入）、剪贴板粘贴图片（Ctrl+V）、`win_drop` 模块与文档。涉及：`src/win_drop.rs`、`src/app.rs`、`src/main.rs`、`src/image_ops.rs`、`arboard`、`docs/FEATURE_CHROME_DROP_IMPL.md`、`docs/CHROME_DRAG_DROP_REPORT.md`。
- **2025-03-03** Chrome 拖入备用方案：支持 Ctrl+V（Mac：Cmd+V）从剪贴板粘贴图片并保存到监听目录，不涉及额外权限；从 Chrome 拖入无效时可用「复制图片 → 粘贴」。涉及：`arboard`、`src/image_ops.rs`、`src/app.rs`、`docs/INTERACTIONS.md`。
- **2025-03-02** 同步代码至 GitHub：提交拖放、快捷抓图、监听与文档等改动；新增 `README.md`，完善 `.gitignore`（排除 `.cursor/`）。仓库：https://github.com/Xiangbao-pig/Deckref
- **2025-03-03** 拖出触发改为「指针离开窗口边缘」：拿起后仅在鼠标移出窗口边界时转为系统文件拖放，更符合直觉。涉及：`src/app.rs`、`docs/INTERACTIONS.md`。
- **2025-03-03** 拖放修复与拖出：窗口启用 `with_drag_and_drop(true)`，从浏览器拖入图片不再显示禁止标志；长按拿起后移动超过约 18px 会发起系统文件拖放，可拖到文件夹或其它软件。涉及：`src/main.rs`、`src/app.rs`、`docs/INTERACTIONS.md`。
- **2025-03-03** 快捷抓图优化：拖入已在监听目录的图片时只置顶不复制；新拖入的图片增加落下动画（从上方落定、缩放收敛）。涉及：`src/image_ops.rs`、`src/image_card.rs`、`src/app.rs`、`docs/INTERACTIONS.md`。
- **2025-03-03** 快捷抓图：从浏览器或任意软件将图片拖入窗口即可保存到监听目录并加入堆叠。支持本地文件（复制保留格式）与纯字节数据（解码保存为 PNG）。涉及：`src/image_ops.rs`、`src/app.rs`、`docs/INTERACTIONS.md`。
- **2025-03-03** 建立 Agent 协作与留痕流程：新增 `AGENTS.md`、`.cursor/rules/deckref-conventions.mdc`、`docs/WORKFLOW.md`、`CHANGELOG.md`。约定「先查文档、改完更新 CHANGELOG、交互以 INTERACTIONS.md 为准」。

---

## 已发布

（此前若有版本发布，可在此按版本/时间罗列；当前仅保留「未发布」即可。）
