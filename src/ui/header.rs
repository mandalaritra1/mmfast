//! The channel header bar: name, members, pinned, the header text (click
//! to expand and edit), and the search field.

use super::{Icon, elided, semibold};
use crate::app::{Action, App, Rhs};
use crate::markup;
use egui::{Align, Layout, RichText, Sense, Ui, vec2};

pub fn show(app: &mut App, ui: &mut Ui) {
    let App { d, u, actions, .. } = app;
    let t = d.theme.clone();
    let Some(ch) = d.current_channel().cloned() else { return };
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        // Search, at the right.
        let id = egui::Id::new("search-field");
        let field = egui::TextEdit::singleline(&mut u.search_text)
            .id(id)
            .hint_text("Search messages")
            .desired_width(if ui.available_width() < 700.0 { 170.0 } else { 240.0 })
            .margin(vec2(28.0, 5.0));
        let resp = ui.add(field);
        Icon::Search.image(t.text_dim, 14.0).paint_at(
            ui,
            egui::Rect::from_center_size(egui::pos2(resp.rect.left() + 14.0, resp.rect.center().y), vec2(14.0, 14.0)),
        );
        if u.focus_search {
            resp.request_focus();
            u.focus_search = false;
        }
        if resp.changed() {
            actions.push(Action::Search { server: false });
        }
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && !u.search_text.trim().is_empty() {
            actions.push(Action::Search { server: true });
        }
        resp.on_hover_text("Enter searches the server. Supports from:, in:, before:, after:, \"phrases\" and -exclusions.");
        ui.add_space(8.0);

        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let icon = match ch.kind.as_str() {
                "O" => Some(Icon::Hash),
                "P" => Some(Icon::Lock),
                "G" => Some(Icon::Users),
                _ => None,
            };
            if let Some(icon) = icon {
                ui.add(icon.image(t.text, 16.0));
            } else if let Some(partner) = ch.dm_partner(&d.me.id) {
                super::avatar(ui, &mut u.images, d, &partner, 22.0);
            }
            let name = d.channel_name(&ch);
            // Narrow (a side panel open, a small window): the name gets what is
            // left after the icons, and the header text gives way first.
            let room = ui.available_width();
            let g = elided(ui, &name, semibold(16.5), t.text, (room - 150.0).clamp(60.0, 260.0));
            ui.label(g);
            ui.add_space(4.0);
            let stats = d.stats.get(&ch.id);
            if !ch.is_direct()
                && room > 320.0
                && let Some(s) = stats
            {
                chip(ui, &t, Icon::Users, s.member_count, "Members");
            }
            let pinned = stats.map(|s| s.pinnedpost_count).unwrap_or(0);
            let pinned_active = matches!(d.rhs, Rhs::Pinned(_));
            if chip_button(ui, &t, Icon::Pin, pinned, "Pinned messages", pinned_active) {
                actions.push(Action::OpenPinned);
            }
            let fav = d.favorite(&ch.id);
            let star = if fav { Icon::StarFilled.image(egui::Color32::from_rgb(0xf5, 0xb3, 0x01), 15.0) } else { Icon::Star.image(t.text_dim, 15.0) };
            let r = ui.add(egui::Button::image(star).frame(false)).on_hover_text(if fav { "Remove from favorites" } else { "Add to favorites" });
            if r.clicked() {
                actions.push(Action::ToggleFavorite(ch.id.clone()));
            }
            let muted = d.muted(&ch.id);
            let bell = if muted { Icon::BellOff.image(t.text_dim, 15.0) } else { Icon::Bell.image(t.text_dim, 15.0) };
            let r = ui.add(egui::Button::image(bell).frame(false)).on_hover_text(if muted { "Muted: unmute" } else { "Mute this channel" });
            if r.clicked() {
                actions.push(Action::ToggleMute(ch.id.clone()));
            }
            r.context_menu(|ui| super::sidebar::channel_menu(ui, d, actions, &ch));
            // Header text, one line; click for all of it.
            let header = markup::plain(&ch.header);
            let avail = ui.available_width() - 8.0;
            if avail > 90.0 {
                let (text, color) = if header.is_empty() {
                    (if ch.is_direct() { String::new() } else { "Add a channel header".to_string() }, t.text_dim.gamma_multiply(0.8))
                } else {
                    (header, t.text_dim)
                };
                if !text.is_empty() {
                    ui.add(egui::Separator::default().vertical().spacing(10.0));
                    let g = elided(ui, &text, egui::FontId::proportional(13.0), color, avail - 20.0);
                    let resp = ui.add(egui::Label::new(g).sense(Sense::click()));
                    u.header_anchor = Some(egui::pos2(resp.rect.left() - 8.0, resp.rect.bottom() + 16.0));
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        if ch.header.is_empty() {
                            u.header_editor = Some((ch.id.clone(), String::new()));
                        } else {
                            u.header_popup = !u.header_popup;
                        }
                    }
                }
            }
        });
    });
}

fn chip(ui: &mut Ui, t: &crate::theme::Theme, icon: Icon, count: i64, tip: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 3.0;
        ui.add(icon.image(t.text_dim, 14.0));
        ui.label(RichText::new(count.to_string()).size(13.0).color(t.text_dim));
    })
    .response
    .on_hover_text(tip);
}

fn chip_button(ui: &mut Ui, t: &crate::theme::Theme, icon: Icon, count: i64, tip: &str, active: bool) -> bool {
    let color = if active { t.accent } else { t.text_dim };
    let text = RichText::new(count.to_string()).size(13.0).color(color);
    let btn = egui::Button::image_and_text(icon.image(color, 14.0), text).frame(active).fill(t.reaction_mine);
    ui.add(btn).on_hover_text(tip).clicked()
}
