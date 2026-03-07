DeckRef v0.1.4 — Windows 安装文件

【运行】
  双击 deckref.exe 即可运行。
  或使用同目录下的 DeckRef-0.1.4.msi 一键安装到本机（若打包时已生成 MSI）。

【说明】
  - 首次运行会在 用户目录\AppData\Roaming\DeckRef\ 下创建配置（窗口大小、最近路径、设置等）。
  - 默认监听 图片\Ref 目录；灵感模式使用 图片\Ref\Inspiration 与 图片\Ref\Screenshots。
  - 可将整个「DeckRef-0.1.4」文件夹拷贝到任意位置或发给他人；内含 README.txt、config_readme.txt、Uninstall.bat。

【升级（从旧版本更新）】
  - 若之前用 MSI 安装：直接运行新版本的 .msi，安装程序会检测到旧版并执行升级（覆盖安装），配置与数据保留在 %APPDATA%\DeckRef，不会丢失。
  - 若之前是绿色版（直接运行 exe）：先关闭正在运行的 DeckRef，再用本文件夹内的 deckref.exe 覆盖旧版本同目录下的 deckref.exe（或直接使用本文件夹、删除旧文件夹即可）。配置在 %APPDATA%\DeckRef，不随 exe 路径变化，会保留。

【卸载】
  运行 Uninstall.bat：可选删除配置与缓存（选「否」则下次安装可继续使用）；不会删除您通过 DeckRef 保存的任何图片。MSI 安装的用户还可到「设置 - 应用」中卸载 DeckRef。

【系统】
  Windows 10/11，无需额外安装运行库（静态链接）。
