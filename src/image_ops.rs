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

/// 当原图最长边超过此值时，使用缩略图加载以节省内存与解码时间（2K 屏上仍显示为高清）。
pub const THUMBNAIL_RESOLUTION_THRESHOLD: u32 = 2048;

/// 缩略图最长边像素数，保证在 2K（2560×1440）屏上堆叠预览仍清晰。
pub const THUMBNAIL_MAX_LONGEST_EDGE: u32 = 1200;

/// JPEG / WebP 可以在解码时按目标边长缩小，从而不必先得到整幅像素缓冲。
/// PNG、PSD、TIFF 等格式没有等价的解码期下采样，仍走解码后再缩放。
pub fn subsamples_when_downscaling(path: &Path) -> bool {
    matches!(
        extension_lower(path).as_deref(),
        Some("jpg") | Some("jpeg") | Some("webp")
    )
}

fn extension_lower(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
}

fn is_jpeg_path(path: &Path) -> bool {
    matches!(extension_lower(path).as_deref(), Some("jpg") | Some("jpeg"))
}

/// jpeg-decoder 的 IDCT 块边长：8 为原尺寸，4/2/1 分别为 1/2、1/4、1/8。
/// 最长边超过 [`THUMBNAIL_RESOLUTION_THRESHOLD`] 时不会返回 8，避免完整像素解码。
pub(crate) fn choose_jpeg_idct_scale(width: u32, height: u32, max_longest_edge: u32) -> u32 {
    let longest = width.max(height);
    if longest <= max_longest_edge || longest == 0 {
        return 8;
    }
    let (target_w, target_h) = thumbnail_target_size(width, height, max_longest_edge);
    for scale in [1u32, 2, 4] {
        let scaled_w = jpeg_scaled_dim(width, scale);
        let scaled_h = jpeg_scaled_dim(height, scale);
        if scaled_w >= target_w && scaled_h >= target_h {
            return scale;
        }
    }
    if longest > THUMBNAIL_RESOLUTION_THRESHOLD {
        4
    } else {
        8
    }
}

/// 与 jpeg-decoder `choose_idct_size` 相同的输出边长公式。
fn jpeg_scaled_dim(len: u32, idct_scale: u32) -> u32 {
    if idct_scale >= 8 || len == 0 {
        return len;
    }
    len.saturating_mul(idct_scale).saturating_sub(1) / 8 + 1
}

fn thumbnail_target_size(width: u32, height: u32, max_longest_edge: u32) -> (u32, u32) {
    let longest = width.max(height);
    if longest <= max_longest_edge || longest == 0 {
        return (width.max(1), height.max(1));
    }
    let scale = max_longest_edge as f64 / longest as f64;
    let nw = (width as f64 * scale).round().max(1.0) as u32;
    let nh = (height as f64 * scale).round().max(1.0) as u32;
    (nw, nh)
}

fn resize_down_to_longest(img: DynamicImage, max_longest_edge: u32) -> DynamicImage {
    use image::imageops::FilterType;
    let (w, h) = (img.width(), img.height());
    let longest = w.max(h);
    if longest <= max_longest_edge || longest == 0 {
        return img;
    }
    let (nw, nh) = thumbnail_target_size(w, h, max_longest_edge);
    img.resize_exact(nw, nh, FilterType::CatmullRom)
}

/// 让 jpeg-decoder 的 `scale` 选中指定 IDCT 块边长（1/2/4），而不是更小或完整尺寸。
fn jpeg_scale_request(width: u32, height: u32, idct_scale: u32) -> (u16, u16) {
    let to_u16 = |v: u32| v.clamp(1, u16::MAX as u32) as u16;
    match idct_scale {
        1 => (1, 1),
        2 => (
            to_u16(jpeg_scaled_dim(width, 1).saturating_add(1)),
            to_u16(jpeg_scaled_dim(height, 1).saturating_add(1)),
        ),
        4 => (
            to_u16(jpeg_scaled_dim(width, 2).saturating_add(1)),
            to_u16(jpeg_scaled_dim(height, 2).saturating_add(1)),
        ),
        _ => (to_u16(width.max(1)), to_u16(height.max(1))),
    }
}

fn jpeg_error(err: jpeg_decoder::Error) -> AppError {
    AppError::Other(format!("JPEG 缩放解码失败: {err}"))
}

/// 按 IDCT 缩小解码 JPEG，返回缩小后、最终 CatmullRom 之前的图像。
fn decode_jpeg_reduced(path: &Path, width: u32, height: u32, idct_scale: u32) -> AppResult<DynamicImage> {
    let file = std::fs::File::open(path).map_err(AppError::Io)?;
    let mut decoder = jpeg_decoder::Decoder::new(std::io::BufReader::new(file));
    let (request_w, request_h) = jpeg_scale_request(width, height, idct_scale);
    decoder.scale(request_w, request_h).map_err(jpeg_error)?;
    let pixels = decoder.decode().map_err(jpeg_error)?;
    let info = decoder
        .info()
        .ok_or_else(|| AppError::Other("JPEG 缺少解码信息".into()))?;
    let out_w = u32::from(info.width);
    let out_h = u32::from(info.height);
    if width.max(height) > THUMBNAIL_RESOLUTION_THRESHOLD && out_w.max(out_h) >= width.max(height) {
        return Err(AppError::Other("JPEG 缩略图仍为全分辨率".into()));
    }
    pixels_to_dynamic(info.pixel_format, out_w, out_h, pixels)
}

fn pixels_to_dynamic(
    format: jpeg_decoder::PixelFormat,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
) -> AppResult<DynamicImage> {
    use image::ImageBuffer;
    match format {
        jpeg_decoder::PixelFormat::RGB24 => {
            let buf = ImageBuffer::from_raw(width, height, pixels)
                .ok_or_else(|| AppError::Other("JPEG RGB 尺寸与像素数不匹配".into()))?;
            Ok(DynamicImage::ImageRgb8(buf))
        }
        jpeg_decoder::PixelFormat::L8 => {
            let buf = ImageBuffer::from_raw(width, height, pixels)
                .ok_or_else(|| AppError::Other("JPEG 灰度尺寸与像素数不匹配".into()))?;
            Ok(DynamicImage::ImageLuma8(buf))
        }
        jpeg_decoder::PixelFormat::L16 => {
            let expected = (width as usize).saturating_mul(height as usize).saturating_mul(2);
            if pixels.len() != expected {
                return Err(AppError::Other("JPEG 16 位灰度尺寸与像素数不匹配".into()));
            }
            let luma: Vec<u8> = pixels
                .chunks_exact(2)
                .map(|c| (u16::from_be_bytes([c[0], c[1]]) >> 8) as u8)
                .collect();
            let buf = ImageBuffer::from_raw(width, height, luma)
                .ok_or_else(|| AppError::Other("JPEG 16 位灰度尺寸与像素数不匹配".into()))?;
            Ok(DynamicImage::ImageLuma8(buf))
        }
        jpeg_decoder::PixelFormat::CMYK32 => {
            let expected = (width as usize).saturating_mul(height as usize).saturating_mul(4);
            if pixels.len() != expected {
                return Err(AppError::Other("JPEG CMYK 尺寸与像素数不匹配".into()));
            }
            let mut rgb = Vec::with_capacity(pixels.len() / 4 * 3);
            for px in pixels.chunks_exact(4) {
                let (c, m, y, k) = (px[0] as u32, px[1] as u32, px[2] as u32, px[3] as u32);
                rgb.push((255 - c.min(255).saturating_mul(255 - k) / 255) as u8);
                rgb.push((255 - m.min(255).saturating_mul(255 - k) / 255) as u8);
                rgb.push((255 - y.min(255).saturating_mul(255 - k) / 255) as u8);
            }
            let buf = ImageBuffer::from_raw(width, height, rgb)
                .ok_or_else(|| AppError::Other("JPEG CMYK 尺寸与像素数不匹配".into()))?;
            Ok(DynamicImage::ImageRgb8(buf))
        }
    }
}

fn load_jpeg_thumbnail(path: &Path, width: u32, height: u32, max_longest_edge: u32) -> AppResult<DynamicImage> {
    let idct_scale = choose_jpeg_idct_scale(width, height, max_longest_edge);
    let img = if idct_scale >= 8 {
        load_image(path)?
    } else {
        decode_jpeg_reduced(path, width, height, idct_scale)?
    };
    Ok(resize_down_to_longest(img, max_longest_edge))
}

/// 测试用：JPEG 在最终缩放到目标边长之前，IDCT 解码得到的尺寸。
fn jpeg_idct_output_size(path: &Path, max_longest_edge: u32) -> AppResult<(u32, u32)> {
    let (width, height) = image_dimensions(path)?;
    let idct_scale = choose_jpeg_idct_scale(width, height, max_longest_edge);
    if idct_scale >= 8 {
        return Ok((width, height));
    }
    let img = decode_jpeg_reduced(path, width, height, idct_scale)?;
    Ok((img.width(), img.height()))
}

fn load_webp_thumbnail(path: &Path, width: u32, height: u32, max_longest_edge: u32) -> AppResult<DynamicImage> {
    let (target_w, target_h) = thumbnail_target_size(width, height, max_longest_edge);
    let data = std::fs::read(path).map_err(AppError::Io)?;
    let (rgba, out_w, out_h) = decode_webp_scaled_rgba(&data, target_w, target_h)?;
    if width.max(height) > THUMBNAIL_RESOLUTION_THRESHOLD
        && (out_w as u64).saturating_mul(out_h as u64) >= (width as u64).saturating_mul(height as u64)
    {
        return Err(AppError::Other("WebP 缩略图仍为全分辨率".into()));
    }
    let buf = image::RgbaImage::from_raw(out_w, out_h, rgba)
        .ok_or_else(|| AppError::Other("WebP 缩略图像素数不匹配".into()))?;
    Ok(resize_down_to_longest(DynamicImage::ImageRgba8(buf), max_longest_edge))
}

/// 用 libwebp 的 `use_scaling` 直接解码到目标尺寸附近，输出缓冲不是原图像素尺寸。
fn decode_webp_scaled_rgba(data: &[u8], target_w: u32, target_h: u32) -> AppResult<(Vec<u8>, u32, u32)> {
    use libwebp_sys::{
        WebPDecode, WebPDecoderConfig, WebPFreeDecBuffer, WebPInitDecoderConfig, MODE_RGBA,
        VP8_STATUS_OK,
    };
    if data.is_empty() || target_w == 0 || target_h == 0 {
        return Err(AppError::Other("WebP 数据为空".into()));
    }
    unsafe {
        let mut config: WebPDecoderConfig = std::mem::zeroed();
        if WebPInitDecoderConfig(&mut config) == 0 {
            return Err(AppError::Other("WebP 解码器 ABI 不匹配".into()));
        }
        config.options.use_scaling = 1;
        config.options.scaled_width = i32::try_from(target_w).unwrap_or(i32::MAX);
        config.options.scaled_height = i32::try_from(target_h).unwrap_or(i32::MAX);
        config.output.colorspace = MODE_RGBA;
        let status = WebPDecode(data.as_ptr(), data.len(), &mut config);
        if status != VP8_STATUS_OK {
            WebPFreeDecBuffer(&mut config.output);
            return Err(AppError::Other(format!("WebP 缩放解码失败: {status}")));
        }
        let width = config.output.width;
        let height = config.output.height;
        let view = config.output.u.RGBA;
        let copy = copy_webp_rgba(view.rgba, view.stride, width, height);
        WebPFreeDecBuffer(&mut config.output);
        let (out_w, out_h, rgba) = copy?;
        Ok((rgba, out_w, out_h))
    }
}

unsafe fn copy_webp_rgba(
    ptr: *const u8,
    stride: i32,
    width: i32,
    height: i32,
) -> AppResult<(u32, u32, Vec<u8>)> {
    if ptr.is_null() || width <= 0 || height <= 0 || stride == 0 {
        return Err(AppError::Other("WebP 输出缓冲无效".into()));
    }
    let width_u = width as u32;
    let height_u = height as u32;
    let row_bytes = (width_u as usize).saturating_mul(4);
    let stride_abs = stride.unsigned_abs() as usize;
    if stride_abs < row_bytes {
        return Err(AppError::Other("WebP 行跨度小于像素宽度".into()));
    }
    let mut out = Vec::with_capacity(row_bytes.saturating_mul(height_u as usize));
    if stride > 0 {
        for y in 0..height_u as usize {
            let row = ptr.add(y.saturating_mul(stride_abs));
            out.extend_from_slice(std::slice::from_raw_parts(row, row_bytes));
        }
    } else {
        for y in 0..height_u as usize {
            let row = ptr.sub(y.saturating_mul(stride_abs));
            out.extend_from_slice(std::slice::from_raw_parts(row, row_bytes));
        }
    }
    Ok((width_u, height_u, out))
}

/// 加载图片并缩放到最长边不超过 `max_longest_edge` 的缩略图；若原图已更小则不缩放。
/// 用于高分辨率大图（如单反 50MB）的堆叠预览，在保证 2K 屏清晰的前提下显著降低解码与显存占用。
/// JPEG / WebP 在需要缩小时走解码期下采样；其余格式仍先解码再缩放。
pub fn load_image_thumbnail(path: &Path, max_longest_edge: u32) -> AppResult<DynamicImage> {
    let (width, height) = image_dimensions(path)?;
    let longest = width.max(height);
    if longest <= max_longest_edge || longest == 0 {
        return load_image(path);
    }
    if subsamples_when_downscaling(path) {
        let decoded = if is_jpeg_path(path) {
            load_jpeg_thumbnail(path, width, height, max_longest_edge)
        } else {
            load_webp_thumbnail(path, width, height, max_longest_edge)
        };
        return match decoded {
            Ok(img) => Ok(img),
            Err(err) if longest > THUMBNAIL_RESOLUTION_THRESHOLD => Err(err),
            Err(_) => {
                let img = load_image(path)?;
                Ok(resize_down_to_longest(img, max_longest_edge))
            }
        };
    }
    let img = load_image(path)?;
    Ok(resize_down_to_longest(img, max_longest_edge))
}

/// 供后台纹理加载线程使用：解码为 RGBA 字节（大图用缩略图），返回 (rgba_vec, width, height)。
pub fn load_image_rgba_for_texture(path: &Path) -> AppResult<(Vec<u8>, u32, u32)> {
    let (w, h) = image_dimensions(path)?;
    let img = if w.max(h) > THUMBNAIL_RESOLUTION_THRESHOLD {
        load_image_thumbnail(path, THUMBNAIL_MAX_LONGEST_EDGE)?
    } else {
        load_image(path)?
    };
    let (width, height) = (img.width(), img.height());
    let rgba = img.to_rgba8();
    Ok((rgba.into_raw(), width, height))
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

    fn write_solid(path: &Path, w: u32, h: u32, format: image::ImageFormat) {
        let buf = image::RgbImage::from_pixel(w, h, image::Rgb([32, 64, 96]));
        let img = image::DynamicImage::ImageRgb8(buf);
        img.save_with_format(path, format).expect("write image");
    }

    #[test]
    fn load_image_thumbnail_caps_longest_edge_on_large_jpeg() {
        let path = std::env::temp_dir().join(format!(
            "deckref_thumb_{}.jpg",
            std::process::id()
        ));
        write_solid(&path, 4000, 3000, image::ImageFormat::Jpeg);
        let img = load_image_thumbnail(&path, THUMBNAIL_MAX_LONGEST_EDGE).expect("thumbnail");
        let _ = std::fs::remove_file(&path);
        assert!(img.width().max(img.height()) <= THUMBNAIL_MAX_LONGEST_EDGE);
        assert!(img.width() > 0 && img.height() > 0);
    }

    #[test]
    fn large_jpeg_idct_decode_is_smaller_than_source_but_covers_target() {
        let path = std::env::temp_dir().join(format!(
            "deckref_idct_{}.jpg",
            std::process::id()
        ));
        write_solid(&path, 4000, 3000, image::ImageFormat::Jpeg);
        let (w, h) = jpeg_idct_output_size(&path, THUMBNAIL_MAX_LONGEST_EDGE).expect("idct size");
        let _ = std::fs::remove_file(&path);
        assert!(w < 4000 && h < 3000, "full-resolution decode: {w}x{h}");
        assert!(w.max(h) >= THUMBNAIL_MAX_LONGEST_EDGE);
    }

    #[test]
    fn oversized_jpeg_never_selects_full_idct() {
        assert_ne!(choose_jpeg_idct_scale(4000, 3000, THUMBNAIL_MAX_LONGEST_EDGE), 8);
        assert_eq!(choose_jpeg_idct_scale(4000, 3000, THUMBNAIL_MAX_LONGEST_EDGE), 4);
        // 1/2 of 2200 is under 1200; still must not fall back to a full decode.
        assert_eq!(choose_jpeg_idct_scale(2200, 1400, THUMBNAIL_MAX_LONGEST_EDGE), 4);
        assert_eq!(choose_jpeg_idct_scale(800, 600, THUMBNAIL_MAX_LONGEST_EDGE), 8);
    }

    #[test]
    fn subsamples_when_downscaling_is_jpeg_and_webp_only() {
        assert!(subsamples_when_downscaling(Path::new("a.jpg")));
        assert!(subsamples_when_downscaling(Path::new("b.JPEG")));
        assert!(subsamples_when_downscaling(Path::new("c.webp")));
        assert!(!subsamples_when_downscaling(Path::new("d.png")));
        assert!(!subsamples_when_downscaling(Path::new("e.tif")));
        assert!(!subsamples_when_downscaling(Path::new("f.psd")));
    }

    #[test]
    fn load_image_thumbnail_caps_large_webp_and_png_fallback() {
        let webp = std::env::temp_dir().join(format!("deckref_thumb_{}.webp", std::process::id()));
        write_solid(&webp, 2600, 1800, image::ImageFormat::WebP);
        let img = load_image_thumbnail(&webp, THUMBNAIL_MAX_LONGEST_EDGE).expect("webp thumb");
        let _ = std::fs::remove_file(&webp);
        assert!(img.width().max(img.height()) <= THUMBNAIL_MAX_LONGEST_EDGE);
        assert!(img.width() < 2600 && img.height() < 1800);

        let png = std::env::temp_dir().join(format!("deckref_thumb_{}.png", std::process::id()));
        write_solid(&png, 80, 40, image::ImageFormat::Png);
        let small = load_image_thumbnail(&png, 20).expect("png thumb");
        let _ = std::fs::remove_file(&png);
        assert_eq!(small.width().max(small.height()), 20);
    }
}
