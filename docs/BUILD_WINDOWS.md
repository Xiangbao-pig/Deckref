# DeckRef Windows 构建与打包说明

## 一、版本与产物

- **版本**：与 `Cargo.toml` 中一致（当前 0.1.1）。
- **产物目录**：`ANzhuang\`（安装文件单独存放，便于分发或拷贝）。

## 二、环境要求

- 已安装 [Rust](https://www.rust-lang.org/tools/install)（`rustc --version` 可用）。
- Windows 10/11，PowerShell 可用。
- 在项目根目录（含 `Cargo.toml`）下执行。

## 三、一键打包

在项目根目录打开 PowerShell，执行：

```powershell
.\scripts\build_windows.ps1
```

脚本会：

1. 执行 `cargo build --release` 编译 Windows 版 release；
2. 创建 **`ANzhuang`** 文件夹（若不存在）；
3. 将 `deckref.exe` 复制到 `ANzhuang\`；
4. 若存在项目根目录下 `assets\`（如 icon.png），则复制到 `ANzhuang\assets\`；
5. 在 `ANzhuang\` 下生成 **使用说明.txt**（运行方式、配置目录、系统要求）；
6. 复制 **卸载.bat**（卸载程序，可选删除 `%APPDATA%\DeckRef` 配置后提示手动删除 ANzhuang 文件夹）；
7. 复制 **config_readme.txt**（说明无需 .env、配置目录路径与所含文件）。

**正常时**：二进制约 **5–15 MB**；若明显偏小，请确认 `cargo build --release` 无报错。

## 四、使用与分发

- **本机运行**：进入 `ANzhuang`，双击 **deckref.exe** 即可。
- **发给他人**：将整个 **ANzhuang** 文件夹打包（zip/拷贝），对方解压后双击 `deckref.exe` 运行。
- **配置与数据**：首次运行会在 `%APPDATA%\DeckRef\` 下创建配置；图片目录默认在 `用户\图片\Ref` 等，见应用内说明。

无需单独安装 VC 运行库；若杀毒软件提示，可选择信任或加入白名单（本地编译的未签名 exe 常见）。

## 五、涉及文件

- **打包脚本**：`scripts/build_windows.ps1`
- **随包复制的文案/脚本**：`scripts/install_readme_win.txt`、`scripts/uninstall_win.bat`、`scripts/config_readme_win.txt`
- **版本号**：`Cargo.toml` 的 `version`；若需与脚本内说明一致，可同步修改脚本顶部的 v0.1.1 注释及 `ANzhuang\使用说明.txt` 内容（脚本会每次覆盖该 txt）。

维护：若修改版本号或安装目录名，请同步改 `build_windows.ps1` 与本文档。
