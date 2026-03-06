@echo off
chcp 65001 >nul
setlocal
echo ==========================================
echo   DeckRef 卸载
echo ==========================================
echo.
echo 【重要】本卸载仅会删除以下之一，不会删除您的任何图片或文档：
echo   - 软件本体：由您手动删除安装文件夹，或通过「设置 - 应用」卸载；
echo   - 配置与缓存：仅限 %APPDATA%\DeckRef 目录（窗口/设置/最近路径等）。
echo   您通过 DeckRef 保存的图片均在「用户\图片\Ref」等目录，不会被删除。
echo.
echo 配置与缓存位置: %APPDATA%\DeckRef
echo   仅含: window.txt, settings.txt, last_path.txt, recent_folders.txt（无图片）
echo.
set /p "CHOICE1=是否删除上述配置与数据 (Y/N)? "
if /i "%CHOICE1%"=="Y" (
    rd /s /q "%APPDATA%\DeckRef" 2>nul && echo 已删除配置与数据。 || echo 目录不存在或已删除。
) else (
    echo 已跳过，配置与数据保留。
)
echo.
echo DeckRef 产生的缓存也位于上述同一目录；清理后下次安装将重新开始。
set /p "CHOICE2=是否清理 DeckRef 产生的缓存 (Y/N)? 选 N 则下次安装可继续使用。 "
if /i "%CHOICE2%"=="Y" (
    rd /s /q "%APPDATA%\DeckRef" 2>nul && echo 已清理缓存。 || echo 目录不存在或已清理。
) else (
    echo 已跳过，下次安装可继续使用现有数据。
)
echo.
echo 若通过 MSI 安装：请到「设置 - 应用」中卸载 DeckRef。
echo 若为绿色版：请手动删除本程序所在文件夹以完成卸载。
echo.
pause
