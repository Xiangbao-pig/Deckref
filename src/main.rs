//! DeckRef 入口：置顶透明窗口，初始化 eframe，处理错误，不使用 unwrap。

use deckref::{AppError, DeckRefApp};
use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;

fn main() -> Result<(), AppError> {
    let watch_path = resolve_pictures_ref_dir()?;

    let icon = load_window_icon();
    let inner_size = deckref::window_state::initial_inner_size();
    let min_size = deckref::window_state::min_inner_size();
    let mut viewport = egui::ViewportBuilder::default()
        .with_transparent(true)
        .with_decorations(true)   // 标题栏：关闭、最小化按钮
        .with_resizable(true)     // 支持拖拽边框调整大小
        .with_always_on_top()
        .with_inner_size(inner_size)
        .with_min_inner_size(min_size)
        .with_drag_and_drop(true); // 接受从浏览器/文件夹拖入的图片；否则会显示禁止标志

    if let Some([px, py]) = deckref::window_state::initial_position() {
        viewport = viewport.with_position(egui::Pos2::new(px, py));
    }

    let viewport = if let Some(icon) = icon {
        viewport.with_icon(icon)
    } else {
        viewport
    };

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let result = eframe::run_native(
        "DeckRef",
        options,
        Box::new(move |cc| {
            configure_cjk_fonts(cc);
            match create_app(watch_path.clone()) {
                Ok(mut app) => {
                    #[cfg(windows)]
                    {
                        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
                        if let Ok(handle) = cc.window_handle().map(|h| h.as_raw()) {
                            if let RawWindowHandle::Win32(win) = handle {
                                let hwnd = win.hwnd.get() as isize;
                                app.set_win_drop_pending_hwnd(hwnd);
                            }
                        }
                    }
                    Ok(Box::new(app) as Box<dyn eframe::App>)
                }
                Err(e) => Err(Box::new(e) as Box<dyn std::error::Error + Send + Sync>),
            }
        }),
    );

    result.map_err(|e| AppError::Other(e.to_string()))
}

/// 解析 assets/icon.png 的路径：优先 manifest 目录（cargo run），其次 exe 同目录，再次当前目录。
fn resolve_assets_icon_path() -> Option<PathBuf> {
    deckref::image_ops::resolve_assets_icon_path()
}

/// 加载窗口图标（32x32 RGBA），失败时返回 None，不阻塞启动。
fn load_window_icon() -> Option<Arc<eframe::egui::IconData>> {
    let path = resolve_assets_icon_path()?;
    let img = image::ImageReader::open(path).ok()?.decode().ok()?;
    let img = img.into_rgba8();
    let (w, h) = (img.width(), img.height());
    let img = if w > 32 || h > 32 {
        image::imageops::resize(
            &img,
            32,
            32,
            image::imageops::FilterType::Triangle,
        )
    } else {
        img
    };
    let (w, h) = (img.width(), img.height());
    let rgba = img.into_raw();
    Some(Arc::new(eframe::egui::IconData {
        rgba,
        width: w,
        height: h,
    }))
}

/// 解析用户目录下的 Pictures/Ref 路径；若目录不存在则创建。
fn resolve_pictures_ref_dir() -> Result<PathBuf, AppError> {
    let user = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(AppError::EnvVar)?;
    let path = PathBuf::from(user).join("Pictures").join("Ref");
    std::fs::create_dir_all(&path).map_err(AppError::Io)?;
    Ok(path)
}

/// 配置中文字体，避免中文显示为方块。优先使用系统自带字体。
fn configure_cjk_fonts(cc: &eframe::CreationContext<'_>) {
    let mut fonts = egui::FontDefinitions::default();

    #[cfg(windows)]
    {
        let font_paths = [
            "C:\\Windows\\Fonts\\msyh.ttc",
            "C:\\Windows\\Fonts\\msyhbd.ttc",
            "C:\\Windows\\Fonts\\simsun.ttc",
        ];
        for path in font_paths {
            if let Ok(data) = std::fs::read(path) {
                let font = egui::FontData::from_owned(data);
                fonts.font_data.insert("cjk".to_string(), font);
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "cjk".to_string());
                fonts
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .insert(0, "cjk".to_string());
                break;
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        // macOS 系统自带中文字体：苹方、华文黑体、宋体-简 等
        let font_paths = [
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Supplemental/PingFang.ttc",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "/System/Library/Fonts/Supplemental/Songti.ttc",
            "/Library/Fonts/Arial Unicode.ttf",
        ];
        for path in font_paths {
            if let Ok(data) = std::fs::read(path) {
                let font = egui::FontData::from_owned(data);
                fonts.font_data.insert("cjk".to_string(), font);
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "cjk".to_string());
                fonts
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .insert(0, "cjk".to_string());
                break;
            }
        }
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Linux 等：常见系统字体目录
        let font_paths = [
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/truetype/fonts-chinese/uming.ttc",
        ];
        for path in font_paths {
            if let Ok(data) = std::fs::read(path) {
                let font = egui::FontData::from_owned(data);
                fonts.font_data.insert("cjk".to_string(), font);
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "cjk".to_string());
                fonts
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .insert(0, "cjk".to_string());
                break;
            }
        }
    }

    cc.egui_ctx.set_fonts(fonts);
}

fn create_app(watch_path: PathBuf) -> Result<DeckRefApp, AppError> {
    DeckRefApp::new(watch_path)
}
