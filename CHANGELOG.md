# Changelog

所有 notable 的功能与重要修复都会记录在此，便于多 Agent 与人类开发者快速了解近期变更。

格式：**日期** + 一句话描述 + 可选「涉及：文件/模块」。

---

## 未发布

- **2025-03-03** 修复：窗口变窄时面包屑未自动省略左侧；并修复地址栏与「最近访问」「按大小/按修改时间/清空」垂直不对齐。地址栏区域改为与工具栏同高（`TOP_BAR_HEIGHT`）、顶对齐（`Align::Min`），右侧预留增至 280 以容纳排列/清空按钮并保证窄窗下折叠。涉及：`src/app.rs`（地址栏 allocate 高度与对齐、RESERVE_LEFT_RIGHT、空白区高度）。
- **2025-03-03** 易用性：地址栏在窗口拉长后自动展开——当可用宽度足以显示完整路径时，面包屑自动显示全部段落，不再显示省略号。涉及：`src/app.rs`（`breadcrumb_collapse_start` 先判完整宽度再决定是否折叠）、`docs/INTERACTIONS.md`。
- **2025-03-03** 易用性：面包屑地址栏过长时（大于窗口宽度）左侧自动折叠为省略号「…」，保留右侧路径可见（参考 Win11 资源管理器）；点击「…」可展开被省略的上级路径并跳转。涉及：`src/app.rs`（`breadcrumb_collapse_start`、地址栏渲染）、`docs/INTERACTIONS.md`。
- **2025-03-03** 易用性：默认模式与画板模式的文件夹地址同步——切换目录或切换模式时同时写入 `last_path.txt` 与 `last_board_path.txt`，切到默认或画板均打开同一文件夹；两模式仍模块化（各自存储文件与布局逻辑）。涉及：`src/window_state.rs`（`save_last_folder_sync`）、`src/app.rs`（`apply_watch_path`、`switch_mode`、`Drop`）、`docs/INTERACTIONS.md`。
- **2025-03-07** 画板模式专用视口逻辑（复刻 Photoshop，仅画板、不影响默认/灵感）：维护三个坐标系（世界坐标、视口状态 scale/offset、屏幕坐标）；缩放以鼠标为中心、公式 `offset += (鼠标屏 - 中心)×(1/s1 - 1/s2)` 不跳动；平移为中键或空格+左键拖动，Delta 累加至 offset；窗口 resize 时补偿中心位移到 offset，画布中心不跳动。独立状态 `board_view_scale` / `board_view_offset` / `last_board_screen_center`，一览/阵列在画板下重置 board_view_*。涉及：`src/app.rs`（画板视口字段、resize 补偿、pan/zoom 分支、空格+左键不触发长按）、`docs/INTERACTIONS.md`（画板视口小节）。
- **2025-03-07** 画板模式与默认/灵感解耦 + 画板下阵列修复：确认画板逻辑仅在 `mode == AppMode::Board` 时生效（board_scale_u、board_saved_world_pos、参考空间读写）；默认与灵感模式始终使用 stack + random_offset，不读 board_saved_world_pos。画板模式下点击「阵列」或首帧无布局时：在 `grid_arrange` 中为每卡写入 `board_saved_world_pos = Some(world_center)` 并设置 `layout_reference_size = stack_layout.available_size`，使阵列显示与保存一致且随窗口缩放。涉及：`src/app.rs`（card_display 注释、grid_arrange 内 Board 分支、首帧阵列条件）。
- **2025-03-07** 画板模式缩放与切文件夹后间距修复：布局以「参考画布尺寸」下的绝对世界坐标保存（`reference_available_size` + 每卡 offset/scale 为参考空间）；每帧用 `board_scale_u = 当前画布/参考画布` 统一缩放位置与缩放，绕画布原点整体缩放，位置与间距比例一致。不再首帧 `take()` 掉 `layout_reference_size`，切到别文件夹改窗口再切回后，相对间距随当前窗口正确缩放。`ImageCard` 新增 `board_saved_world_pos`；加载时 `use_board_absolute` 只写参考空间；保存与放下、重排时在画板模式下读写参考空间。涉及：`src/layout.rs`（apply 分 use_board_absolute 两路、layout_from_cards 用 board_saved_world_pos）、`src/image_card.rs`（board_saved_world_pos）、`src/app.rs`（board_scale_u、card_display 预计算、命中/R-Tree/绘制/拿起/放下统一用 card_display，放下与重排写参考空间）。
- **2025-03-07** 画板模式布局随窗口比例缩放（初版）：保存布局时写入 `reference_available_size`，加载后根据当前画布尺寸比例缩放每卡的 offset 与 display_scale。涉及：`src/layout.rs`、`src/app.rs`（layout_reference_size、保存时传入 last_stack_size）。（后续由「参考空间 + 每帧 scale_u」方案替代，见上一条。）
- **2025-03-07** 0.3.0 Phase 1：画板模式与布局持久化。新增第三模式「画板」，左侧边栏三入口；画板模式下画布与当前目录绑定，在该目录内写入 `.deckref_layout.json`（卡片 position/scale/order），进入/切换目录时若存在则加载；退出、切换目录或约每 5 秒防抖写回；成功写入的目录路径追加到配置目录 `layout_dirs.txt` 供卸载时安全清理。画板模式支持地址栏与最近到访、后退/前进，上次画板目录持久化于 `last_board_path.txt`。涉及：`src/mode.rs`（AppMode::Board）、`src/layout.rs`（新建）、`src/window_state.rs`（last_board_path、append_layout_dir）、`src/app.rs`（画板加载/保存/防抖、缓存）、`docs/INTERACTIONS.md`、`CHANGELOG.md`。
- **2025-03-07** 图标可来自 assets：若存在 `assets/icon.png` 或 `assets/main icon.png`，构建时从中生成 `wix/DeckRef.ico`（32×32/16×16）并嵌入 exe 与 MSI；否则仍使用内置程序绘制 logo。打包脚本版本号改为优先从 `cargo pkgid` 读取，保证与 Cargo.toml 一致。涉及：`build.rs`（`build_icon_from_png`、回退 `build_icon_fallback`）、`Cargo.toml`（build-dependencies image）、`scripts/build_windows.ps1`（版本解析）。
- **2025-03-07** Windows 图标注册：MSI 与 exe 均使用统一 logo。构建时 `build.rs` 生成 `wix/DeckRef.ico`（32×32/16×16）并用 **winres** 嵌入 exe，资源管理器/任务栏/快捷方式显示图标；WiX 中注册 **ARPPRODUCTICON** 与快捷方式 **Icon**，「添加/删除程序」与开始菜单显示 logo。涉及：`build.rs`、`Cargo.toml`（build、winres、ico）、`wix/DeckRef.wxs`（Icon、ARPPRODUCTICON、Shortcut Icon）、`scripts/build_windows.ps1`（复制 DeckRef.ico）、`docs/BUILD_WINDOWS.md`。
- **2025-03-05** Windows 打包 v0.1.4：产物改为 **名称+版本号** 文件夹 `DeckRef-0.1.4`，内含 exe、使用说明、config_readme、卸载.bat；可选生成 MSI（需安装 WiX Toolset v3.11）。卸载脚本保留「是否清理 DeckRef 产生的缓存」选项，选 N 则下次安装可继续使用。Cargo.toml 版本设为 0.1.4。涉及：`Cargo.toml`、`scripts/build_windows.ps1`、`scripts/install_readme_win.txt`、`docs/BUILD_WINDOWS.md`、`.gitignore`（DeckRef-*）。
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

### [0.3.0] - 2025-03-07

- **画板模式**：新增第三模式「画板」，左侧三入口；画板下画布与当前目录绑定，目录内 `.deckref_layout.json` 持久化卡片位置/缩放/顺序，进入或切换目录时自动加载，退出或约每 5 秒防抖写回；`last_board_path.txt`、`layout_dirs.txt` 记录画板目录与布局目录供清理。
- **画板视口**：画板专用缩放与平移（世界/视口/屏幕三坐标系），缩放以鼠标为中心不跳动，中键或空格+左键平移；窗口 resize 时补偿中心；一览/阵列在画板下重置视口；布局以参考画布尺寸保存，每帧按 `board_scale_u` 统一缩放。
- **地址栏**：面包屑过长时左侧折叠为「…」、点击可展开；窗口拉长后自动展开完整路径；与工具栏同高、顶对齐，窄窗下折叠；默认与画板模式文件夹地址同步（`last_path.txt` / `last_board_path.txt`）。
- **构建与图标**：`build.rs` 从 assets 生成 `wix/DeckRef.ico` 并嵌入 exe（winres）；打包脚本版本号从 `cargo pkgid` 读取；新增 `docs/PRODUCT_ROADMAP_0.3.md`。
- **依赖**：serde、serde_json（布局序列化）；build-dependencies：winres、ico、image。

（此前若有版本发布，可在此按版本/时间罗列。）

### [0.2.0] - 2025-03-05

- **Windows 打包**：产物文件夹改为 **DeckRef-{版本号}**（如 `DeckRef-0.2.0`），内含 exe、使用说明、config_readme、Uninstall.bat；可选生成 MSI 安装包（需安装 WiX Toolset v3.11，`wix/DeckRef.wxs`）。版本号优先从 `cargo pkgid` 读取，与 Cargo.toml 一致。
- **图标**：支持从 **assets/icon.png** 或 **assets/main icon.png** 在构建时生成 `wix/DeckRef.ico`（32×32/16×16）并嵌入 exe 与 MSI；若无则使用内置绘制 logo。`build.rs` + winres + ico。
- **界面**：底部栏右下角显示版本号（半透明）；左键点住图片时显示该图完整文件名。
- **阵列**：热力图式紧密排列，按原比例无缝、行内/行间无缝隙，行等高、按宽高比贪心分行，整体缩放填满可用区域。
- **安全**：新增 `SECURITY.md` 漏洞报告策略。

（此前若有版本发布，可在此按版本/时间罗列。）

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
