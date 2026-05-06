//! 顶层地址栏与工具按钮（叠加层）。

use super::DeckRefApp;
use crate::app_nav::{
    breadcrumb_collapse_start, list_drives_windows, list_subdirs, path_breadcrumbs,
    THIS_PC_SENTINEL,
};
use crate::app_shell_layout::TOP_BAR_HEIGHT;
use crate::image_card::CardSortOrder;
use crate::mode::AppMode;
use eframe::egui::{self, Rect};
use std::path::PathBuf;

pub(super) fn show_top_bar(app: &mut DeckRefApp, ctx: &egui::Context) {
    let screen = ctx.screen_rect();
    let toolbar_rect = Rect::from_min_max(
        screen.min,
        egui::Pos2::new(screen.max.x, screen.min.y + TOP_BAR_HEIGHT),
    );
    egui::Area::new(egui::Id::new("deckref_toolbar"))
        .order(egui::Order::Foreground)
        .fixed_pos(toolbar_rect.min)
        .constrain(true)
        .show(ctx, |ui| {
            ui.set_min_size(egui::Vec2::new(screen.width(), TOP_BAR_HEIGHT));
            let fill = ui.visuals().window_fill();
            let c = egui::Color32::from(fill);
            let a = (c.a() as f32 * 0.04).round() as u8;
            let bg = egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
            ui.painter().rect_filled(
                Rect::from_min_size(egui::Pos2::ZERO, ui.available_size()),
                egui::Rounding::same(0.0),
                bg,
            );
            ui.horizontal(|ui| {
                if app.mode == AppMode::Default || app.mode == AppMode::Board {
                    let recent = app.recent_folders.clone();
                    let current_path = PathBuf::from(&app.watch_path);
                    egui::menu::menu_button(ui, "最近访问", |ui| {
                        ui.set_max_width(320.0);
                        ui.set_max_height(220.0);
                        ui.label(egui::RichText::new("最近到访").size(12.0).strong());
                        ui.separator();
                        if recent.is_empty() {
                            ui.label(egui::RichText::new("暂无").color(ui.visuals().weak_text_color()));
                        } else {
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                for path in &recent {
                                    if *path == current_path {
                                        continue;
                                    }
                                    let s = path.display().to_string();
                                    let truncate = if s.len() > 45 {
                                        let mut start = s.len().saturating_sub(44);
                                        while start < s.len() && !s.is_char_boundary(start) {
                                            start += 1;
                                        }
                                        format!("…{}", &s[start..])
                                    } else {
                                        s.clone()
                                    };
                                    if ui.button(truncate).on_hover_text(s).clicked() {
                                        app.navigate_watch_path(path.clone(), true);
                                        ui.close_menu();
                                    }
                                }
                            });
                        }
                    })
                    .response
                    .on_hover_text("最近到访的 5 个文件夹，点击快速切换（有缓存）");
                    ui.separator();
                }
                if app.mode == AppMode::Default || app.mode == AppMode::Board {
                    if ui.input(|i| i.key_pressed(egui::Key::L) && (i.modifiers.ctrl || i.modifiers.mac_cmd)) {
                        ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::L));
                        ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::L));
                        app.address_bar_editing = true;
                        app.address_bar_edit_text = app.watch_path.clone();
                        app.address_bar_edit_request_focus = true;
                    }

                    let can_back = app.nav_index > 0;
                    let can_forward = app.nav_index < app.nav_history.len().saturating_sub(1);
                    if ui.add_enabled(can_back, egui::Button::new(" ◀ ").frame(false))
                        .on_hover_text("后退")
                        .clicked()
                    {
                        app.nav_back();
                    }
                    if ui.add_enabled(can_forward, egui::Button::new(" ▶ ").frame(false))
                        .on_hover_text("前进")
                        .clicked()
                    {
                        app.nav_forward();
                    }
                    ui.add_space(4.0);

                    if app.address_bar_editing {
                        let id = egui::Id::new("addr_bar_edit");
                        if app.address_bar_edit_request_focus {
                            ui.ctx().memory_mut(|m| m.request_focus(id));
                            app.address_bar_edit_request_focus = false;
                        }
                        let te = egui::TextEdit::singleline(&mut app.address_bar_edit_text)
                            .id(id)
                            .desired_width(ui.available_width().min(500.0))
                            .font(egui::TextStyle::Body);
                        let resp = ui.add(te);
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            let s = app.address_bar_edit_text.trim();
                            if !s.is_empty() {
                                let p = PathBuf::from(s);
                                let p = crate::app_nav::normalize_watch_path_for_nav(p);
                                let path = std::fs::canonicalize(&p).unwrap_or(p);
                                if path.is_dir() {
                                    app.navigate_watch_path(path, true);
                                }
                            }
                            app.address_bar_editing = false;
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            app.address_bar_editing = false;
                        }
                        if resp.lost_focus() {
                            app.address_bar_editing = false;
                        }
                        let _ = resp;
                    } else {
                        let watch_path_buf = PathBuf::from(&app.watch_path);
                        let segments = path_breadcrumbs(&watch_path_buf);
                        let arrow_color = ui.visuals().weak_text_color();
                        let screen_w = ui.ctx().screen_rect().width();
                        const RESERVE_LEFT_RIGHT: f32 = 280.0;
                        let avail_breadcrumb = (screen_w - RESERVE_LEFT_RIGHT).max(40.0);
                        let (start_index, _seg_widths, _sep_w, _ellipsis_btn_w) =
                            breadcrumb_collapse_start(ui, &segments, avail_breadcrumb);

                        let _ = ui.allocate_ui_with_layout(
                            egui::Vec2::new(avail_breadcrumb, TOP_BAR_HEIGHT),
                            egui::Layout::left_to_right(egui::Align::Min),
                            |ui| {
                                egui::ScrollArea::horizontal()
                                    .id_salt("addr_bar_breadcrumb")
                                    .max_height(TOP_BAR_HEIGHT - 4.0)
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            if start_index > 0 {
                                                let resp = egui::menu::menu_button(
                                                    ui,
                                                    egui::RichText::new("…").size(12.0).color(arrow_color),
                                                    |ui| {
                                                        ui.set_max_width(280.0);
                                                        ui.set_max_height(200.0);
                                                        egui::ScrollArea::vertical().show(ui, |ui| {
                                                            for (path, label) in segments.iter().take(start_index) {
                                                                let label_str = label.to_string_lossy();
                                                                if ui.button(label_str.as_ref()).clicked() {
                                                                    if path.as_os_str() != THIS_PC_SENTINEL
                                                                        && path.is_dir()
                                                                    {
                                                                        app.navigate_watch_path(path.clone(), true);
                                                                    }
                                                                    ui.close_menu();
                                                                }
                                                            }
                                                        });
                                                    },
                                                );
                                                if resp.response.hovered() {
                                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                                }
                                                resp.response.on_hover_text("被省略的上级路径，点击可跳转");
                                                ui.add(
                                                    egui::Label::new(
                                                        egui::RichText::new(" › ").color(arrow_color),
                                                    )
                                                    .selectable(false),
                                                );
                                            }
                                            for (i, (path, label)) in segments.iter().enumerate() {
                                                if i < start_index {
                                                    continue;
                                                }
                                                let is_sentinel = path.as_os_str() == THIS_PC_SENTINEL;
                                                let is_current = i == segments.len() - 1;
                                                let label_str = label.to_string_lossy();
                                                let style = if is_current {
                                                    egui::RichText::new(label_str.as_ref()).size(12.0).strong()
                                                } else {
                                                    egui::RichText::new(label_str.as_ref()).size(12.0)
                                                };

                                                let seg_btn =
                                                    egui::Button::new(style).frame(true).min_size(egui::Vec2::ZERO);
                                                let seg_resp = ui.add(seg_btn);
                                                if seg_resp.hovered() {
                                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                                }
                                                if seg_resp.clicked() {
                                                    if is_sentinel {
                                                        #[cfg(windows)]
                                                        {
                                                            let drives = list_drives_windows();
                                                            if let Some(first) = drives.first() {
                                                                app.navigate_watch_path(first.clone(), true);
                                                            }
                                                        }
                                                    } else if path.is_dir() {
                                                        app.navigate_watch_path(path.clone(), true);
                                                    }
                                                }
                                                seg_resp.on_hover_text(if is_current {
                                                    "当前监听目录"
                                                } else {
                                                    "点击切换到该目录"
                                                });

                                                if is_sentinel {
                                                    #[cfg(windows)]
                                                    {
                                                        let drives = list_drives_windows();
                                                        egui::menu::menu_button(
                                                            ui,
                                                            egui::RichText::new(" › ").color(arrow_color),
                                                            |ui| {
                                                                ui.set_max_width(200.0);
                                                                ui.set_max_height(200.0);
                                                                egui::ScrollArea::vertical().show(ui, |ui| {
                                                                    for drive in &drives {
                                                                        let name = drive.display().to_string();
                                                                        if ui.button(name.as_str()).clicked() {
                                                                            app.navigate_watch_path(drive.clone(), true);
                                                                            ui.close_menu();
                                                                        }
                                                                    }
                                                                });
                                                            },
                                                        );
                                                    }
                                                    #[cfg(not(windows))]
                                                    {
                                                        ui.add(
                                                            egui::Label::new(
                                                                egui::RichText::new(" › ").color(arrow_color),
                                                            )
                                                            .selectable(false),
                                                        );
                                                    }
                                                } else {
                                                    let subdirs = list_subdirs(path, 80);
                                                    if !subdirs.is_empty() {
                                                        egui::menu::menu_button(
                                                            ui,
                                                            egui::RichText::new(" › ").color(arrow_color),
                                                            |ui| {
                                                                ui.set_max_width(220.0);
                                                                ui.set_max_height(200.0);
                                                                egui::ScrollArea::vertical().show(ui, |ui| {
                                                                    for subdir in &subdirs {
                                                                        let name = subdir
                                                                            .file_name()
                                                                            .unwrap_or_default()
                                                                            .to_string_lossy();
                                                                        if ui.button(name.as_ref()).clicked() {
                                                                            app.navigate_watch_path(
                                                                                subdir.clone(),
                                                                                true,
                                                                            );
                                                                            ui.close_menu();
                                                                        }
                                                                    }
                                                                });
                                                            },
                                                        );
                                                    } else {
                                                        ui.add(
                                                            egui::Label::new(
                                                                egui::RichText::new(" › ").color(arrow_color),
                                                            )
                                                            .selectable(false),
                                                        );
                                                    }
                                                }
                                            }
                                        });
                                    });
                            },
                        );

                        let blank_id = egui::Id::new("addr_blank");
                        let avail = ui.available_width();
                        if avail > 4.0 {
                            let blank_resp = ui.allocate_ui(egui::Vec2::new(avail, TOP_BAR_HEIGHT), |ui| {
                                let rect = ui.available_rect_before_wrap();
                                ui.interact(rect, blank_id, egui::Sense::click())
                            });
                            if blank_resp.inner.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
                            }
                            if blank_resp.inner.clicked() {
                                app.address_bar_editing = true;
                                app.address_bar_edit_text = app.watch_path.clone();
                                app.address_bar_edit_request_focus = true;
                            }
                            blank_resp
                                .response
                                .on_hover_text("点击或 Ctrl+L 输入/粘贴路径");
                        }
                    }
                } else {
                    ui.label(format!("监听: {}", app.watch_path)).on_hover_text(
                        "从 Chrome 拖入无效时，可复制图片后 Ctrl+V（Mac：Cmd+V）粘贴保存",
                    );
                }
                ui.add_space(8.0);
                if ui.button("按名称").clicked() {
                    app.sort_order = CardSortOrder::Name;
                    app.apply_sort();
                }
                if ui.button("按大小").clicked() {
                    app.sort_order = CardSortOrder::Size;
                    app.apply_sort();
                }
                if ui.button("按修改时间").clicked() {
                    app.sort_order = CardSortOrder::Modified;
                    app.apply_sort();
                }
                if ui.button("清空").clicked() {
                    app.cards.clear();
                    app.texture_cache.clear();
                    app.texture_cache_scope.clear();
                    app.pending_texture_paths.clear();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(egui::RichText::new("⚙").size(16.0))
                        .on_hover_text("设置")
                        .clicked()
                    {
                        app.settings_open = true;
                    }
                });
            });
        });
}
