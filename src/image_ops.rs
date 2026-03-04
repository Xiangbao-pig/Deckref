//! 图片处理模块。纯函数式接口，所有操作返回 Result，禁止 unwrap。

use crate::error::{AppError, AppResult};
use image::{DynamicImage, ImageFormat};
use std::path::{Path, PathBuf};

/// 解析 assets 下 logo/窗口图标的路径：优先 icon.png，其次 main icon.png，再则任意含 icon 的 png。
/// 查找顺序：manifest 目录（cargo run）→ exe 同目录 → 当前目录。
pub fn resolve_assets_icon_path() -> Option<PathBuf> {
    let try_root = |root: &Path| -> Option<PathBuf> {
        let assets_dir = root.join("assets");
        let fixed = ["icon.png", "main icon.png"];
        for name in &fixed {
            let p = assets_dir.join(name);
            if p.exists() {
                return Some(p);
            }
        }
        // 支持替换后的长文件名等：assets 下任意含 "icon" 的 png
        if let Ok(entries) = std::fs::read_dir(&assets_dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().map_or(false, |e| e.eq_ignore_ascii_case("png"))
                    && p.file_stem()
                        .and_then(|s| s.to_str())
                        .map_or(false, |s| s.to_lowercase().contains("icon"))
                {
                    return Some(p);
                }
            }
        }
        None
    };
    #[allow(deprecated)]
    if let Some(root) = option_env!("CARGO_MANIFEST_DIR").map(PathBuf::from) {
        if let Some(p) = try_root(&root) {
            return Some(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(root) = exe.parent() {
            if let Some(p) = try_root(root) {
                return Some(p);
            }
        }
    }
    if let Ok(root) = std::env::current_dir() {
        if let Some(p) = try_root(&root) {
            return Some(p);
        }
    }
    None
}

/// 标准预览图片扩展名（小写）。
pub const STANDARD_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp", "tiff", "tif"];

/// 扩展格式扩展名（image 库可解码）；开启「预览专业格式」时与标准合并使用。
const EXTENDED_FULL: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "bmp", "webp", "tiff", "tif",
    "tga", "ico", "dds", "avif", "exr", "hdr", "pnm", "pbm", "pgm", "ppm", "qoi", "ff", "psd",
];

/// 返回当前应使用的图片扩展名列表。`extended == true` 时包含专业/扩展格式。
pub fn get_image_extensions(extended: bool) -> &'static [&'static str] {
    if extended {
        EXTENDED_FULL
    } else {
        STANDARD_EXTENSIONS
    }
}

/// 常见图片扩展名（与 watcher 一致），用于快捷抓图时判断是否为图片。
#[allow(dead_code)]
const IMAGE_EXTENSIONS: &[&str] = STANDARD_EXTENSIONS;

/// 按扩展名判断是否为支持的图片文件。`extended == true` 时包含专业格式（TGA、ICO、DDS、AVIF 等）。
pub fn is_image_path_with(path: &Path, extended: bool) -> bool {
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e.to_lowercase(),
        None => return false,
    };
    get_image_extensions(extended).iter().any(|s| *s == ext.as_str())
}

/// 按扩展名判断是否为支持的图片文件（仅标准格式）。
pub fn is_image_path(path: &Path) -> bool {
    is_image_path_with(path, false)
}

/// 判断 `path` 是否在 `dir` 下（同一目录或其子路径）。用于快捷抓图去重：已在监听目录的文件不再复制。
pub fn path_is_under(path: &Path, dir: &Path) -> bool {
    let path = match std::fs::canonicalize(path) {
        Ok(p) => p,
        Err(_) => return false,
    };
    let dir = match std::fs::canonicalize(dir) {
        Ok(d) => d,
        Err(_) => return false,
    };
    path.starts_with(&dir)
}

/// 从路径加载图片，自动根据扩展名推断格式。支持 PSD（需开启预览专业格式时才会被列入目录，此处仅解码）。
pub fn load_image(path: &Path) -> AppResult<DynamicImage> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());
    if ext.as_deref() == Some("psd") {
        return load_psd(path);
    }
    let data = std::fs::read(path).map_err(AppError::Io)?;
    let format = ImageFormat::from_path(path).map_err(AppError::Image)?;
    image::load_from_memory_with_format(&data, format).map_err(AppError::Image)
}

/// 从 PSD 文件加载为 DynamicImage（RGBA）。失败时返回 AppError。
fn load_psd(path: &Path) -> AppResult<DynamicImage> {
    use image::ImageBuffer;
    let data = std::fs::read(path).map_err(AppError::Io)?;
    let psd = psd::Psd::from_bytes(&data).map_err(|e| AppError::Other(e.to_string()))?;
    let w = psd.width();
    let h = psd.height();
    let rgba = psd.rgba();
    let buf = ImageBuffer::from_raw(w, h, rgba.to_vec())
        .ok_or_else(|| AppError::Other("PSD 图像尺寸与像素数不匹配".into()))?;
    Ok(DynamicImage::ImageRgba8(buf))
}

/// 获取图片尺寸 (宽, 高)，不完整解码时可返回错误。支持 PSD。
pub fn image_dimensions(path: &Path) -> AppResult<(u32, u32)> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());
    if ext.as_deref() == Some("psd") {
        let data = std::fs::read(path).map_err(AppError::Io)?;
        let psd = psd::Psd::from_bytes(&data).map_err(|e| AppError::Other(e.to_string()))?;
        return Ok((psd.width(), psd.height()));
    }
    image::image_dimensions(path).map_err(AppError::Image)
}

/// 将图片保存到路径，格式由扩展名决定。
pub fn save_image(img: &DynamicImage, path: &Path) -> AppResult<()> {
    let format = ImageFormat::from_path(path).map_err(AppError::Image)?;
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    img.write_to(&mut cursor, format).map_err(AppError::Image)?;
    std::fs::write(path, buf).map_err(AppError::Io)?;
    Ok(())
}

/// 从内存中的图片数据解码并保存到指定路径；格式由路径扩展名决定，无扩展名则用 PNG。
/// 用于从浏览器等拖入的纯字节数据（无本地路径）。
pub fn save_image_from_bytes(data: &[u8], path: &Path) -> AppResult<()> {
    let img = image::load_from_memory(data).map_err(AppError::Image)?;
    let format = ImageFormat::from_path(path).unwrap_or(ImageFormat::Png);
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    img.write_to(&mut cursor, format).map_err(AppError::Image)?;
    std::fs::write(path, buf).map_err(AppError::Io)?;
    Ok(())
}

/// 将原始 RGBA 像素数据（行优先，每像素 4 字节）保存为 PNG。用于剪贴板粘贴等。
pub fn save_rgba_as_png(
    width: u32,
    height: u32,
    rgba: &[u8],
    path: &Path,
) -> AppResult<()> {
    use image::ImageBuffer;
    let buf = ImageBuffer::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| AppError::Other("剪贴板图像尺寸与字节数不匹配".into()))?;
    let img = image::DynamicImage::ImageRgba8(buf);
    let mut out = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut out);
    img.write_to(&mut cursor, ImageFormat::Png).map_err(AppError::Image)?;
    std::fs::write(path, out).map_err(AppError::Io)?;
    Ok(())
}

/// 将本地图片文件复制到目标目录，使用唯一文件名（原主名_时间戳.扩展名），避免覆盖。
/// 仅当源路径为支持的图片格式时复制；返回新文件路径。
pub fn copy_image_to_dir(src: &Path, dest_dir: &Path) -> AppResult<PathBuf> {
    if !src.is_file() || !is_image_path(src) {
        return Err(AppError::Other("非图片文件或路径无效".into()));
    }
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("image");
    let ext = src
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");
    let sanitized = sanitize_filename_stem(stem);
    let timestamp = chrono_timestamp();
    let name = format!("{}_{}.{}", sanitized, timestamp, ext);
    let dest_path = dest_dir.join(&name);
    std::fs::copy(src, &dest_path).map_err(AppError::Io)?;
    Ok(dest_path)
}

/// 在目标目录生成一个唯一文件名（capture_时间戳.png），用于无文件名时的抓图保存。
pub fn unique_capture_path(dest_dir: &Path) -> PathBuf {
    let name = format!("capture_{}.png", chrono_timestamp());
    dest_dir.join(name)
}

/// 在截图目录生成下一个序号文件名 screenshot-N.png（N 为当前目录中已有 screenshot-*.png 的最大编号+1）。
/// 用于运行时监听到剪贴板截图后自动保存到截图文件夹。
pub fn next_screenshot_path(screenshot_dir: &Path) -> PathBuf {
    let mut max_n: u32 = 0;
    if let Ok(entries) = std::fs::read_dir(screenshot_dir) {
        for e in entries.flatten() {
            let path = e.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.starts_with("screenshot-") && name.ends_with(".png") {
                let num: Option<u32> = name
                    .trim_start_matches("screenshot-")
                    .trim_end_matches(".png")
                    .parse()
                    .ok();
                if let Some(n) = num {
                    max_n = max_n.max(n);
                }
            }
        }
    }
    screenshot_dir.join(format!("screenshot-{}.png", max_n + 1))
}

/// 列出截图目录下 screenshot-*.png 文件，按编号升序（screenshot-1, screenshot-2, ...）。用于灵感模式合并展示。
pub fn list_screenshot_paths_ordered(screenshot_dir: &Path) -> Vec<PathBuf> {
    let mut pairs: Vec<(u32, PathBuf)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(screenshot_dir) {
        for e in entries.flatten() {
            let path = e.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if name.starts_with("screenshot-") && name.ends_with(".png") {
                let num: Option<u32> = name
                    .trim_start_matches("screenshot-")
                    .trim_end_matches(".png")
                    .parse()
                    .ok();
                if let Some(n) = num {
                    pairs.push((n, path));
                }
            }
        }
    }
    pairs.sort_by_key(|(n, _)| *n);
    pairs.into_iter().map(|(_, p)| p).collect()
}

/// 用于 Windows 虚拟文件拖入：在目标目录生成唯一路径（drop_时间戳_序号.扩展名），避免覆盖。
pub fn unique_path_for_drop(dest_dir: &Path, index: u32) -> PathBuf {
    let ext = "png";
    let name = format!("drop_{}_{}.{}", chrono_timestamp(), index, ext);
    dest_dir.join(name)
}

fn sanitize_filename_stem(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(80)
        .collect()
}

fn chrono_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}_{:03}", d.as_secs(), d.subsec_millis())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn is_image_path_accepts_supported_extensions() {
        assert!(is_image_path(Path::new("a.png")));
        assert!(is_image_path(Path::new("b.JPG")));
        assert!(is_image_path(Path::new("c.jpeg")));
        assert!(is_image_path(Path::new("d.gif")));
        assert!(is_image_path(Path::new("e.webp")));
    }

    #[test]
    fn is_image_path_rejects_non_image() {
        assert!(!is_image_path(Path::new("x.txt")));
        assert!(!is_image_path(Path::new("y")));
        assert!(!is_image_path(Path::new("z.png.exe")));
    }

    #[test]
    fn unique_capture_path_has_expected_format() {
        let dir = Path::new(".");
        let p = unique_capture_path(dir);
        assert!(p.file_name().unwrap().to_str().unwrap().starts_with("capture_"));
        assert!(p.extension().map_or(false, |e| e == "png"));
    }

    #[test]
    fn unique_path_for_drop_has_expected_format() {
        let dir = Path::new(".");
        let p = unique_path_for_drop(dir, 0);
        assert!(p.file_name().unwrap().to_str().unwrap().starts_with("drop_"));
        assert!(p.to_string_lossy().ends_with("_0.png"));
    }
}
