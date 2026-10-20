# Changelog

所有 notable 的功能与重要修复都会记录在此，便于多 Agent 与人类开发者快速了解近期变更。

格式：**日期** + 一句话描述 + 可选「涉及：文件/模块」。

---

## 未发布

- **2026-10-20** 大图缩略图：JPEG 在解码时按 IDCT 缩小、WebP 用 libwebp 按目标边长缩放，最长边超过 2048 时不再先得到整幅像素缓冲；PNG 等格式仍解码后再缩放。主线程回退只对 JPEG/WebP 做这档缩略图，其余超大图不在主线程完整解码。阈值仍是 2048 / 1200。涉及：`src/image_ops.rs`、`src/app.rs`。
- **2026-10-20** R-Tree：屏幕 AABB 没变时不再每帧 `bulk_load`。命中检测先查指针附近的包络，再做 `rotated_rect_contains`，仍然前端优先。拿起中的卡仍在树里，并照旧单独画在最上层。涉及：`src/app.rs`。
- **2026-10-20** 监听：Create/Modify/Remove 只把路径交给后台线程读尺寸和 metadata，主线程收到结果后用 `from_path_with_metadata` 建卡。防抖仍是 400ms，超过 `MAX_CARDS` 仍删第一张，拖入落下动画保留；切换文件夹会丢掉过期任务。涉及：`src/app.rs`。
- **2026-10-20** 重绘：只在动画还没收敛、目录/纹理/监听 IO 还没结束、拿起或拖动、或 Shift 后的剪贴板监听窗口内请求重绘；后台结果到达时唤醒界面。静止堆叠不再因为视角已经平移或缩放而空转。涉及：`src/app.rs`。
- **2025-03-03** 底部栏右下角显示版本号（半透明灰色）与左键点住图片时显示该图完整文件名。涉及：`src/app.rs`、`docs/INTERACTIONS.md`。
- **2025-03-02** 阵列（热力图式）紧密排列：阵列改为按原比例、无缝紧密排列（类似股市热力图）。行内与行间无缝隙，每行等高、按 aspect 贪心分行，整体缩放填满可用区域；内边距改为 4px。涉及：`src/app.rs`（`grid_arrange` 行式无缝排布、row_boundaries、display_scale 按矩形宽高计算）。
- **2025-03-05** R-Tree 视口查询框扩大：查询时不再只用屏幕框，改为向四周各扩充 500 像素（`VIEWPORT_QUERY_PADDING`）再查 R-Tree，预加载邻域内的卡，减少平移时 pop-in。涉及：`src/app.rs`、`docs/ARCHITECTURE_PERFORMANCE_REPORT.md`。
- **2025-03-05** 视口剔除保留 R-Tree：先用 R-Tree 查询与视口相交的卡，再仅对查询结果做 20% 可见面积判断，避免对 3000 张卡暴力遍历算面积。涉及：`src/app.rs`（恢复 `RTree::bulk_load`、`locate_in_envelope`，在查询结果上做 `intersection_area/area`）、`docs/ARCHITECTURE_PERFORMANCE_REPORT.md`。
- **2025-03-05** 视口剔除放宽为「80% 在屏外才剔除」：参考成熟参考图软件，仅当卡片可见比例 < 20% 时剔除（即 80% 以上在屏幕外）；使用 AABB 的 `intersection_area` / `area` 计算可见比例，移除 RTree 查询。涉及：`src/app.rs`（`VISIBLE_RATIO_THRESHOLD`、可见比例过滤）、`docs/ARCHITECTURE_PERFORMANCE_REPORT.md`。
- **2025-03-05** 渲染循环视口剔除：引入 **rstar** 存储每张卡在屏幕空间的 AABB，渲染前用 `ui.clip_rect().expand2(32)` 作视口，`RTree::bulk_load` + `locate_in_envelope` 得到可见卡索引；仅对可见卡调用 `ensure_texture` 与 `draw_card`，拿起中的卡始终参与 AABB 与绘制。主线程大图解码：确认 `ensure_texture` 在回退路径中仍用 `load_image_thumbnail`（>2048px），主线程绝不完整解码超大图。涉及：`Cargo.toml`（rstar）、`src/app.rs`（`CardScreenRect`、`screen_aabb_for_rotated_rect`、视口剔除与可见集）、`docs/ARCHITECTURE_PERFORMANCE_REPORT.md`。
- **2025-03-05** 纹理解码前后端分离：图片解码移至后台线程 `deckref-tex-loader`，主线程仅做 GPU 上传。`ensure_texture` 未命中时只将 path 发往后台并加入 pending，本帧返回 None；每帧 `poll_texture_results` 最多上传 5 张已完成解码的纹理，切换文件夹/清空时清空 pending。请求顺序改为堆叠前端优先（`cards.iter().rev()`），可见图先出现。spawn 失败时回退为同步解码。涉及：`src/image_ops.rs`（`load_image_rgba_for_texture`）、`src/app.rs`（`run_texture_loader_thread`、`texture_loading_tx/rx`、`pending_texture_paths`、`poll_texture_results`、`ensure_texture`）、`docs/ARCHITECTURE_PERFORMANCE_REPORT.md`。
- **2025-03-05** 纹理缓存清理：仅保留最近 **10 个文件夹**的图片纹理缓存，超过后自动淘汰最久未访问的文件夹下全部纹理，控制显存与内存；切换至未缓存文件夹或点击「清空」时清空缓存与作用域。涉及：`src/app.rs`（`texture_cache_scope`、`MAX_TEXTURE_CACHE_FOLDERS`、`ensure_texture` 按文件夹加入/淘汰、`apply_watch_path`/清空时清 scope）、`docs/ARCHITECTURE_PERFORMANCE_REPORT.md`。
- **2025-03-05** 高分辨率图缩略图加载：当图片最长边 > 2048px 时，堆叠预览使用缩略图（最长边 1200px、CatmullRom 缩放）而非原图，在 2K 屏上仍保持清晰的同时显著降低解码时间与显存占用（适合单反/50MB 级大图）。涉及：`src/image_ops.rs`（`load_image_thumbnail`、`THUMBNAIL_RESOLUTION_THRESHOLD`、`THUMBNAIL_MAX_LONGEST_EDGE`）、`src/app.rs`（`ensure_texture`）、`docs/ARCHITECTURE_PERFORMANCE_REPORT.md`。
- **2025-03-05** Windows 打包：新增卸载程序与配置说明。ANzhuang 内增加 **卸载.bat**（可选删除 %APPDATA%\DeckRef 后提示手动删除安装文件夹）、**config_readme.txt**（环境与配置说明：无需 .env、配置目录为 DeckRef）；修正使用说明与文档中配置路径为 `%APPDATA%\DeckRef`。涉及：`scripts/uninstall_win.bat`、`scripts/config_readme_win.txt`、`scripts/install_readme_win.txt`、`scripts/build_windows.ps1`、`docs/BUILD_WINDOWS.md`。
- **2025-03-05** Windows 打包：新增 `scripts/build_windows.ps1` 与 `docs/BUILD_WINDOWS.md`，一键将 release 版 `deckref.exe` 及可选 `assets`、使用说明放入 **ANzhuang** 文件夹，便于在 Windows 上安全分发与运行（版本 0.1.1）。涉及：`scripts/build_windows.ps1`、`scripts/install_readme_win.txt`、`docs/BUILD_WINDOWS.md`。
- **2025-03-04** macOS 支持与打包：评估并确认基本功能可在 Mac 上运行；在 `main.rs` 中为 macOS/Linux 增加 CJK 字体路径（苹方、华文黑体等），Windows 保持原有字体。新增 `scripts/build_mac.sh` 在 Mac 本机一键打出 `DeckRef.app` 并打包为 `DeckRef-mac.zip`，朋友解压后双击即可运行。新增 `docs/BUILD_MAC.md` 说明 Mac 运行评估与打包步骤。涉及：`src/main.rs`、`scripts/build_mac.sh`、`docs/BUILD_MAC.md`。
- **2025-03-04** 剪贴板截图仅 Shift 后保存：修复「浏览 QQ 群图片时自动保存所有群图」的 bug。改为**仅当按下 Shift 后的约 5 秒内**才把剪贴板新图当作截图保存到 Ref/Screenshots；未按 Shift 或超时后不保存，避免误存 QQ/浏览器等复制的图片。闲暇时仍会定期读剪贴板以更新内部哈希，进入监听窗口时不误存旧内容。涉及：`src/app.rs`（`shift_listening_until`、`poll_clipboard_screenshot`、`SHIFT_LISTENING_DURATION`）、`docs/INTERACTIONS.md`。
- **2025-03-04** 右键拖动桥接系统标题栏（SendInput + 后台线程）：Windows 上右键拖动超过阈值后，先 **SendInput 注入左键按下**，再 ReleaseCapture + WM_SYSCOMMAND SC_MOVE，主线程在系统模态拖动中阻塞；**后台线程**轮询 VK_RBUTTON，一旦松开则 **SendInput 左键松开**，系统结束拖动。由系统接管移动，无抖动。涉及：`src/win_move.rs`（SendInput LEFTDOWN/LEFTUP、GetAsyncKeyState 轮询线程）、`src/app.rs`（有 hwnd 时调用桥接，返回后重置手势）、`docs/INTERACTIONS.md`。
- **2025-03-04** 右键拖动移动窗口（桥接回退）：Windows 上桥接 SC_MOVE 在右键按下时系统不响应，导致窗口不移动；已改回始终使用每帧 OuterPosition，右键**按住并拖动**超过约 4 像素即可移动窗口。涉及：`src/app.rs`、`docs/INTERACTIONS.md`。（已被本次 SendInput 方案替代。）
- **2025-03-04** 预览专业格式图片：设置中新增「预览专业格式图片」选项；开启后监听与加载时包含 TGA、ICO、DDS、AVIF、EXR、HDR、PNM、QOI、FF、**PSD** 等扩展名，并集成 **psd** crate 解码 PSD 为 RGBA 预览。更改该设置后会重启 watcher 并重新加载当前目录。涉及：`src/image_ops.rs`（`get_image_extensions`、`is_image_path_with`、`load_psd`、`image_dimensions` 对 PSD）、`src/watcher.rs`（`extended_formats`）、`src/win_recent.rs`、`src/settings.rs`、`src/app.rs`、`docs/SETTINGS.md`、`docs/INTERACTIONS.md`、`Cargo.toml`（psd）。
- **2025-03-04** 运行时监听剪贴板截图：约每 0.8 秒检测剪贴板，若图片与上次不同则自动保存到 **Ref/Screenshots**，按 **screenshot-1.png、screenshot-2.png…** 依次命名；若当前为灵感模式，新截图立即加入堆叠并带落下动画。灵感模式合并展示 Windows 最近使用的图片与 Ref/Screenshots 中截图（按编号排序），总数上限 30。涉及：`src/image_ops.rs`（`next_screenshot_path`、`list_screenshot_paths_ordered`）、`src/app.rs`（`screenshot_dir`、`poll_clipboard_screenshot`、灵感加载合并截图目录）、`docs/INTERACTIONS.md`。
- **2025-03-04** 地址栏面包屑 Win11 风格重构：每级路径为独立按钮，悬停有浅灰背景（frame 启用）；每级右侧 › 可展开该层级子文件夹列表（`list_subdirs`，最多 80 个），无子目录时为不可点击 ›；点击地址栏右侧空白或 Ctrl+L（Mac Cmd+L）进入编辑模式（整栏变单行输入框），Enter 确认跳转，Esc 或失焦且路径未改时恢复面包屑；移除「▤ 输入或粘贴路径」按钮。涉及：`src/app.rs`（`list_subdirs`、面包屑与空白区逻辑）、`docs/INTERACTIONS.md`。
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

### [0.1.3] - 2025-03-05

- **纹理解码前后端分离**：图片解码移至后台线程 `deckref-tex-loader`，主线程仅做 GPU 上传；未命中时 path 发往后台、本帧返回 None，每帧最多上传 5 张完成解码的纹理；请求顺序堆叠前端优先。
- **视口剔除 + R-Tree**：使用 **rstar** 存储每卡屏幕 AABB，渲染前用视口（clip_rect 扩大 32px）做 `RTree::bulk_load` + `locate_in_envelope`；仅对可见卡调用 `ensure_texture` 与绘制；视口查询向四周扩充 500px 预加载邻域，可见比例 < 20% 才剔除。
- **依赖**：新增 `rstar`。

（此前若有版本发布，可在此按版本/时间罗列。）

### [0.1.2] - 2025-03-05

- **纹理缓存**：仅保留最近 10 个文件夹的图片纹理，超过后自动淘汰最久未访问的文件夹，控制显存与内存。
- **大图缩略图**：最长边 > 2048px 的图片堆叠预览使用缩略图（最长边 1200px），降低解码时间与显存，适合单反/大图。
- **Windows 打包**：`scripts/build_windows.ps1` 一键产出 ANzhuang 文件夹；新增卸载脚本与配置说明（`%APPDATA%\DeckRef`）；见 `docs/BUILD_WINDOWS.md`。
- **macOS 支持与打包**：main 中为 macOS/Linux 增加 CJK 字体路径；`scripts/build_mac.sh` 打出 `DeckRef.app` 并打包为 zip；见 `docs/BUILD_MAC.md`。

（此前若有版本发布，可在此按版本/时间罗列。）

### [0.1.1] - 2025-03-04

- **双模式**：默认 / 灵感（Windows 最近使用的文件、Ref/Inspiration）；模式切换保留缓存。
- **地址栏**：面包屑导航（Win11 风格、子目录 ›）、最近到访（5 条持久化）、Ctrl+L 编辑路径、后退/前进；启动恢复上次文件夹。
- **剪贴板截图**：运行时监听剪贴板，**仅 Shift 后约 5 秒内**将新图保存到 Ref/Screenshots（避免误存 QQ/浏览器）；灵感模式合并展示截图。
- **专业格式预览**：设置中「预览专业格式图片」可开启 TGA/ICO/DDS/AVIF/EXR/HDR/PNM/QOI/FF/**PSD** 等，PSD 由 `psd` crate 解码。
- **右键拖动移动窗口**：Windows 上右键拖动超阈值后 SendInput 桥接系统标题栏拖动，由系统接管无抖动（`src/win_move.rs`）。
- **设置与状态**：设置持久化（反转平移、拖拽出窗口、专业格式开关）；窗口尺寸与路径恢复；配置目录 `last_path.txt` / `recent_folders.txt` / `settings.txt`。
- **其它**：启动异步加载；拖出又拖回仅置顶；HDROP/FileContents 二选一防重复保存；Chrome/Edge OLE 拖入、Ctrl+V 粘贴；文档 `SETTINGS.md`、`AGENT_ROLES.md` 等。

（此前若有版本发布，可在此按版本/时间罗列。）
