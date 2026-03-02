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

整体是「单主线程 UI + 一个监听线程」的架构，无额外线程池或异步任务。

---

## 二、数据流与执行路径（与性能相关）

### 2.1 启动阶段

```
main
  → resolve_pictures_ref_dir()           // 目录解析/创建
  → eframe::run_native(..., create_app)
       → configure_cjk_fonts(cc)        // 读字体文件入内存，一次性
       → DeckRefApp::new(watch_path)
            → mpsc::channel() + run_watcher(…)   // 启动监听线程
            → load_existing_images()     // 见下
```

**load_existing_images()**（在 `App::new` 内，主线程）：

1. `list_existing_image_paths(dir)`  
   - `read_dir` 遍历，按扩展名过滤  
   - 对每个文件 `metadata(p).modified()` 用于排序（每个文件一次 IO）  
   - 返回按修改时间排序的路径列表
2. 对每条路径（最多 `MAX_CARDS`，当前 80）：  
   - `image_ops::image_dimensions(&path)` → 打开文件读头部/解码头  
   - `ImageCard::from_path(path, intrinsic)` → 内部再次 `std::fs::metadata(&path)` 取 size/modified  

**结论**：启动时主线程串行做「目录遍历 + 每文件多次 IO（metadata + dimensions）」，卡片多时启动会明显阻塞；同一文件的 metadata 在排序与 `from_path` 中重复访问。

### 2.2 每帧主循环（run_ui）

```
update() → run_ui(ctx, frame)
  → poll_watch_events()                  // try_recv 消费所有 pending
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

- 使用 `HashMap<PathBuf, TextureHandle>` 作为纹理缓存，无独立容量上限；卡片数受 `MAX_CARDS` 限制，超出时从 `cards` 移除并从 cache 删对应项。
- 逻辑：若 cache 中无该 path，则  
  `image_ops::load_image(path)` → `std::fs::read(path)` + `image::load_from_memory_with_format(...)`，  
  再 `ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())`，  
  最后 `ctx.load_texture(...)` 并插入 cache。
- **load_image** 是完整解码为 RGBA，无缩略图或分级分辨率；大图会带来主线程解码耗时和显存占用。
- **每帧**对当前所有卡片的 path 调用 `ensure_texture`，未命中时才解码；首帧或新加入卡片时，多张同时未命中会导致多张图在主线程顺序解码。

### 2.4 文件监听与新增卡片

- 监听在独立线程，防抖 400ms，通过 `mpsc` 发 `WatchEvent`（Create/Modify/Remove 当前都当 Modify 处理）。
- 主线程在 `poll_watch_events()` 里对每个事件：  
  `image_dimensions(&path)` + `ImageCard::from_path(path, display_size)`，然后 push 到 `cards`；超过 `MAX_CARDS` 则移除首项并删对应 texture。
- 新增卡片的 IO 与 `from_path` 的 metadata 读取都在主线程，批量事件时会有短时尖峰。

---

## 三、性能关键节点归纳（仅标注，不优化）

以下为后续可优先考虑的「关键节点」，当前仅作记录。

1. **主线程阻塞**
   - 启动：`load_existing_images` 中目录扫描 + 每文件多次 metadata/dimensions，全部在主线程。
   - 每帧：所有 UI、命中检测、动画、纹理未命中时的解码、绘制，均在同一线程。

2. **纹理与图片**
   - 纹理为**原图**解码，显示尺寸由 `effective_display_size` 缩小，存在「大图小显」的显存与解码成本。
   - 无缩略图或按需分辨率；首展或批量加入时，多张未命中会在主线程顺序解码。

3. **每帧分配与 work**
   - `paths: Vec<PathBuf> = self.cards.iter().map(|c| c.path.clone()).collect()` 每帧分配。
   - 对全部卡片做命中检测、动画更新、ensure_texture；无视口剔除或 LOD，卡片数固定上限 80。

4. **IO 重复与一致性**
   - 启动：同一文件的 metadata 在排序与 `ImageCard::from_path` 中各用一次；dimensions 与 from_path 分开调用，存在重复打开文件。
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
- **防抖** = 400ms，监听侧已做合并。
- **无异步运行时**：若未来将 IO 或解码移出主线程，需要自行 spawn 或引入轻量异步/线程池。
- **egui**：即时模式、每帧重算 UI，纹理通过 ctx 管理；大纹理数量与尺寸会直接影响显存与上传成本。

---

## 五、报告结论

- **架构**：单主线程 UI + 单监听线程，模块边界清晰（app / watcher / image_ops / image_card），便于后续在「不改功能」前提下做性能优化。
- **关键节点**：启动阶段的主线程 IO 与重复 metadata/dimensions；每帧的全局 ensure_texture（及主线程解码）；全量原图纹理无缩略图；每帧 paths 克隆与全卡命中/动画/绘制。
- 本期仅做上述观察与标注，**不做具体优化**；在关键节点需要优化时再针对上述项逐项实施。

---

*文档版本：基于当前代码库静态分析，未做运行时 profiling。*
