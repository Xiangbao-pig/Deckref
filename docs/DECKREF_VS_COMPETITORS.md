# DeckRef 与竞品模式对比

本文档将 DeckRef 的**默认**、**灵感**、**画板**三种模式分别与 **Chrome**、**Photoshop**、**Miro**、**PureRef** 四款竞品对照，说明每种模式最接近哪款软件的体验、差异在哪里，并据此给出后续可考虑的改进方向。  
竞品在画布/窗口上的具体行为见 [COMPETITOR_CANVAS_RESEARCH.md](COMPETITOR_CANVAS_RESEARCH.md)；DeckRef 的实现约定见 [CHUANGKOU_CANVAS_INTERACTION.md](CHUANGKOU_CANVAS_INTERACTION.md)。

---

## 一、模式与竞品对应关系概览

| DeckRef 模式 | 最接近的竞品 | 次要参考 | 一句话定位 |
|--------------|--------------|----------|------------|
| **默认**     | Chrome       | —        | 单目录 = 单「页」，视口浏览本地图片，无 Resize 补偿，偏「浏览器式」浏览 |
| **灵感**     | Chrome       | —        | 与默认同一套画布/视口交互，仅数据源改为灵感集（最近使用 + 截图），体验仍偏浏览器 |
| **画板**     | PureRef      | Photoshop | 参考图板 + 布局持久化，Resize 补偿、空格 Pan，对标 PureRef，部分借鉴 PS 导航 |

---

## 二、默认模式 vs Chrome

### 2.1 为何对标 Chrome

- **内容模型**：默认模式是「当前选中的**一个目录**」对应「一屏内的内容」，类似浏览器里「一个页面/文档」对应一个 URL；切换目录 ≈ 换页。
- **视口逻辑**：画布是「当前目录下卡片的堆叠布局」，视口用 `view_offset` + `view_scale` 在这块内容上平移/缩放，不持久化视口状态；和 Chrome 的 Layout Viewport + Visual Viewport 在「单页内容上移动」的直觉一致。
- **无 Resize 补偿**：窗口变大变小后，画布中心会跟着内容区中心跑，和浏览器 Resize 后「文档不自动补偿、只是可见区域变了」的行为类似。

### 2.2 相似点

| 维度 | DeckRef 默认 | Chrome |
|------|----------------|--------|
| 画布空间 | 堆叠布局 + 卡片逻辑坐标（random_offset 等） | 文档内容（page/document） |
| 屏幕空间 | 内容区 = CentralPanel，screen_center 为变换锚点 | Visual Viewport（可见区域 + scale） |
| Zoom | 滚轮、以指针为中心、50%～500% | 以焦点/手势为中心，scale 变化 |
| 内容与视口 | 视口在「当前目录内容」上滑动/缩放，不保存 | 视口在单页上滚动/缩放，刷新或换页即重置 |
| Resize | 无补偿，中心会漂移 | 一般不补偿，视口尺寸变 |

### 2.3 差异点

| 维度 | DeckRef 默认 | Chrome |
|------|----------------|--------|
| 内容来源 | 本地文件夹，文件系统驱动 | 网络/本地 HTML，文档驱动 |
| 导航 | 面包屑、最近到访、前进后退（目录维度） | 地址栏、历史、多标签 |
| 一览/阵列 | 有「打散」「阵列」重排，视图重置 100% | 无直接等价，多为整页缩放 |
| 单对象操作 | 长按拿起、拖放、单卡缩放（Transform） | 由页面内应用实现，无统一范式 |
| 多窗口 | 单窗口 | 多标签/多窗口 |

### 2.4 可考虑的改进（默认模式）

- **Resize 补偿（可选）**：若希望「拉大窗口时当前看到的区域不跑」，可像画板一样在默认/灵感下也做「内容区中心位移 → offset 补偿」，使画布中心尽量保持在屏幕中心；实现上可与画板共用思路、按模式开关。
- **视口记忆（可选）**：当前切换目录后视口重置；若需「回到某目录时恢复上次缩放/平移」，可对该目录持久化 view_offset / view_scale（或仅 scale），与竞品「浏览器记住缩放」类似。

---

## 三、灵感模式 vs Chrome

### 3.1 为何仍对标 Chrome

- 灵感与默认**共用同一套画布与视口**（同一组 `view_offset`、`view_scale`），交互完全一致；差异只在**数据源**：灵感 = 最近使用 + Ref/Screenshots，默认 = 当前选中目录。
- 因此「画布/屏幕空间 + Zoom + Transform + Resize」的体验与默认一致，仍是「单块内容 + 视口浏览」，对应 Chrome 的「单页 + 视口」模型。

### 3.2 相似点

- 与「默认 vs Chrome」相同：以指针为中心的 Zoom、无 Resize 补偿、视口不持久化、单卡拿起/缩放等。

### 3.3 差异点

| 维度 | DeckRef 灵感 | Chrome |
|------|----------------|--------|
| 内容集合 | 固定集合（最近+截图，上限 30） | 单页 DOM，可任意大 |
| 入口 | 侧栏切模式，无独立 URL | 地址栏/书签/历史 |
| 与默认的差异 | 仅数据源和保存路径（Inspiration/Screenshots） | 无「模式」概念，每页独立 |

### 3.4 可考虑的改进（灵感模式）

- 与默认模式相同：若为默认增加 Resize 补偿或视口记忆，灵感可一并享受（因共用 view 状态）。
- 若未来为「灵感」单独做 UI（如时间线、来源筛选），仍建议保持与默认同一套画布/视口逻辑，避免两套行为分裂。

---

## 四、画板模式 vs PureRef（+ Photoshop）

### 4.1 为何对标 PureRef 为主、Photoshop 为辅

- **PureRef**：参考图/看板工具，画布上摆图片、位置与缩放持久化、窗口 Resize 时可选择「缩放补偿」、右键/标题栏移动窗口、Alt/中键 Pan、滚轮 Zoom——与 DeckRef 画板的产品形态和交互高度一致。
- **Photoshop**：画板模式复刻了 PS 的「空格 + 左键拖画布」、以指针为中心的 Zoom、画布与视口分离；Resize 上我们采用「offset 补偿」而非 PS 的「缩放时调整窗口大小」，更接近 PureRef 的「保持看到的内容」思路。

### 4.2 相似点

| 维度 | DeckRef 画板 | PureRef | Photoshop |
|------|----------------|---------|------------|
| 画布空间 | 参考空间 board_saved_world_pos + display_scale，随目录持久化 | 画布扩展以容纳图片，原点 (0,0) | 文档像素网格 |
| 屏幕空间 | 内容区中心 + board_view_offset/scale | 窗口内可见区域，相机 offset+scale | 文档窗口可见区域 |
| Zoom | 滚轮、以指针为中心、50%～500% | 滚轮、Z+拖拽、以指针为中心 | Scrubby/滚轮、以点击或指针为中心 |
| Pan | 中键 或 空格+左键 | Alt+左键 或 中键 | 空格+左键 或 中键 |
| Resize | offset 补偿，画布中心不动 | 可选「Scale scene on window resize」用 zoom 补偿 | 可选「缩放时调整窗口」 |
| 布局持久化 | .deckref_layout.json（sidecar） | 场景保存 | 文档保存 |

### 4.3 差异点

| 维度 | DeckRef 画板 | PureRef | Photoshop |
|------|----------------|---------|------------|
| Resize 策略 | **仅 offset 补偿**，scale 不变 | **可选** zoom 随窗口变，保持「看到的内容」 | 可选「缩放时调整窗口」 |
| 画布范围 | 随内容 + 参考尺寸比例（board_scale_u） | 画布随内容扩展，Optimize canvas | 固定文档尺寸 |
| 多选 Transform | 暂无，仅单卡拿起+缩放 | 支持多选整体移动/缩放 | 多选共用一个变换框 |
| 平移快捷方式 | 空格+左键（复刻 PS） | Alt+左键 或 中键 | 空格+左键 或 中键 |
| 窗口移动 | 右键拖过阈值 | 右键拖 或 标题栏 | 系统标题栏 |

### 4.4 可考虑的改进（画板模式）

- **Resize 策略可选**：  
  - 当前：仅 offset 补偿（画布中心对准新中心，scale 不变）。  
  - 可增加「PureRef 式」选项：Resize 时**同时调整 zoom**，使「当前可见内容」尽量保持相同的视觉范围（类似「Scale scene on window resize」），用户可在设置中二选一。
- **多选 Transform**：  
  - 若支持框选/Shift 多选，可做「整体移动 + 整体缩放」，类似 PureRef/Photoshop 的多选变换框；坐标仍落在画布空间，布局持久化时一并写入。
- **画布边界/优化**：  
  - PureRef 的「Optimize canvas」「Focus canvas」对应我们「阵列」「一览」的部分效果；可考虑画板下单独的「适应选区/全部」快捷操作，便于快速框定视图。

---

## 五、与 Miro 的对比（三个模式）

Miro 是**无限白板 + 协作**，对象以便签、图形、连线为主，画布原点在中心、视口不随 Resize 补偿。DeckRef 三个模式与 Miro 的关系可概括为：

| 维度 | DeckRef | Miro |
|------|----------|------|
| 默认/灵感 | 有界堆叠内容 + 视口，无无限画布 | 无限画布，viewport 在 board 上移动 |
| 画板 | 有参考空间 + 持久化，画布随内容与参考尺寸延伸 | 无限画布，对象坐标相对 board 中心 |
| Zoom 中心 | 以指针为中心 | 视口中心或手势中心 |
| Resize | 画板有 offset 补偿；默认/灵感无 | 无补偿，viewport 尺寸变 |
| 协作/多选 | 单卡拿起为主，无多选变换 | 多选、框选、协作编辑 |

结论：**DeckRef 没有刻意复刻 Miro**；画板在「画布中心 + 视口」的数学模型上与 Miro 的 viewport API 类似，但产品形态是「本地参考图板」而非「在线无限白板」。若未来做多选或「画布适应选区」，可参考 Miro 的 viewport.set / zoomTo 的用法，不必追求行为完全一致。

---

## 六、改进方案汇总（按优先级建议）

| 优先级 | 方向 | 适用模式 | 参考竞品 | 说明 |
|--------|------|----------|----------|------|
| 高 | Resize 补偿（offset）扩展到默认/灵感 | 默认、灵感 | Chrome 无，但我们可自选 | 减少「拉大窗口时画布跑偏」的违和感，与画板一致 |
| 高 | 画板 Resize 可选「zoom 补偿」 | 画板 | PureRef「Scale scene on window resize」 | 用户可选：保持 scale 不变 vs 保持可见范围 |
| 中 | 视口记忆（按目录/模式） | 默认、灵感 | Chrome 记住缩放 | 切回某目录或灵感时恢复上次 view_scale / view_offset |
| 中 | 多选 + 整体 Transform | 画板 | PureRef、Photoshop | 框选/Shift 多选后整体移动、缩放，写回布局 |
| 低 | 「适应选区/全部」快捷 | 画板 | PureRef Focus/Optimize | 一键把视口或画布适配到选中或全部内容 |

以上可作为产品路线图与 [CHUANGKOU_CANVAS_INTERACTION.md](CHUANGKOU_CANVAS_INTERACTION.md) 的输入，实现时在窗口与画布文档中补充对应小节即可。

---

**文档版本**：初稿  
**依赖**：[COMPETITOR_CANVAS_RESEARCH.md](COMPETITOR_CANVAS_RESEARCH.md)、[CHUANGKOU_CANVAS_INTERACTION.md](CHUANGKOU_CANVAS_INTERACTION.md)、[INTERACTIONS.md](INTERACTIONS.md)
