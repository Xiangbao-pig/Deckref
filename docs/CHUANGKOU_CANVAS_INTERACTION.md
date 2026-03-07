# Chuangkou（窗口）与画布交互维护文档

本文档严格区分**画布空间（Canvas Space）**与**屏幕空间（Screen Space）**，并梳理各模式下**缩放视图（Zoom）**、**多选/单卡缩放（Transform）**与**调整窗口（Resize）**的逻辑。后续若遇画板模式窗口缩放等问题，应优先据此文档核对实现并再改代码。

**画板模式**：画板采用 **PS 式取景框**逻辑（窗口变化时仅 Clip 变、不重排不重绘、画布固定、原点左上角）时，以 **[BOARD_MODE_OPTIMIZATION.md](BOARD_MODE_OPTIMIZATION.md)** 为准；与该文档冲突处，画板模式以 BOARD_MODE_OPTIMIZATION 为准。

**相关代码**：`src/app.rs`（视口状态、坐标变换、Zoom/Pan/Resize 补偿）、`src/layout.rs`（画板布局与 reference 尺寸）、`src/window_state.rs`（窗口位置与尺寸持久化）。交互行为详见 [INTERACTIONS.md](INTERACTIONS.md)。

---

## 一、空间定义（必须严格区分）

### 1.1 画布空间（Canvas Space / 世界坐标 World Space）

- **含义**：卡片与布局使用的**逻辑坐标**，不随窗口大小或视口缩放直接变化；单位与「内容区」的逻辑尺寸一致。
- **代码中的体现**：
  - **默认 / 灵感模式**：`stack_layout` 的布局位置、每张卡的 `random_offset`、`anim_position_delta`；平移 `view_offset`、缩放 `view_scale` 仅影响「画布 → 屏幕」的变换，不改变这些逻辑坐标。
  - **画板模式**：`board_saved_world_pos`、`display_scale` 为**画布空间绝对值**（BOARD_MODE_OPTIMIZATION）：不随窗口 Resize 缩放，无 `board_scale_u`；视口由 `board_view_offset`、`board_view_scale` 与 `board_view` 模块提供变换，仅 Clip/取景框随窗口变化。
- **重要点**：画布空间是「内容」的坐标系；视口只做观察变换，不修改画布内的物体坐标（除用户拿起、放下、阵列等主动操作）。

### 1.2 屏幕空间（Screen Space）

- **含义**：用户在窗口内看到的**像素坐标**，与 egui/eframe 的 UI 坐标系一致；随窗口尺寸、DPI、内容区布局变化。
- **代码中的体现**：
  - `ctx.screen_rect()`：整个窗口内容区（内框）的像素矩形。
  - **内容区**：`CentralPanel` 的 `ui.available_rect_before_wrap()`，即扣除顶栏、底栏、左侧模式栏后的区域，存为 `stack_rect`，其 `size` 每帧写入 `last_stack_size`。
  - **内容区中心**：`screen_center = stack_rect.center()`，用于画布↔屏幕的变换原点。
  - 指针位置 `pointer`、`clip_rect`、`viewport_for_culling` 等均为屏幕空间。

### 1.3 画布 ↔ 屏幕 变换公式（各模式统一使用）

以下公式在 `app.rs` 中按当前模式选取 `(view_offset, view_scale)` 或 `(board_view_offset, board_view_scale)` 后统一使用：

- **world_to_screen**：  
  `v = world - view_offset` →  
  `screen = screen_center + v * view_scale`
- **screen_to_world**：  
  `v = (screen - screen_center) / view_scale` →  
  `world = view_offset + v`

即：画布原点在逻辑上为 `view_offset`，在屏幕上的「锚点」为 `screen_center`，缩放为 `view_scale`。

---

## 二、各模式下的视口状态（与窗口 Resize 的关系）

| 模式     | 平移量               | 缩放倍率               | 说明 |
|----------|----------------------|------------------------|------|
| 默认     | `view_offset`        | `view_scale`           | 与灵感、画板互不影响 |
| 灵感     | `view_offset`        | `view_scale`           | 同上 |
| 画板     | `board_view_offset`  | `board_view_scale`     | 独立于默认/灵感；Resize 时单独补偿 |

切换模式时，只切换「用哪一组变量」参与上述变换；不共享状态。一览/阵列时的重置见下文 Zoom 与 Resize 节。

---

## 三、缩放视图（Zoom）

**定义**：未拿起任何卡片时，通过滚轮改变**整个视图**的缩放（所有内容一起放大/缩小），不改变窗口尺寸。

### 3.1 行为（所有模式）

- **输入**：未拿起图片时，在内容区滚轮。
- **效果**：视图缩放以**当前指针位置**为中心——缩放前后，指针所指的**画布空间点**保持不变（即该点始终在同一屏幕像素上）。
- **范围**：`VIEW_SCALE_MIN = 0.5`（50%）～ `VIEW_SCALE_MAX = 5.0`（500%）；灵敏度 `VIEW_ZOOM_SENSITIVITY`。

### 3.2 默认 / 灵感模式

- **变量**：`view_scale`、`view_offset`。
- **逻辑**：  
  - 滚轮前先用 `screen_to_world` 得到指针下的世界点 `world_before`。  
  - 更新 `view_scale` 并 clamp。  
  - 为保持该世界点仍在指针下：  
    `view_offset = world_before - (pointer - screen_center) / view_scale`。

### 3.3 画板模式

- **变量**：`board_view_scale`、`board_view_offset`。
- **逻辑**：  
  - 同上，先取指针下的世界点。  
  - 更新 `board_view_scale` 并 clamp。  
  - 补偿平移：  
    `board_view_offset += (pointer - screen_center) * (1/s1 - 1/s2)`  
    （s1、s2 为缩放前后倍率），使指针下的世界点不动。

### 3.4 与「一览」「阵列」的联动

- **一览**：将视图重置为默认——`view_scale = 1.0`、`view_offset = Vec2::ZERO`；若当前为画板，同时 `board_view_scale = 1.0`、`board_view_offset = Vec2::ZERO`。
- **阵列**：默认/灵感设 `view_scale = 1.8`、`view_offset = Vec2::ZERO`；画板设 `board_view_scale = 1.8`、`board_view_offset = Vec2::ZERO`，且画板会写 `layout_reference_size = stack_layout.available_size`。

---

## 四、多选/单卡缩放（Transform）

**定义**：用户「拿起」单张卡片后，通过滚轮调节该卡的显示缩放（Transform），松手后该缩放会保留到该卡（`display_scale` 或画板参考空间下的 scale）。

### 4.1 行为（所有模式共通）

- **输入**：已拿起一张卡片时，滚轮。
- **效果**：仅该卡缩放，范围 `DISPLAY_SCALE_MIN`（30%）～ `DISPLAY_SCALE_MAX`（500%）；松手后写入该卡的 `display_scale`（或画板下的参考 scale）。
- **代码**：拿起期间用 `held_scale`；松手时根据是否画板+是否有 `layout_reference_size`，决定写回 `random_offset`+`display_scale` 还是 `board_saved_world_pos`+参考 scale。

### 4.2 画板模式下的 Transform

- 画板模式：位置与缩放为**画布空间绝对值**（BOARD_MODE_OPTIMIZATION），**不**随窗口 Resize 缩放。
- 松手时直接写入 `board_saved_world_pos = drop_world`、`display_scale = scale`（无 `board_scale_u` 换算）；重排时直接使用 `old_world_pos[i+1]`。
- 视口逻辑在 `src/board_view.rs` 中模块化，与默认/灵感解耦。

---

## 五、调整窗口（Resize）

**定义**：用户拖拽窗口边缘改变窗口大小，导致内容区（CentralPanel）的 `stack_rect` / `last_stack_size` 变化。

### 5.1 默认 / 灵感模式

- **当前实现**：**不做**任何针对 Resize 的视口补偿。
- 效果：内容区变大/变小后，`screen_center` 会变，画布仍用同一套 `view_offset`、`view_scale`，因此「画布中心」在屏幕上的位置会随内容区中心移动而移动（可能产生「画布跟着窗口中心跑」的观感）。

### 5.2 画板模式（PS 式：仅 Clip 变，不 Reflow/Repaint）

- **目标**（BOARD_MODE_OPTIMIZATION）：Resize 时**仅裁剪区域（Clip）变化**；画布尺寸、元素位置与缩放**不变**，不触发 Reflow/Repaint。
- **实现**：
  - 视口补偿由 `board_view::apply_resize_compensation` 完成：用 `last_board_screen_center` 与当前 `screen_center` 的差补偿 `board_view_offset`，使原先在中心的世界点仍在新中心。
  - **不**使用 `board_scale_u` 或 `last_stack_size` 对位置/缩放做任何换算；画布内坐标与 scale 为绝对值。
- **相关代码**：`src/board_view.rs`（resize/zoom/pan 纯函数）、`src/app.rs`（仅画板分支调用）。

### 5.3 窗口状态持久化（与 Resize 无关，但与窗口尺寸相关）

- 关闭窗口时：将**外框位置**（`outer_rect.min`）与**内尺寸**（`ctx.screen_rect()` 的宽高）保存到配置目录（如 `window.txt`）；下次启动用其恢复位置与尺寸。
- 最小内尺寸：200×150（内容区），由 `window_state::min_inner_size()` 与 ViewportBuilder 限制。
- 详见 `window_state.rs` 与 [INTERACTIONS.md](INTERACTIONS.md)「窗口与启动」一节。

---

## 六、小结表（便于后续按文档改功能）

| 项目           | 默认/灵感                     | 画板 |
|----------------|-------------------------------|------|
| 画布→屏幕变量  | view_offset, view_scale       | board_view_offset, board_view_scale（board_view 模块） |
| Zoom（滚轮）   | 改 view_scale + view_offset 以指针为中心 | board_view::apply_zoom_at_mouse |
| Transform（拿起+滚轮） | held_scale → 松手写 display_scale / random_offset | held_scale → 松手写 board_saved_world_pos + display_scale（画布绝对值，无 board_scale_u） |
| Resize 补偿    | 无                            | board_view::apply_resize_compensation；仅 Clip 变，不缩放画布内容 |
| 一览/阵列      | 重置 view_scale/offset；阵列 1.8 | 同左并重置 board_*；阵列写 layout_reference_size |

---

## 七、与 INTERACTIONS.md 的对应关系

- **画布空间** 对应 INTERACTIONS 中的「世界坐标 (World Space)」。
- **屏幕空间** 对应「屏幕坐标 (Screen Space)」。
- 本文的 **Zoom** = INTERACTIONS 的「缩放视图（View zoom）」。
- 本文的 **Transform** = INTERACTIONS 的「拿起时滚轮缩放（Scale while holding）」。
- 本文的 **Resize** = INTERACTIONS 的「窗口调整（Window Resize）」；画板补偿公式与 INTERACTIONS 中「offset -= 中心位移 / scale」一致。

若修改 Zoom / Transform / Resize 任一逻辑，请同时更新本文与 INTERACTIONS.md，并严格保持「画布空间」与「屏幕空间」的区分，避免把屏幕像素与画布逻辑坐标混用（例如补偿时必须用 `delta / board_view_scale` 转为画布位移）。
