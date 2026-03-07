# DeckRef Windows 构建与打包说明

## 一、版本与产物

- **版本**：与 `Cargo.toml` 中一致（当前 **0.3.2**）。
- **产物目录**：`DeckRef-<version>\`（如 `DeckRef-0.3.2`），内含 exe、说明、卸载脚本，以及（若已安装 WiX）MSI 一键安装包。

## 二、环境要求

- 已安装 [Rust](https://www.rust-lang.org/tools/install)（`rustc --version` 可用）。
- Windows 10/11，PowerShell 可用。
- 在项目根目录（含 `Cargo.toml`）下执行。
- **MSI 可选**：需安装 [WiX Toolset](https://wixtoolset.org/) 才能生成 `DeckRef-<version>.msi` 并放入安装文件夹；未安装时脚本仍会生成版本号文件夹与绿色版文件。
- **安装 WiX（用于生成 MSI）**：在 PowerShell 中执行 `winget install WiXToolset.WiXToolset`，或从 [WiX 官网](https://wixtoolset.org/docs/wix3/) 下载安装 v3.11/v3.14。安装完成后重新运行 `.\scripts\build_windows.ps1`，即可在 `DeckRef-<version>\` 下得到同名的 `.msi` 文件。

## 三、一键打包

在项目根目录打开 PowerShell，执行：

```powershell
.\scripts\build_windows.ps1
```

脚本会：

1. 从 `Cargo.toml` 读取版本号（优先 `cargo pkgid`，失败时默认 0.3.2）；
2. 执行 `cargo build --release` 编译 Windows 版 release；
3. 创建 **`DeckRef-<version>`** 文件夹（若已存在会先清空）；
4. 将 `deckref.exe`、**DeckRef.ico**（构建时生成于 `wix/`）、**README.txt**、**config_readme.txt**、**Uninstall.bat** 复制到该文件夹；
5. 若存在项目根目录下 `assets\`，则复制到 `DeckRef-<version>\assets\`；
6. 若已安装 WiX Toolset v3.11：生成 **DeckRef-<version>.msi** 并放入同一文件夹，供一键安装到「Program Files\DeckRef」及开始菜单快捷方式（**图标**会在资源管理器、任务栏、开始菜单、「添加/删除程序」等处正常显示）。

**正常时**：二进制约 **5–15 MB**；若明显偏小，请确认 `cargo build --release` 无报错。

## 四、使用与分发

- **绿色版**：进入 `DeckRef-<version>`（如 `DeckRef-0.3.2`），双击 **deckref.exe** 即可运行。
- **MSI 安装**：双击 **DeckRef-<version>.msi**（如 `DeckRef-0.3.2.msi`）按提示安装；卸载到「设置 - 应用」中卸载 DeckRef，或运行安装目录下的 **Uninstall.bat** 清理配置/缓存（选「否」则下次安装可继续使用）。
- **发给他人**：将整个 **DeckRef-<version>** 文件夹打包（zip/拷贝），对方解压后可直接运行 exe 或使用 MSI 安装。
- **配置与数据**：首次运行会在 `%APPDATA%\DeckRef\` 下创建配置；用户通过 DeckRef 保存的图片在 `用户\图片\Ref` 等目录，卸载不会删除。

无需单独安装 VC 运行库；若杀毒软件提示，可选择信任或加入白名单。

## 五、卸载脚本说明

- **Uninstall.bat** 提供两个选项：
  1. 是否删除配置与数据（`%APPDATA%\DeckRef` 下的 window/settings/last_path/recent_folders）；
  2. 是否清理 DeckRef 产生的缓存（同一目录）；选 **N** 则下次安装可继续使用现有数据。
- 卸载**不会**删除用户通过 DeckRef 保存的任何图片（均在「用户\图片\Ref」等目录）。

## 六、涉及文件

- **打包脚本**：`scripts/build_windows.ps1`
- **图标**：构建时由 `build.rs` 生成。若存在 **`assets/icon.png`** 或 **`assets/main icon.png`**，则从中生成 `wix/DeckRef.ico`（32×32/16×16）并嵌入 exe，MSI 打包时复制到产物目录供 WiX 用于开始菜单与「添加/删除程序」图标；否则使用内置绘制的 logo（深蓝底 + 白色 D 形）。MSI 与 exe 均使用统一图标。
- **随包复制的文案/脚本**：`scripts/install_readme_win.txt`、`scripts/uninstall_win.bat`、`scripts/config_readme_win.txt`
- **MSI 源**：`wix/DeckRef.wxs`（需 WiX Toolset 编译）
- **版本号**：以 `Cargo.toml` 的 `version` 为准；脚本会据此生成 `DeckRef-<version>` 文件夹及 MSI 文件名。

维护：若修改版本号或 MSI 行为，请同步改 `build_windows.ps1`、`wix/DeckRef.wxs` 与本文档。
