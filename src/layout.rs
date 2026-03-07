//! 画板布局持久化：.deckref_layout.json（sidecar）读写与 layout_dirs 登记。
//! Phase 1：卡片 position/scale/order；Phase 2 将增加 groups 字段。

use crate::error::AppResult;
use crate::image_card::ImageCard;
use crate::window_state;
use eframe::egui::Vec2;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const LAYOUT_FILENAME: &str = ".deckref_layout.json";

/// 单卡在布局中的持久化数据（路径相对 watch_path）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutCard {
    /// 相对监听目录的文件名或相对路径
    pub path: String,
    #[serde(rename = "offset")]
    pub offset: [f32; 2],
    #[serde(rename = "rotation")]
    pub rotation: f32,
    #[serde(rename = "scale")]
    pub scale: f32,
}

/// 布局文件根结构；groups 预留 Phase 2。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeckRefLayout {
    pub version: u32,
    /// 保存时内容区尺寸 [宽, 高]，用于加载时按当前窗口比例缩放位置与缩放
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_available_size: Option<[f32; 2]>,
    pub cards: Vec<LayoutCard>,
    #[serde(default)]
    pub groups: Vec<serde_json::Value>,
}

impl Default for DeckRefLayout {
    fn default() -> Self {
        Self {
            version: 1,
            reference_available_size: None,
            cards: Vec::new(),
            groups: Vec::new(),
        }
    }
}

/// 将 watch_path 下的 .deckref_layout.json 读出并解析；无文件或解析失败返回 None。
pub fn load_layout(watch_path: &Path) -> Option<DeckRefLayout> {
    let path = watch_path.join(LAYOUT_FILENAME);
    let s = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&s).ok()
}

/// 将当前卡片列表写入 watch_path 下的 .deckref_layout.json；
/// 路径按相对 watch_path 存储；available_size 为保存时内容区尺寸，用于下次加载时按窗口比例缩放。
/// 成功后将该目录规范路径追加到 layout_dirs.txt。
pub fn save_layout(watch_path: &Path, cards: &[ImageCard], available_size: Vec2) -> AppResult<()> {
    let layout = layout_from_cards(watch_path, cards, available_size);
    let path = watch_path.join(LAYOUT_FILENAME);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(crate::error::AppError::Io)?;
    }
    let s = serde_json::to_string_pretty(&layout).map_err(|e| crate::error::AppError::Other(e.to_string()))?;
    std::fs::write(&path, s).map_err(crate::error::AppError::Io)?;
    if let Ok(canonical) = watch_path.canonicalize() {
        let _ = window_state::append_layout_dir(&canonical);
    }
    Ok(())
}

fn layout_from_cards(watch_path: &Path, cards: &[ImageCard], available_size: Vec2) -> DeckRefLayout {
    let cards: Vec<LayoutCard> = cards
        .iter()
        .map(|c| {
            let path = path_relative_to(watch_path, &c.path);
            let pos = c.board_saved_world_pos.unwrap_or(c.random_offset);
            LayoutCard {
                path,
                offset: [pos.x, pos.y],
                rotation: c.random_rotation,
                scale: c.display_scale,
            }
        })
        .collect();
    DeckRefLayout {
        version: 1,
        reference_available_size: Some([available_size.x, available_size.y]),
        cards,
        groups: Vec::new(),
    }
}

/// 路径转为相对 base 的字符串；若无法相对化则用文件名。
fn path_relative_to(base: &Path, full: &Path) -> String {
    if let Ok(rel) = full.strip_prefix(base) {
        rel.display().to_string().replace('\\', "/")
    } else {
        full.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string()
    }
}

/// 根据布局重排并应用每卡的 offset/rotation/scale；布局中不存在的卡片按原顺序追加到末尾并保留默认值。
/// 若 layout 带 reference_available_size 且 current_available_size 为 Some，则按比例缩放 offset 与 scale；
/// 否则不在此处缩放，由调用方在首帧获得真实画布尺寸后再缩放（见 layout_reference_size）。
pub fn apply_layout_to_cards(
    watch_path: &Path,
    cards: &mut Vec<ImageCard>,
    layout: &DeckRefLayout,
    current_available_size: Option<Vec2>,
) {
    if layout.cards.is_empty() {
        return;
    }
    let scale_u = layout.reference_available_size.and_then(|ref_sz| {
        let ref_v = Vec2::new(ref_sz[0], ref_sz[1]);
        current_available_size.filter(|c| c.x > 0.0 && c.y > 0.0 && ref_v.x > 0.0 && ref_v.y > 0.0)
            .map(|cur| {
                let sx = cur.x / ref_v.x;
                let sy = cur.y / ref_v.y;
                (sx + sy) / 2.0
            })
    }).unwrap_or(1.0);

    let use_board_absolute = layout.reference_available_size.is_some();
    let by_rel: std::collections::HashMap<String, LayoutCard> = layout
        .cards
        .iter()
        .cloned()
        .map(|c| (normalize_rel(&c.path), c))
        .collect();
    let mut ordered: Vec<ImageCard> = Vec::with_capacity(cards.len());
    for lc in &layout.cards {
        let key = normalize_rel(&lc.path);
        if let Some(pos) = cards.iter().position(|c| path_matches_rel(&c.path, watch_path, &key)) {
            let mut card = cards.remove(pos);
            if use_board_absolute {
                let ref_pos = Vec2::new(lc.offset[0], lc.offset[1]);
                card.board_saved_world_pos = Some(ref_pos);
                card.random_offset = ref_pos;
                card.display_scale = lc.scale.clamp(0.3, 5.0);
            } else {
                card.board_saved_world_pos = None;
                card.random_offset = Vec2::new(lc.offset[0] * scale_u, lc.offset[1] * scale_u);
                card.display_scale = (lc.scale * scale_u).clamp(0.3, 5.0);
            }
            card.random_rotation = lc.rotation;
            ordered.push(card);
        }
    }
    for card in cards.iter() {
        let rel = path_relative_to(watch_path, &card.path);
        let key = normalize_rel(&rel);
        if !by_rel.contains_key(&key) {
            ordered.push(card.clone());
        }
    }
    *cards = ordered;
}

fn normalize_rel(s: &str) -> String {
    s.replace('\\', "/").trim_matches('/').to_string()
}

fn path_matches_rel(full: &Path, base: &Path, rel_key: &str) -> bool {
    let rel = path_relative_to(base, full);
    normalize_rel(&rel) == rel_key
}
