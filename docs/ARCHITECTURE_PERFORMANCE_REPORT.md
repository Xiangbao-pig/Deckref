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
- **app.rs**：应用状态、UI 主循环、事件轮询、纹理缓存、卡片列表与交互
- **watcher.rs**：目录扫描、防抖监听、通过 channel 发送 `WatchEvent`
- **image_ops.rs**：`load_image`、`image_dimensions`、`save_image`（纯函数，无内部缓存）
- **image_card.rs**：`ImageCard` 结构、堆叠布局、命中检测、绘制（mesh/阴影）、动画插值
- **error.rs**：统一错误类型与 `AppResult`

整体是「单主线程 UI + 一个监听线程 + 一个按需存在的加载线程」的架构；启动时不再阻塞主线程。

---

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
  → 底部栏 / CentralPanel UI
  → 堆叠区域：
       → 命中检测（从最前到最后，每卡 position_and_rotation + rotated_rect_contains）
       → 每卡动画更新（lerp scale/rotation/position）
       → paths = cards.iter().map(|c| c.path.clone()).collect()
       → for path in paths { ensure_texture(ctx, path) }  // 可能解码图片
       → 绘制所有卡片（未拿起的）
       → 若有拿起卡，再绘制阴影 + 该卡
       → 长按/拿起/松手/滚轮缩放等状态更新
  → 若 drag_pending：canonicalize + drag::start_drag（主线程）
  → ctx.request_repaint()
```

- **request_repaint** 每帧调用 → 应用会持续以高刷新率重绘（依赖 egui/eframe 的节流策略）。
- 所有输入、布局、纹理加载、绘制均在**主线程**。

### 2.3 纹理加载（ensure_texture）

- 使用 `HashMap<PathBuf, TextureHandle>` 作为纹理缓存；**清理模式**：仅保留最近 **MAX_TEXTURE_CACHE_FOLDERS（10）个文件夹**的纹理，按「最近使用」顺序维护 `texture_cache_scope`，超过时淘汰最久未访问的文件夹下全部纹理，控制显存与内存。切换至未缓存文件夹或点击「清空」时清空 cache 与 scope。
- 逻辑：若 cache 中无该 path，则先 `image_ops::image_dimensions(path)` 判断尺寸；若**最长边 > THUMBNAIL_RESOLUTION_THRESHOLD（2048）**则调用 `load_image_thumbnail(path, THUMBNAIL_MAX_LONGEST_EDGE)`（解码后缩放到最长边 1200px，CatmullRom），否则 `load_image(path)` 原图；再 `ColorImage::from_rgba_unmultiplied(...)`、`ctx.load_texture(...)` 并插入 cache；插入后将该 path 所在文件夹加入 scope 前端，若 scope 超过 10 则弹出最旧文件夹并 evict 其下全部纹理。
- **缩略图**：高分辨率大图（如单反 50MB）在堆叠预览时只加载 1200px 最长边的缩略图，保证 2K 屏清晰且降低解码与显存；原图仍用于布局尺寸（intrinsic_size 来自启动时 dimensions）。
- **每帧**对当前所有卡片的 path 调用 `ensure_texture`，未命中时才解码；首帧或新加入卡片时，多张同时未命中会在主线程顺序解码（但每张若为超大图则只解码缩略图）。

### 2.4 文件监听与新增卡片

- 监听在独立线程，防抖 400ms，通过 `mpsc` 发 `WatchEvent`（Create/Modify/Remove 当前都当 Modify 处理）。
- 主线程在 `poll_watch_events()` 里对每个事件：  
  `image_dimensions(&path)` + `ImageCard::from_path(path, display_size)`，然后 push 到 `cards`；超过 `MAX_CARDS` 则移除首项并删对应 texture。
- 新增卡片的 IO 与 `from_path` 的 metadata 读取都在主线程，批量事件时会有短时尖峰。

### 2.5 剪贴板截图轮询（poll_clipboard_screenshot）

- **频率**：每帧都会调用 `poll_clipboard_screenshot`，但内部按 `CLIPBOARD_POLL_INTERVAL`（当前 0.8 秒）节流，**实际读取剪贴板**最多约 1.25 次/秒。
- **主线程**：`arboard::Clipboard::new()` 与 `get_image()` 均在主线程执行；若系统剪贴板 API 偶发阻塞（如其他进程正在写入），可能造成单次数毫秒～数十毫秒的卡顿，但仅每 0.8s 一次。
- **单次成本**：读剪贴板 → 对图片字节做哈希（用于去重）→ 若为新图则创建目录、保存 PNG。大图（如 4K 截图）时哈希为单次遍历，量级在数毫秒内；保存文件仅在「检测到新图」时发生。
- **结论**：0.8s 间隔在常见「剪贴板监控」应用中属于中等频率（不少产品用 0.5～2s）。对绝大多数用户性能影响可接受；若在低端机或希望更省资源，可将 `CLIPBOARD_POLL_INTERVAL` 调大至 1.0～1.5。

## 三、性能关键节点归纳（仅标注，不优化）

以下为后续可优先考虑的「关键节点」，当前仅作记录。

1. **主线程阻塞**
   - ~~启动：`load_existing_images` 中目录扫描 + 每文件多次 metadata/dimensions，全部在主线程。~~ **已优化**：启动改为 `start_async_load` + 后台线程 + `poll_loading_cards` 消费。
   - 每帧：所有 UI、命中检测、动画、纹理未命中时的解码、绘制，均在同一线程。
   - 剪贴板：每 `CLIPBOARD_POLL_INTERVAL`（0.8s）在主线程读一次剪贴板并哈希；偶发系统 API 阻塞时可能短时卡顿，频率已节流。

2. **纹理与图片**
   - ~~纹理为**原图**解码，显示尺寸由 `effective_display_size` 缩小，存在「大图小显」的显存与解码成本。~~ **已优化**：最长边 > 2048px 的图在 `ensure_texture` 时加载缩略图（最长边 1200px），2K 屏上仍清晰，解码与显存显著降低。
   - 首展或批量加入时，多张未命中会在主线程顺序解码（超大图仅解码缩略图）。

3. **每帧分配与 work**
   - `paths: Vec<PathBuf> = self.cards.iter().map(|c| c.path.clone()).collect()` 每帧分配。
   - 对全部卡片做命中检测、动画更新、ensure_texture；无视口剔除或 LOD，卡片数固定上限 80。

4. **IO 重复与一致性**
   - ~~启动：同一文件的 metadata 在排序与 `ImageCard::from_path` 中各用一次；dimensions 与 from_path 分开调用，存在重复打开文件。~~ **已优化**：启动使用 `list_existing_image_paths_with_metadata`（一次 metadata）+ 后台线程 `image_dimensions` + 主线程 `from_path_with_metadata`（零 IO）。
   - 监听事件处理：每个事件一次 dimensions + 一次 from_path（内部 metadata），与启动类似。

5. **刷新与输入**
   - 每帧 `request_repaint()`，实际帧率由 egui/平台决定；无「有输入/动画才重绘」的节流策略说明。

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
