# DeckRef 架构与性能观察报告

**角色**：性能/架构视角（不涉及功能逻辑修改）  
**范围**：当前代码库架构、数据流、潜在性能关键节点  
**目的**：为后续「关键节点」优化提供依据，本期不做具体优化。

---

## 一、技术栈与模块划分

| 层次     | 技术/模块        | 说明 |
|----------|------------------|------|
| 运行时   | Rust 2021        | 单二进制，无异步运行时 |
| UI/窗口  | eframe 0.29 + egui 0.29 | 即时模式 GUI，主线程驱动 |
| 文件监听 | notify + notify-debouncer-mini | 独立线程 + 400ms 防抖，mpsc 上报 |
| 图像     | image 0.25       | 解码/编码/尺寸查询，均在调用线程 |
| 拖拽     | drag 2           | 主线程发起系统拖放 |

**源码模块**（`src/`）：

- **main.rs**：入口、窗口选项、CJK 字体加载、`DeckRefApp` 创建
- **build.rs**（仅 Windows 构建）：从 `assets/icon.png` 或 `assets/main icon.png` 生成 `wix/DeckRef.ico` 并嵌入 exe（winres）；若无则内置绘制 logo。MSI 打包使用同一 ico。
- **app/mod.rs**（模块 `app`）：`DeckRefApp` 状态、UI 主循环 `run_ui`、事件轮询、纹理缓存；`CentralPanel` 内堆叠画布委托 **`app/ui_stack_canvas.rs`**
- **app/stack_viewport.rs**：中央区视口补偿、中键/空格平移、未拿起时滚轮缩放；`ViewTransform`（本帧世界↔屏幕快照，与拆分前「滚轮前捕获的 offset/scale」语义一致）
- **app/stack_cull_draw.rs**：空文件夹提示、首帧阵列、`card_display`、悬停命中、动画插值、R-Tree 视口剔除、可见纹理请求、卡片/拿起/框选绘制
- **app/stack_interaction.rs**：框选端点更新、长按拿起与拿起时滚轮/跟手、松手放下与框选/单击选中
- **app/ui_mode_sidebar.rs**、**app/ui_top_bar.rs**、**app/ui_bottom_bar.rs**：左侧模式条、顶栏地址栏与工具、底栏（`app` 子模块，可访问 `DeckRefApp` 私有字段）
- **app_shell_layout.rs**：顶栏/底栏/模式侧栏共用像素常量
- **app_nav.rs**（`crate` 内私有模块）：地址栏面包屑、盘符列表、折叠宽度计算
- **app_threads.rs**：后台卡片列表扫描（`deckref-loader`）、纹理解码线程（`deckref-tex-loader`）入口
- **app_viewport.rs**：视口剔除用 `CardScreenRect`、旋转矩形屏幕 AABB 计算
- **watcher.rs**：目录扫描、防抖监听、通过 channel 发送 `WatchEvent`
- **image_ops.rs**：`load_image`、`image_dimensions`、`save_image`、缩略图与后台解码（纯函数式接口，无内部缓存）
- **image_card.rs**：`ImageCard` 结构、堆叠布局、命中检测、绘制（mesh/阴影）、动画插值
- **error.rs**：统一错误类型与 `AppResult`
- **mode.rs**：应用模式（默认/灵感）
- **settings.rs**：设置结构体与持久化（settings.txt）
- **window_state.rs**：窗口位置/尺寸与最近路径持久化
- **Windows 专用**（`#[cfg(windows)]`）：**win_drop.rs**（OLE 拖放）、**win_move.rs**（右键拖动移动窗口）、**win_recent.rs**（最近使用的文件）

整体是「单主线程 UI + 一个监听线程 + 按需存在的加载线程与纹理解码线程」的架构；启动时不再阻塞主线程。

## 二、数据流与执行路径（与性能相关）

### 2.1 启动阶段

```
main
  → resolve_pictures_ref_dir()           // 目录解析/创建
  → eframe::run_native(..., create_app)
       → configure_cjk_fonts(cc)        // 读字体文件入内存，一次性
       → DeckRefApp::new(base_path)
            → mpsc::channel() + run_watcher(…)   // 启动监听线程
            → start_async_load()        // 见下（不再阻塞主线程）
```

**start_async_load()**（在 `App::new` 与切换模式无缓存时调用）：

1. 若存在未结束的上次加载，先 drop 对应 receiver 并 join 旧线程。
2.  spawn 后台线程 `deckref-loader`，传入当前 `mode` 与 `watch_path`。
3. 后台线程：
   - **默认模式**：`list_existing_image_paths_with_metadata(watch_path, MAX_CARDS)`（一次 read_dir + 每文件一次 metadata 用于排序与元数据，无重复 IO），对每条 `(path, file_size, modified)` 调用 `image_ops::image_dimensions(&path)`，通过 channel 发送 `(path, (w,h), (file_size, modified))`。
   - **灵感模式**（Windows）：`win_recent::list_recent_image_paths(max)`，对每条 path 读 metadata + dimensions，再发送；非 Windows 同默认模式逻辑、使用 `watch_path`。
4. 主线程每帧在 `run_ui` 中调用 `poll_loading_cards()`：`try_recv` 消费 channel，用 `ImageCard::from_path_with_metadata(path, intrinsic, file_size, modified)` 构造卡片并 push（不读 metadata，无重复 IO）；当 channel 关闭（Disconnected）时 join 加载线程。

**结论**：启动时主线程不再串行做目录遍历与每文件多次 IO；窗口可立即显示，卡片随加载完成逐步出现。同一文件的 metadata 仅在后台线程读取一次，主线程用 `from_path_with_metadata` 零 IO 构造卡片。spawn 失败时回退到同步 `load_existing_images()`。

### 2.2 每帧主循环（run_ui）

```
update() → run_ui(ctx, frame)
  → poll_watch_events()                  // try_recv 消费所有 pending
  → poll_loading_cards()                // 消费异步加载线程发来的卡片
  → poll_texture_results(ctx)           // 消费后台纹理解码结果，每帧最多 5 张 GPU 上传
  → 模式侧栏 → CentralPanel（`ui_stack_canvas::show`）→ 顶栏 → 底栏
  → 堆叠画布（`ui_stack_canvas` 内顺序，与历史 `app/mod.rs` 内联版一致）：
       → stack_viewport：resize 补偿、平移、**滚轮写入前** `ViewTransform` 快照，再应用滚轮缩放
       → stack_cull_draw：`card_display` → 命中检测（R-Tree 指针包络 + `rotated_rect_contains`，布局未变时复用上一帧树与悬停结果）→ 动画 lerp → 同一棵 R-Tree 做视口剔除（与窗口相交就保留；窗外 500px 只预取够大的一块）→ 可见卡按屏幕尺寸要 512/1024/1600 纹理（堆叠前端优先）→ draw_card；拿起卡阴影与顶层绘制；框选高亮与进行中的选框
       → stack_interaction：框选端点；绘制后长按超时→拿起、拿起时滚轮与跟手；松手放下与框选/单击选中
  → 若 drag_pending：canonicalize + drag::start_drag（主线程）
  → 仅在拿起/拖动/动画未收敛/加载或纹理 pending/监听 IO 未回时 `request_repaint`；Shift 截图窗口与画板布局防抖用 `request_repaint_after`
```

- **request_repaint** 不再每帧调用。静止堆叠（含已经平移或缩放后停住的画面）不空转；后台加载、纹理解码、监听 IO 完成时由工作线程唤醒 UI。
- 监听 Create/Modify/Remove 只把路径交给 `deckref-watch-io` 读尺寸与 metadata，主线程用 `from_path_with_metadata` 建卡。切换文件夹时 generation 递增，丢掉过期结果。
- JPEG / WebP 缩略图在解码期按目标边长缩小。总览档最长边 512，单张在屏幕上更大时才升到 1024 或 1600，避免一夹单反同时上传几十张 1200px 纹理。PNG 等格式仍是解码后再缩放。主线程回退路径不对超大的非 JPEG/WebP 做完整解码。
- 所有输入、布局、纹理上传、绘制均在**主线程**。尺寸、metadata 与像素解码在后台线程。

### 2.3 纹理加载（ensure_texture）

- 使用 `HashMap<PathBuf, TextureHandle>` 作为纹理缓存；**清理模式**：仅保留最近 **MAX_TEXTURE_CACHE_FOLDERS（10）个文件夹**的纹理，按「最近使用」顺序维护 `texture_cache_scope`，超过时淘汰最久未访问的文件夹下全部纹理，控制显存与内存。切换至未缓存文件夹或点击「清空」时清空 cache、scope 与 `pending_texture_paths`。
- **前后端分离**：纹理解码在后台线程 `deckref-tex-loader` 完成（`image_ops::load_image_rgba_for_texture`：大图缩略图），主线程只做 `ColorImage::from_rgba_unmultiplied` + `ctx.load_texture`。`ensure_texture` 未命中时仅将 path 发送到 `texture_loading_tx` 并加入 `pending_texture_paths`，本帧返回 None；每帧 `poll_texture_results` 从 `texture_loading_rx` 取最多 **UPLOADS_PER_FRAME（5）** 个结果，仅当 path 仍属于当前 `cards` 时上传并插入 cache，避免切换文件夹后仍写入旧纹理。请求顺序为堆叠前端优先（`cards.iter().rev()`），可见图先出现。
- **缩略图**：高分辨率大图（如单反）在堆叠总览时只加载最长边 512px 的缩略图。卡片在屏幕上的最长边超过当前纹理时，再单独换成 1024 或 1600。原图仍用于布局尺寸（intrinsic_size 来自启动时 dimensions）。
- **每帧**仅对**视口内**卡片的 path 调用 `ensure_texture`（rstar 视口剔除；与窗口相交即可见）；未命中且后台可用时只发请求不阻塞；主线程不再在本帧内做解码，仅在上传完成的帧重绘。**主线程回退**（loader spawn 失败时）：仍用 `load_image_thumbnail` 按目标边长缩小，主线程绝不完整解码超大图。

~~**后续可进一步优化**：视口剔除（仅对可能可见的卡请求纹理）；~~ **已实施**：rstar + clip_rect 视口剔除，且屏幕 AABB 未变时复用同一棵 R-Tree。每帧 path 复用减少分配；`UPLOADS_PER_FRAME` 可配置。监听事件的 dimensions/metadata 已移入 `deckref-watch-io`。

### 2.4 文件监听与新增卡片

- 监听在独立线程，防抖 400ms，通过 `mpsc` 发 `WatchEvent`（Create/Modify/Remove 当前都当「路径变更」处理）。事件线程只转发路径并唤醒 UI。
- 主线程 `poll_watch_events()` 只把路径和 generation 交给 `deckref-watch-io`。后台读 `image_dimensions` 与 metadata 后回传，主线程用 `from_path_with_metadata` 建卡并 push；超过 `MAX_CARDS`（灵感模式为 30）则移除首项并删对应 texture。切换文件夹时 generation 递增，过期结果丢弃。
- worker 启动失败时才在主线程同步读盘。

### 2.5 剪贴板截图轮询（poll_clipboard_screenshot）

- **频率**：每帧都会调用 `poll_clipboard_screenshot`，但内部按 `CLIPBOARD_POLL_INTERVAL`（当前 0.8 秒）节流，**实际读取剪贴板**最多约 1.25 次/秒。
- **主线程**：`arboard::Clipboard::new()` 与 `get_image()` 均在主线程执行；若系统剪贴板 API 偶发阻塞（如其他进程正在写入），可能造成单次数毫秒～数十毫秒的卡顿，但仅每 0.8s 一次。
- **单次成本**：读剪贴板 → 对图片字节做哈希（用于去重）→ 若为新图则创建目录、保存 PNG。大图（如 4K 截图）时哈希为单次遍历，量级在数毫秒内；保存文件仅在「检测到新图」时发生。
- **结论**：0.8s 间隔在常见「剪贴板监控」应用中属于中等频率（不少产品用 0.5～2s）。对绝大多数用户性能影响可接受；若在低端机或希望更省资源，可将 `CLIPBOARD_POLL_INTERVAL` 调大至 1.0～1.5。

## 三、性能关键节点归纳（仅标注，不优化）

以下为后续可优先考虑的「关键节点」，当前仅作记录。

1. **主线程阻塞**
   - ~~启动：`load_existing_images` 中目录扫描 + 每文件多次 metadata/dimensions，全部在主线程。~~ **已优化**：启动改为 `start_async_load` + 后台线程 + `poll_loading_cards` 消费。
   - ~~每帧：所有 UI、命中检测、动画、纹理未命中时的解码、绘制，均在同一线程。~~ **已优化**：纹理解码在后台线程 `deckref-tex-loader`，主线程仅做每帧最多 5 张 GPU 上传；UI、命中检测、动画、绘制仍在主线程。
   - 剪贴板：每 `CLIPBOARD_POLL_INTERVAL`（0.8s）在主线程读一次剪贴板并哈希；偶发系统 API 阻塞时可能短时卡顿，频率已节流。

2. **纹理与图片**
   - ~~纹理为**原图**解码，显示尺寸由 `effective_display_size` 缩小，存在「大图小显」的显存与解码成本。~~ **已优化**：JPEG/WebP 在解码期缩小（IDCT 1/2、1/4、1/8 或 libwebp `use_scaling`）。总览先到最长边 512px；屏幕上放得更大才升到 1024 或 1600。其余格式仍解码后缩放。主线程回退不对超大的非 JPEG/WebP 做完整解码。
   - 视口剔除：与窗口有任何相交就绘制。放大后窗口只盖住大图的一角时不再整张消失。窗外 500px 的预取仍要求相交面积够一块（至少约 64×64，或占该图 20%）。
   - 首展或批量加入时，仅可见卡会请求纹理；未命中由后台解码，主线程只做上传。

3. **每帧分配与 work**
   - 视口剔除后仅对可见卡做 ensure_texture 与绘制。屏幕 AABB 未变时复用上一帧 R-Tree，不再每帧 `bulk_load`。命中检测先查与指针附近包络相交的卡片（不是完全包含），再对候选做旋转矩形测试。
   - ~~对全部卡片做命中检测、动画更新、ensure_texture；无视口剔除或 LOD，卡片数固定上限 80。~~ **已优化**：rstar 视口剔除，仅可见卡请求纹理与绘制。

4. **IO 重复与一致性**
   - ~~启动：同一文件的 metadata 在排序与 `ImageCard::from_path` 中各用一次；dimensions 与 from_path 分开调用，存在重复打开文件。~~ **已优化**：启动使用 `list_existing_image_paths_with_metadata`（一次 metadata）+ 后台线程 `image_dimensions` + 主线程 `from_path_with_metadata`（零 IO）。
   - ~~监听事件处理：每个事件一次 dimensions + 一次 from_path（内部 metadata），与启动类似。~~ **已优化**：主线程只入队路径，`deckref-watch-io` 读尺寸与 metadata，主线程 `from_path_with_metadata`。

5. **刷新与输入**
   - ~~每帧 `request_repaint()`，实际帧率由 egui/平台决定；无「有输入/动画才重绘」的节流策略说明。~~ **已优化**：仅在交互动画、加载/纹理/监听 IO 未完成时 `request_repaint`。静止画面（包括停住的平移和缩放）不空转。Shift 截图窗口和画板 5 秒布局保存用 `request_repaint_after`。后台结果到达时唤醒 UI。

6. **拖拽**
   - `drag_pending` 时在主线程做 `canonicalize` 和 `drag::start_drag`，可能短暂阻塞。

7. **字体**
   - 启动时一次性读取字体文件，体量有限，非当前瓶颈。

---

## 四、资源与规模假设（便于后续优化决策）

- **MAX_CARDS** = 80，当前单列表、单 cache 规模固定。
- **MAX_TEXTURE_CACHE_FOLDERS** = 10，纹理缓存仅保留最近 10 个文件夹的纹理，超出后按「最久未访问」淘汰整文件夹，避免跨文件夹无限累积。
- **防抖** = 400ms，监听侧已做合并。
- **无异步运行时**：启动阶段已通过独立加载线程 + mpsc 移出主线程；若未来将纹理解码等移出主线程，可继续用 spawn + channel 或引入轻量线程池。
- **egui**：即时模式、每帧重算 UI，纹理通过 ctx 管理；大纹理数量与尺寸会直接影响显存与上传成本。

---

## 五、报告结论

- **架构**：单主线程 UI + 单监听线程，模块边界清晰（app / watcher / image_ops / image_card），便于后续在「不改功能」前提下做性能优化。
- **关键节点**：~~启动阶段的主线程 IO 与重复 metadata/dimensions~~（已优化为异步加载）；每帧的全局 ensure_texture（及主线程解码）；全量原图纹理无缩略图；每帧 paths 克隆与全卡命中/动画/绘制。
- 启动阶段优化已实施（后台加载线程 + 单次 metadata）；其余关键节点在需要时再针对实施。

---

*文档版本：基于当前代码库静态分析，未做运行时 profiling。*
