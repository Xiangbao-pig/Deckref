# DeckRef macOS 构建与打包说明

## 一、Mac 上能否运行？

可以。当前代码在 macOS 上**基本功能均可运行**：

- **跨平台部分**：eframe/egui、notify、image、arboard、drag 等依赖均为跨平台；窗口、堆叠、平移、拖拽（egui 内）、剪贴板粘贴、文件监听、设置与窗口状态持久化（`~/.config/deckref/`）均可用。
- **已做适配**：配置目录在 Unix 下为 `~/.config/deckref`；中文字体在 macOS 下使用系统字体（苹方、华文黑体等），见 `main.rs` 中 `configure_cjk_fonts`。
- **仅 Windows 的功能**：灵感模式「Windows 最近使用的文件」、从资源管理器拖入时的 OLE 拖放、右键拖动移动窗口的 Win32 桥接等；在 Mac 上这些路径会编译为 no-op 或使用替代逻辑（如非 Windows 下地址栏无盘符、窗口移动用 egui 的 OuterPosition）。

结论：在 Mac 上编译运行后，堆叠浏览、拖入保存、剪贴板粘贴、双模式、设置与记忆等核心功能均可使用。

## 二、在 Mac 上打包成「点一下就能运行」的 zip

需要在 **macOS 本机** 上执行（无法在 Windows 上直接交叉编译出可双击运行的 .app）。

### 1. 环境要求

- 已安装 Rust：`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`，然后 `rustc --version` 确认。
- 在项目根目录打开终端。

### 2. 一键打包

**必须在 macOS 本机执行**（在 Windows 上会生成 `deckref.exe`，脚本找不到 `deckref`，且无法得到 Mac 可用的 .app）。

```bash
chmod +x scripts/build_mac.sh
./scripts/build_mac.sh
```

脚本会输出二进制大小和 zip 大小。**正常时**：二进制约 **5–15 MB**，`DeckRef-mac.zip` 也约 **5–15 MB**。若 zip 只有几十 KB，说明打包的不是完整 .app（例如在 Windows 上运行了脚本或打错了包）。

脚本会：

1. `cargo build --release` 编译当前架构（Intel 或 Apple Silicon）的 release 二进制；
2. 生成 `DeckRef.app`（内含 `Contents/MacOS/deckref` 与可选 `assets`）；
3. 打出 `DeckRef-mac.zip`（体积应与二进制相当，约 5–15 MB）。

### 3. 分发给朋友

- 将 **`DeckRef-mac.zip`** 发给对方；
- 对方在 Mac 上解压，得到 **`DeckRef.app`**；
- **双击 `DeckRef.app`** 即可运行（首次可能需在「系统设置 → 隐私与安全性」中允许来自未签名开发者的应用）。

### 4. 可选：同时打 Intel + Apple Silicon

若需同时支持 Intel 与 M 系列 Mac，需安装对应 target 并分别打包后提供两个 zip，或在一个 .app 里做通用二进制（fat binary），可按需在 `build_mac.sh` 中增加：

```bash
rustup target add x86_64-apple-darwin aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
# 再用 lipo 合并或分别打两个 .app/zip
```

当前脚本默认只打**当前机器架构**，对大多数「给朋友用」场景已足够。

## 三、涉及文件

- **字体与平台**：`src/main.rs`（`configure_cjk_fonts` 的 `#[cfg(target_os = "macos")]` 等）。
- **打包脚本**：`scripts/build_mac.sh`。
- **配置/数据目录**：Mac 上为 `~/.config/deckref/`（与 window_state、SETTINGS 一致）；图片目录仍为 `~/Pictures/Ref`。

维护：若修改版本号或 .app 显示名，可同步改 `scripts/build_mac.sh` 中的 `CFBundleShortVersionString` / `CFBundleName` 与 `Cargo.toml` 的 version。
