# DeckRef 测试与 Cursor 使用指引

说明如何在项目内**合理、高效**地做测试，以及如何**用 Cursor 跑测试、写测试、把测试纳入流程**。

---

## 一、当前测试方式（Rust）

- **单元测试**：在模块内用 `#[cfg(test)] mod tests { ... }`，`cargo test` 会编译并运行。
- **位置**：例如 `src/image_ops.rs` 底部已有 `mod tests`，覆盖 `is_image_path`、`unique_capture_path`、`unique_path_for_drop` 等纯逻辑。
- **GUI 部分**：主逻辑在 `app.rs` / `image_card.rs`，与 eframe/egui 强耦合，适合先测**可拆出的纯函数**（如布局计算、路径/扩展名判断），UI 行为主要靠人工验证或后续集成测试。

### 常用命令（在项目根目录执行）

| 命令 | 作用 |
|------|------|
| `cargo test` | 运行全部测试（含 lib 的单元测试） |
| `cargo test --no-run` | 只编译不运行（快速检查能否通过编译） |
| `cargo test image_ops` | 只运行名称中含 `image_ops` 的测试 |
| `cargo test -- --nocapture` | 运行测试并打印 `println!` 输出（调试用） |
| `cargo clippy` | 静态检查，建议在提交前与 `cargo test` 一起跑 |

---

## 二、用 Cursor 高效跑测试

### 1. 终端里直接跑

- 在 Cursor 里打开终端（项目根目录），执行：
  ```bash
  cargo test
  ```
- 改完代码后随时再跑一次，确认没改坏。

### 2. 用 Cursor 的「任务/命令」跑（若已配置）

- 若有 `tasks.json` 配置了 `cargo test`，可用「运行任务」一键执行。
- 未配置时，直接终端 `cargo test` 即可。

### 3. 提交/推送前必跑

- `docs/GITHUB_SUBMISSION.md` 规定：推送前必须 `cargo build` 通过，并建议 `cargo clippy` 无新增 warning。
- **建议**：提交前同时跑 `cargo test`，保证已有测试全绿，再执行提交/推送。

### 4. 让 Agent 帮你跑

- 在对话里说：「改完这段后请运行 `cargo test` 并把结果贴出来。」
- 或：「提交前请按 GITHUB_SUBMISSION 预审，并运行 `cargo test` 和 `cargo clippy`。」

---

## 三、用 Cursor 合理写测试

### 1. 优先测「纯逻辑」

- **适合加单元测试**：不依赖 GUI、不依赖真实文件系统的函数。例如：
  - `image_ops::is_image_path`
  - 路径/字符串处理、布局/数学计算、状态机转换等。
- **暂不强行测**：每帧调用的 UI 回调、依赖窗口/OpenGL 的绘制，除非抽成可测的小函数。

### 2. 新增逻辑时顺带补测试

- 新增或修改了**可测的纯函数**时，在同一个文件底部 `#[cfg(test)] mod tests` 里补用例。
- 在对话里说：「给这个函数加 2～3 个单元测试，边界情况要覆盖到。」

### 3. 测试风格

- 用例名能看出在测什么，例如：`is_image_path_accepts_supported_extensions`、`unique_capture_path_has_expected_format`。
- 用 `assert!` / `assert_eq!` 表达预期；需要临时目录时可用 `std::env::temp_dir()` 或 `tempfile` crate，避免写死 `/tmp`（Windows 兼容）。

### 4. 失败时怎么查

- `cargo test` 会标出失败用例和 panic 信息。
- 用 `cargo test -- --nocapture` 看 `println!` 输出，或让 Agent「加一点断言和打印，再跑一次 test」。

---

## 四、测试在流程中的位置

- **开发中**：改完一块逻辑就顺手 `cargo test`，避免积压到提交前才发现。
- **提交前**：按 `docs/GITHUB_SUBMISSION.md` 做预审时，加上「`cargo test` 与 `cargo clippy` 通过」。
- **CI（若后续有）**：在 GitHub Actions 等流水线里跑 `cargo test` 和 `cargo clippy`，保证主分支始终可构建、测试全过。

---

## 五、小结

- **跑测试**：项目根目录执行 `cargo test`；提交前与 `cargo build`、`cargo clippy` 一起跑。
- **写测试**：优先给纯函数加单元测试，放在对应模块的 `#[cfg(test)] mod tests` 里；新增逻辑时顺带补测。
- **用 Cursor**：在对话里明确要求「改完运行 cargo test」「提交前跑 test + clippy」，或让 Agent 为某函数补充单元测试。

这样测试既可持续运行，又能和 Cursor 的日常开发、提交预审结合起来，做到合理且高效。
