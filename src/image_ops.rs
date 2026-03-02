//! 图片处理模块。纯函数式接口，所有操作返回 Result，禁止 unwrap。

use crate::error::{AppError, AppResult};
use image::{DynamicImage, ImageFormat};
use std::path::Path;

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
