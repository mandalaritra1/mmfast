//! Team strip, and the channel sidebar: Threads, channels, direct messages.

use super::{Icon, badge, elided, medium, section_label, semibold};
use crate::app::{Action, App, Center, Switcher};
use crate::model::Channel;
use egui::{Align2, Color32, Margin, Rect, RichText, Sense, Ui, pos2, vec2};

pub fn teams(app: &mut App, ui: &mut Ui) {
    let App { d, u, actions, .. } = app;
    let t = &d.theme;
    ui.spacing_mut().item_spacing.y = 10.0;
    for team in &d.teams {
        let active = team.id == d.team;
        let (rect, resp) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
        let fill = if active { t.sidebar_active } else { t.sidebar_hover };
        ui.painter().rect_filled(rect, 9.0, fill);
        if active {
            ui.painter().rect_filled(Rect::from_min_size(pos2(rect.left() - 10.0, rect.top() + 8.0), vec2(4.0, 24.0)), 2.0, Color32::WHITE);
        }
        let icon = (team.last_team_icon_update > 0)
            .then(|| u.images.get(ui.ctx(), &crate::model::ImageKey::TeamIcon { team_id: team.id.clone(), updated: team.last_team_icon_update }))
            .flatten();
        if let Some(tex) = &icon {
            egui::Image::new(tex).corner_radius(9).paint_at(ui, rect);
            if active {
                ui.painter().rect_stroke(rect.expand(2.0), 10.0, egui::Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Outside);
            }
        }
        let initials: String = team
            .display_name
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2)
            .collect::<String>()
            .to_uppercase();
        if icon.is_none() {
            ui.painter().text(rect.center(), Align2::CENTER_CENTER, initials, semibold(15.0), Color32::WHITE);
        }
        let unread = d.channels.values().filter(|c| c.team_id == team.id).any(|c| d.unread(c).0);
        let mentions: i64 = d.channels.values().filter(|c| c.team_id == team.id).map(|c| d.unread(c).1).sum();
        if mentions > 0 && !active {
            badge(ui, rect.right_top() + vec2(6.0, 4.0), mentions, t.badge);
        } else if unread && !active {
            ui.painter().circle_filled(rect.right_top() + vec2(-2.0, 2.0), 4.5, Color32::WHITE);
        }
        if resp.on_hover_text(&team.display_name).clicked() && !active {
            actions.push(Action::SelectTeam(team.id.clone()));
        }
    }
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let App { d, u, actions, .. } = app;
    let t = d.theme.clone();
    let team_name = d.teams.iter().find(|x| x.id == d.team).map(|x| x.display_name.clone()).unwrap_or_default();

    egui::Frame::new().inner_margin(Margin { left: 16, right: 8, top: 12, bottom: 8 }).show(ui, |ui| {
        ui.horizontal(|ui| {
            let g = elided(ui, &team_name, semibold(16.0), t.sidebar_text, ui.available_width() - 40.0);
            ui.label(g);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = super::icon_button(ui, Icon::Settings, t.sidebar_dim, 16.0, "Settings");
                if r.clicked() {
                    u.settings_open = !u.settings_open;
                }
            });
        });
        let me = d.me.display_name();
        ui.horizontal(|ui| {
            let color = if d.connected { t.online } else { t.sidebar_dim };
            let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 4.0, color);
            let status = if d.connected {
                me
            } else if d.synced || d.have_data {
                "Reconnecting…".to_string()
            } else {
                "Connecting…".to_string()
            };
            let resp = ui.label(RichText::new(status).size(12.5).color(t.sidebar_dim));
            if !d.connected && !d.conn_detail.is_empty() {
                resp.on_hover_text(&d.conn_detail);
            }
        });
        ui.add_space(8.0);
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width() - 8.0, 30.0), Sense::click());
        let fill = if resp.hovered() { t.sidebar_hover.gamma_multiply(1.4) } else { t.sidebar_hover };
        ui.painter().rect_filled(rect, 6.0, fill);
        Icon::Search.image(t.sidebar_dim, 14.0).paint_at(ui, Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), vec2(14.0, 14.0)));
        ui.painter().text(pos2(rect.left() + 30.0, rect.center().y), Align2::LEFT_CENTER, "Find channel", egui::FontId::proportional(13.0), t.sidebar_dim);
        let hint = if cfg!(target_os = "macos") { "⌘K" } else { "Ctrl+K" };
        ui.painter().text(pos2(rect.right() - 10.0, rect.center().y), Align2::RIGHT_CENTER, hint, egui::FontId::proportional(11.5), t.sidebar_dim);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            u.switcher = Some(Switcher::default());
        }
    });

    egui::ScrollArea::vertical().id_salt("sidebar-scroll").auto_shrink([false, false]).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        // Threads
        let unread_threads = d.threads.as_ref().map(|x| x.total_unread_threads).unwrap_or(0);
        let mentions_threads = d.threads.as_ref().map(|x| x.total_unread_mentions).unwrap_or(0);
        if nav_row(ui, &t, Icon::Threads, "Threads", d.center == Center::Threads, unread_threads > 0, mentions_threads) {
            actions.push(Action::ShowThreads);
        }

        // Unread channels first, as Mattermost's "group unreads separately".
        let mut unread: Vec<&Channel> =
            d.channels.values().filter(|c| !c.is_direct() && c.team_id == d.team && d.unread(c).0).collect();
        unread.sort_by_key(|c| d.channel_name(c).to_lowercase());
        let unread_ids: std::collections::HashSet<&str> = unread.iter().map(|c| c.id.as_str()).collect();
        if !unread.is_empty() {
            section_label(ui, "UNREADS", t.sidebar_dim);
            for c in &unread {
                channel_row(ui, d, u, actions, c);
            }
        }
        match d.categories.get(&d.team) {
            Some(cats) => {
                let placed: std::collections::HashSet<&str> =
                    cats.iter().flat_map(|c| c.channel_ids.iter().map(String::as_str)).collect();
                for cat in cats {
                    let mut list: Vec<&Channel> = cat.channel_ids.iter().filter_map(|id| d.channels.get(id)).collect();
                    // Channels the server has not filed yet (just joined, a new DM).
                    let strays = d.channels.values().filter(|c| !placed.contains(c.id.as_str()));
                    match cat.kind.as_str() {
                        "channels" => list.extend(strays.filter(|c| !c.is_direct() && c.team_id == d.team)),
                        "direct_messages" => list.extend(strays.filter(|c| c.is_direct())),
                        _ => {}
                    }
                    list.retain(|c| !unread_ids.contains(c.id.as_str()) && (c.is_direct() || c.team_id == d.team));
                    match cat.sorting.as_str() {
                        "alphabetical" => list.sort_by_key(|c| d.channel_name(c).to_lowercase()),
                        "recent" => list.sort_by_key(|c| std::cmp::Reverse(c.last_post_at)),
                        _ if cat.kind == "direct_messages" => list.sort_by_key(|c| std::cmp::Reverse(c.last_post_at)),
                        _ => {}
                    }
                    if cat.kind == "direct_messages" {
                        list.truncate(40);
                    }
                    if list.is_empty() && cat.kind == "favorites" {
                        continue;
                    }
                    let collapsed = u.collapsed.contains(&cat.id);
                    let plus = match cat.kind.as_str() {
                        "channels" => Some(("Browse channels", Action::OpenBrowse)),
                        "direct_messages" => Some(("Message someone", Action::OpenBrowse)),
                        _ => None,
                    };
                    let (toggle, plus_clicked) = category_header(ui, &t, &cat.display_name, collapsed, plus.as_ref().map(|p| p.0));
                    if toggle {
                        if collapsed {
                            u.collapsed.remove(&cat.id);
                        } else {
                            u.collapsed.insert(cat.id.clone());
                        }
                    }
                    if plus_clicked {
                        if cat.kind == "direct_messages" {
                            u.switcher = Some(crate::app::Switcher::default());
                        } else {
                            actions.push(Action::OpenBrowse);
                        }
                    }
                    for c in list {
                        if collapsed && d.current != c.id && d.unread(c).1 == 0 {
                            continue;
                        }
                        channel_row(ui, d, u, actions, c);
                    }
                }
            }
            None => {
                let mut channels: Vec<&Channel> = d
                    .channels
                    .values()
                    .filter(|c| !c.is_direct() && c.team_id == d.team && !unread_ids.contains(c.id.as_str()))
                    .collect();
                channels.sort_by_key(|c| d.channel_name(c).to_lowercase());
                let mut directs: Vec<&Channel> = d.channels.values().filter(|c| c.is_direct()).collect();
                directs.sort_by_key(|c| std::cmp::Reverse(c.last_post_at));
                section_label(ui, "CHANNELS", t.sidebar_dim);
                for c in channels {
                    channel_row(ui, d, u, actions, c);
                }
                section_label(ui, "DIRECT MESSAGES", t.sidebar_dim);
                for c in directs.into_iter().take(40) {
                    channel_row(ui, d, u, actions, c);
                }
            }
        }
        ui.add_space(8.0);
        if nav_row(ui, &t, Icon::Plus, "Browse channels", false, false, 0) {
            actions.push(Action::OpenBrowse);
        }
        ui.add_space(16.0);
    });
}

/// A category title: click folds it; the optional plus does `plus`.
fn category_header(ui: &mut Ui, t: &crate::theme::Theme, name: &str, collapsed: bool, plus: Option<&str>) -> (bool, bool) {
    ui.add_space(10.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::click());
    let color = if resp.hovered() { t.sidebar_text } else { t.sidebar_dim };
    let icon = if collapsed { Icon::ChevronRight } else { Icon::ChevronDown };
    icon.image(color, 12.0).paint_at(ui, Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(12.0, 12.0)));
    ui.painter().text(pos2(rect.left() + 28.0, rect.center().y), Align2::LEFT_CENTER, name.to_uppercase(), semibold(11.5), color);
    let mut plus_clicked = false;
    if let Some(tip) = plus {
        let r = Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), vec2(20.0, 20.0));
        let p = ui.interact(r, ui.id().with(("cat-plus", name)), Sense::click());
        if p.hovered() || resp.hovered() {
            Icon::Plus.image(if p.hovered() { t.sidebar_text } else { t.sidebar_dim }, 14.0).paint_at(ui, r.shrink(3.0));
        }
        plus_clicked = p.on_hover_text(tip).clicked();
    }
    (resp.clicked() && !plus_clicked, plus_clicked)
}

fn nav_row(ui: &mut Ui, t: &crate::theme::Theme, icon: Icon, label: &str, active: bool, unread: bool, mentions: i64) -> bool {
    ui.add_space(4.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    let rect = rect.shrink2(vec2(8.0, 0.0));
    if active {
        ui.painter().rect_filled(rect, 6.0, t.sidebar_active);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, t.sidebar_hover);
    }
    let color = if active || unread { t.sidebar_text } else { t.sidebar_dim };
    icon.image(color, 16.0).paint_at(ui, Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(16.0, 16.0)));
    let font = if unread { semibold(14.0) } else { medium(14.0) };
    ui.painter().text(pos2(rect.left() + 36.0, rect.center().y), Align2::LEFT_CENTER, label, font, color);
    if mentions > 0 {
        badge(ui, pos2(rect.right() - 8.0, rect.center().y), mentions, t.badge);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

fn channel_row(ui: &mut Ui, d: &crate::app::Data, u: &mut crate::app::UiState, actions: &mut Vec<Action>, c: &Channel) {
    let t = &d.theme;
    let active = d.center == Center::Channel && d.current == c.id;
    let (unread, mentions) = d.unread(c);
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    let rect = rect.shrink2(vec2(8.0, 0.0));
    if active {
        ui.painter().rect_filled(rect, 6.0, t.sidebar_active);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, t.sidebar_hover);
    }
    let muted = d.muted(&c.id);
    let color = if active || unread {
        t.sidebar_text
    } else if muted {
        t.sidebar_dim.gamma_multiply(0.55)
    } else {
        t.sidebar_dim
    };
    let icon_rect = Rect::from_center_size(pos2(rect.left() + 18.0, rect.center().y), vec2(16.0, 16.0));
    match c.kind.as_str() {
        "O" => Icon::Hash.image(color, 15.0).paint_at(ui, icon_rect),
        "P" => Icon::Lock.image(color, 14.0).paint_at(ui, icon_rect),
        "D" => {
            let partner = c.dm_partner(&d.me.id).unwrap_or_default();
            let r = Rect::from_center_size(icon_rect.center(), vec2(20.0, 20.0));
            super::paint_avatar(ui, &mut u.images, d, &partner, r);
        }
        _ => Icon::Users.image(color, 15.0).paint_at(ui, icon_rect),
    }
    let name = d.channel_name(c);
    let font = if unread { semibold(14.0) } else { egui::FontId::proportional(14.0) };
    let max = rect.width() - 46.0 - if mentions > 0 { 30.0 } else { 0.0 };
    let g = elided(ui, &name, font, color, max);
    ui.painter().galley(pos2(rect.left() + 36.0, rect.center().y - g.size().y / 2.0), g, color);
    if mentions > 0 {
        badge(ui, pos2(rect.right() - 8.0, rect.center().y), mentions, if muted { t.sidebar_dim } else { t.badge });
    } else if muted {
        Icon::BellOff.image(t.sidebar_dim.gamma_multiply(0.55), 12.0).paint_at(
            ui,
            Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(12.0, 12.0)),
        );
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.clicked() {
        actions.push(Action::OpenChannel(c.id.clone()));
    }
    resp.context_menu(|ui| channel_menu(ui, d, actions, c));
}

/// Favorite, mute, leave: the channel's right-click menu (also in the header).
pub fn channel_menu(ui: &mut Ui, d: &crate::app::Data, actions: &mut Vec<Action>, c: &Channel) {
    let fav = d.favorite(&c.id);
    let muted = d.muted(&c.id);
    let (fi, fl) = if fav { (Icon::StarFilled, "Remove from favorites") } else { (Icon::Star, "Add to favorites") };
    if ui.add(egui::Button::image_and_text(fi.image(d.theme.text, 14.0), fl).frame(false)).clicked() {
        actions.push(Action::ToggleFavorite(c.id.clone()));
        ui.close();
    }
    let (mi, ml) = if muted { (Icon::Bell, "Unmute") } else { (Icon::BellOff, "Mute") };
    if ui.add(egui::Button::image_and_text(mi.image(d.theme.text, 14.0), ml).frame(false)).clicked() {
        actions.push(Action::ToggleMute(c.id.clone()));
        ui.close();
    }
    if !c.is_direct() && c.name != "town-square" {
        ui.separator();
        let leave = egui::Button::image_and_text(Icon::Logout.image(d.theme.error, 14.0), egui::RichText::new("Leave channel").color(d.theme.error)).frame(false);
        if ui.add(leave).clicked() {
            actions.push(Action::Leave(c.id.clone()));
            ui.close();
        }
    }
}
