@echo off
chcp 65001 >nul
setlocal
echo ==========================================
echo   DeckRef 卸载
echo ==========================================
echo.
echo 配置与数据位置: %APPDATA%\DeckRef
echo   含: window.txt, settings.txt, last_path.txt, recent_folders.txt
echo.
set /p "CHOICE=是否删除上述配置与数据 (Y/N)? "
if /i "%CHOICE%"=="Y" (
    rd /s /q "%APPDATA%\DeckRef" 2>nul && echo 已删除配置。 || echo 目录不存在或已删除。
) else (
    echo 已跳过，配置保留。
)
echo.
echo 请手动删除本程序所在文件夹(ANzhuang)以完成卸载。
echo.
pause
