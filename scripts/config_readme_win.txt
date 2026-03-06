DeckRef 环境与配置说明 (Windows)

【无需环境配置文件】
  本程序不需要 .env 或其它环境配置文件即可运行；所有配置均在首次运行后自动生成。

【配置与数据目录】
  位置: %APPDATA%\DeckRef
  典型路径: C:\Users\你的用户名\AppData\Roaming\DeckRef
  仅含程序生成的配置（无用户图片）:
  - window.txt        窗口位置与尺寸
  - settings.txt      设置项（反转平移、拖拽出窗口、专业格式预览等）
  - last_path.txt     上次打开的文件夹路径
  - recent_folders.txt 最近到访的 5 个文件夹

【用户图片（卸载不会删除）】
  通过 DeckRef 保存的图片均在以下目录，卸载时不会删除：
  - 默认监听/保存: 用户\图片\Ref
  - 截图: 用户\图片\Ref\Screenshots
  - 灵感模式: 用户\图片\Ref\Inspiration

【图片目录】
  默认监听: 用户\图片\Ref
  灵感模式: 用户\图片\Ref\Inspiration 与 用户\图片\Ref\Screenshots
  以上目录若不存在，程序会在首次使用时提示或自动创建。

【卸载】
  运行本文件夹内的 Uninstall.bat。卸载仅会：
  - 可选删除 %APPDATA%\DeckRef（配置与缓存，不含任何图片）；
  - 软件本体由您手动删除安装文件夹，或通过「设置 - 应用」卸载。
  不会删除您在「用户\图片\Ref」等目录中通过 DeckRef 保存的任何图片。
