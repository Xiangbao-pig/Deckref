//! Windows 构建：生成 DeckRef.ico 并嵌入 exe，供资源管理器/任务栏/快捷方式显示。
//! 若存在 assets/icon.png 或 assets/main icon.png 则从中生成 ICO，否则使用内置程序绘制。
//! 生成的 wix/DeckRef.ico 同时被 MSI 打包脚本复制到安装目录，供 WiX 用于开始菜单与“添加/删除程序”图标。

fn main() {
    if !cfg!(target_os = "windows") {
        return;
    }

    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let wix_dir = manifest_dir.join("wix");
    let ico_path = wix_dir.join("DeckRef.ico");

    std::fs::create_dir_all(&wix_dir).expect("create wix dir");

    let icon_dir = build_icon(&manifest_dir);
    let file = std::fs::File::create(&ico_path).expect("create DeckRef.ico");
    icon_dir.write(file).expect("write DeckRef.ico");

    let mut res = winres::WindowsResource::new();
    res.set_icon(ico_path.to_str().expect("ico path utf8"));
    if let Err(e) = res.compile() {
        eprintln!("winres: {}", e);
        // 未安装 MSVC 或 windres 时仅跳过嵌入，不阻塞构建
    }
}

/// 优先从 assets 下的 PNG 生成 ICO；若无则使用内置绘制。
fn build_icon(manifest_dir: &std::path::Path) -> ico::IconDir {
    let candidates = ["icon.png", "main icon.png"];
    for name in candidates {
        let path = manifest_dir.join("assets").join(name);
        if path.exists() {
            if let Ok(icon_dir) = build_icon_from_png(&path) {
                return icon_dir;
            }
        }
    }
    build_icon_fallback()
}

/// 从 PNG 文件生成 32x32 与 16x16 两档 ICO。
fn build_icon_from_png(path: &std::path::Path) -> std::io::Result<ico::IconDir> {
    let img = image::ImageReader::open(path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?
        .decode()
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let rgba = img.into_rgba8();

    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
    for (w, h) in [(32u32, 32u32), (16u32, 16u32)] {
        let resized = image::imageops::resize(
            &rgba,
            w,
            h,
            image::imageops::FilterType::Triangle,
        );
        let raw: Vec<u8> = resized.into_raw();
        let image = ico::IconImage::from_rgba_data(w, h, raw);
        let entry = ico::IconDirEntry::encode(&image).map_err(|e| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, e)
        })?;
        icon_dir.add_entry(entry);
    }
    Ok(icon_dir)
}

/// 内置回退：程序绘制的 32x32 / 16x16 ICO（深蓝底 + 白色 D 形）。
fn build_icon_fallback() -> ico::IconDir {
    let mut icon_dir = ico::IconDir::new(ico::ResourceType::Icon);
    for (w, h) in [(32u32, 32u32), (16u32, 16u32)] {
        let rgba = make_icon_rgba(w, h);
        let image = ico::IconImage::from_rgba_data(w, h, rgba);
        let entry = ico::IconDirEntry::encode(&image).expect("encode icon entry");
        icon_dir.add_entry(entry);
    }
    icon_dir
}

fn make_icon_rgba(width: u32, height: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let w = width as i32;
    let h = height as i32;
    let (r_bg, g_bg, b_bg) = (0x25, 0x63, 0xA8);
    let (r_fg, g_fg, b_fg) = (0xFF, 0xFF, 0xFF);
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let (r, g, b, a) = if in_d_shape(x, y, w, h) {
                (r_fg, g_fg, b_fg, 255)
            } else {
                let edge = edge_alpha(x, y, w, h);
                (r_bg, g_bg, b_bg, edge)
            };
            rgba[i] = r;
            rgba[i + 1] = g;
            rgba[i + 2] = b;
            rgba[i + 3] = a;
        }
    }
    rgba
}

fn in_d_shape(x: i32, y: i32, w: i32, h: i32) -> bool {
    let pad = (w.min(h) / 6).max(1);
    let left = pad;
    let right = w - pad;
    let top = pad;
    let bottom = h - pad;
    let mid_x = (left + right) / 2;
    let mid_y = (top + bottom) / 2;
    let rad_y = (bottom - top) / 2;
    if x < left {
        return false;
    }
    if x <= left + (right - left) / 4 {
        return true;
    }
    if x >= right {
        return false;
    }
    let dx = x - mid_x;
    let dy = y - mid_y;
    let in_circle = dx * dx * (rad_y * rad_y) + dy * dy * ((right - left) / 2) * ((right - left) / 2)
        <= (rad_y * rad_y) * ((right - left) / 2) * ((right - left) / 2);
    in_circle && dx >= 0
}

fn edge_alpha(x: i32, y: i32, w: i32, h: i32) -> u8 {
    let d = x.min(y).min((w - 1 - x).min(h - 1 - y));
    if d < 0 {
        return 0;
    }
    match d {
        0 => 0,
        1 => 80,
        2 => 180,
        _ => 255,
    }
}
