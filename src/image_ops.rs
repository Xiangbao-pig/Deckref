//! 图片处理模块。纯函数式接口，所有操作返回 Result，禁止 unwrap。

use crate::error::{AppError, AppResult};
use image::{DynamicImage, ImageFormat};
use std::path::{Path, PathBuf};

/// 常见图片扩展名（与 watcher 一致），用于快捷抓图时判断是否为图片。
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp", "webp", "tiff", "tif"];

/// 按扩展名判断是否为支持的图片文件。
pub fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| IMAGE_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
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

/// 从路径加载图片，自动根据扩展名推断格式。
pub fn load_image(path: &Path) -> AppResult<DynamicImage> {
    let data = std::fs::read(path).map_err(AppError::Io)?;
    let format = ImageFormat::from_path(path).map_err(AppError::Image)?;
    image::load_from_memory_with_format(&data, format).map_err(AppError::Image)
}

/// 获取图片尺寸 (宽, 高)，不完整解码时可返回错误。
pub fn image_dimensions(path: &Path) -> AppResult<(u32, u32)> {
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
