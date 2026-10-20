# DeckRef 性能优化设计（功能不变）

**日期**：2026-10-19（Asia/Shanghai）  
**仓库**：https://github.com/Xiangbao-pig/Deckref  
**基线版本**：0.1.3  
**目标**：在不改变现有交互与功能的前提下，降低大图峰值内存、减少平移/缩放掉帧、消除监听落盘主线程尖峰、空闲时停止无意义重绘。

---

## 背景与约束

- 已有能力（保持）：异步目录加载、`deckref-tex-loader` 后台解码、最长边 >2048 → 1200 缩略图、rstar 视口剔除（查询框四周 +500px、可见比例 <20% 才剔除）、按最近 10 个文件夹淘汰纹理、`MAX_CARDS = 80`。
- **不改**：双模式、拖入拖出、灵感模式、设置项语义、用户可见预览清晰度目标、卡片上限与文件夹缓存策略（本期不提高 `MAX_CARDS`、不加新内存预算 UI）。
- **成功标准**：同一套操作路径下，大图加载峰值内存明显下降；静止堆叠时 CPU/GPU 占用下降；批量落盘进监听目录时主线程无明显顿挫；交互手感与现文档 `INTERACTIONS.md` 一致。

---

## 1. 大图解码（优先）

### 问题

`load_image_thumbnail` 先 `load_image` 完整解码，再 `resize_exact`。单反/50MB 级图会短时占用整图内存，再产生缩略图副本。

### 方案

- 在 `image_ops` 增加「目标最长边」导向的加载入口（可扩展现有 `load_image_rgba_for_texture` / `load_image_thumbnail`）：
  - 先取尺寸（已有 `image_dimensions`）。
  - 若最长边 ≤ `THUMBNAIL_RESOLUTION_THRESHOLD`（2048）：行为与现网一致，整图解码。
  - 若需要缩略图：对 **JPEG / WebP** 等支持解码时下采样的格式，优先走「按目标边长解码/缩小」路径，避免完整像素缓冲；其余格式（PNG、PSD、TIFF 等）保留「解码后缩放」回退，保证结果正确。
- 阈值不变：`THUMBNAIL_MAX_LONGEST_EDGE = 1200`；主线程回退路径仍禁止完整解码超大图。
- 输出仍为 RGBA，供现有 GPU 上传路径使用。

### 非目标

- 不引入新依赖除非现有 `image` crate 无法覆盖 JPEG/WebP 下采样（优先用现有 crate API）。
- 不改变原图文件与拖出文件内容（仅预览纹理）。

---

## 2. 每帧 R-Tree 与命中检测

### 问题

每帧对全部卡 `bulk_load` R-Tree；命中检测从最前到最后扫全部卡。平移/缩放时 CPU 开销偏高。

### 方案

- **R-Tree 复用**：为屏幕 AABB 维护「脏标记」。仅当卡片集合、布局参数、view 变换、单卡 `display_scale` / 动画位移 / 旋转等影响屏幕 AABB 的状态变化时重建；否则复用上一帧树。拿起中的卡仍参与查询与绘制。
- **命中检测**：优先用现有（或复用）R-Tree 查询指针附近小包络，再对候选做 `rotated_rect_contains`；无指针移动且无拖动手势时可跳过精算。
- 动画更新逻辑保持：有 `anim_*` 收敛中的卡每帧更新，并置脏。

### 非目标

- 不改为持续增量插入的复杂动态索引（除非复用方案实测仍不够）；首版以「脏则重建」为主。

---

## 3. 监听事件移出主线程

### 问题

`poll_watch_events` 对每个事件在主线程做 `image_dimensions` + `ImageCard::from_path`（含 metadata IO），批量落盘时产生短时尖峰。

### 方案

- 主线程收到 Create/Modify 类事件后，只把 path 交给后台（复用加载通道或独立轻量 worker），后台完成 dimensions + metadata，经 mpsc 回传。
- 主线程 `poll` 回传结果后用 `from_path_with_metadata` 构造卡片、push、动画；超 `MAX_CARDS` 淘汰与现逻辑一致。
- 防抖仍 400ms；Remove 等路径保持当前语义（若仍按 Modify 处理则一并异步化 IO）。

### 非目标

- 不改变 watcher 防抖参数与「超过上限删首项」策略。

---

## 4. 按需重绘

### 问题

每帧无条件 `request_repaint()`，空闲仍高刷新。

### 方案

- 仅在以下情况请求重绘：存在未收敛动画、异步加载/纹理解码 pending、拿起/拖动/窗口移动手势、剪贴板监听窗口内、用户输入导致状态变化。
- 静止且无 pending 时不主动 `request_repaint`，依赖 egui 输入驱动的下一帧。
- 后台完成加载/解码时仍通过现有 poll 路径触发下一帧（收到结果时 `request_repaint`）。

### 非目标

- 不引入固定低帧率计时器（除非按需重绘后某动画卡死，再补 `request_repaint_after`）。

---

## 涉及文件（预期）

| 区域 | 文件 |
|------|------|
| 解码 | `src/image_ops.rs`，调用方 `src/app.rs` |
| 视口/命中/重绘 | `src/app.rs` |
| 监听消费 | `src/app.rs`（必要时小改 `src/watcher.rs` 仅传 path） |
| 文档 | `CHANGELOG.md`、`docs/ARCHITECTURE_PERFORMANCE_REPORT.md`（标注已实施项） |

---

## 验证

- `cargo build --release`（至少默认 target；有条件时 Windows）。
- 手工：打开含大量大图的目录；快速平移/缩放；向监听目录批量复制图片；静止放置 10s 观察 CPU。
- 回归：拖入、拖出、模式切换缓存、清空、专业格式开关路径不破。

---

## 明确不做（本期）

- 提高 `MAX_CARDS`、按字节的全局纹理内存预算 UI。
- 功能/交互改版、UI 视觉重设计。
- 引入完整异步运行时（tokio 等）。
