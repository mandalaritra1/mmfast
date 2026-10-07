//! Everything drawn above the main layout: the image lightbox, the channel
//! switcher, the post menu and emoji picker, dialogs, settings, the drop
//! target, toasts, and global shortcuts.

use super::{Icon, icon_button, semibold};
use crate::app::{Action, App, Center, Picker, Rhs, Switcher};
use crate::backend::UploadData;
use crate::config::ThemePref;
use crate::markup::{self, RenderCtx};
use crate::model::{ImageKey, human_size};
use egui::{Align2, Color32, Frame, Id, Key, Margin, Modifiers, Order, Rect, RichText, Sense, Stroke, vec2};

pub fn shortcuts(app: &mut App, ctx: &egui::Context) {
    let cmd = Modifiers::COMMAND;
    if ctx.input_mut(|i| i.consume_key(cmd, Key::K)) {
        app.u.switcher = if app.u.switcher.is_some() { None } else { Some(Switcher::default()) };
    }
    if ctx.input_mut(|i| i.consume_key(cmd | Modifiers::SHIFT, Key::F) || i.consume_key(cmd, Key::F)) {
        app.u.focus_search = true;
        if app.d.center != Center::Channel
            && let Some(id) = Some(app.d.current.clone()).filter(|c| !c.is_empty())
        {
            app.actions.push(Action::OpenChannel(id));
        }
    }
    // An open editor (a message being edited, a dialog) gets Escape first.
    let editing = app.u.editing.is_some() || app.u.header_editor.is_some() || app.u.confirm_delete.is_some();
    if !editing && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        if app.u.lightbox.is_some() {
            app.u.lightbox = None;
        } else if app.u.switcher.is_some() {
            app.u.switcher = None;
        } else if app.u.menu.is_some() || app.u.picker.is_some() || app.u.profile.is_some() {
            app.u.menu = None;
            app.u.picker = None;
            app.u.profile = None;
        } else if app.u.header_popup || app.u.settings_open {
            app.u.header_popup = false;
            app.u.settings_open = false;
        } else if app.d.rhs != Rhs::None {
            app.d.rhs = Rhs::None;
        }
    }
    // Alt+Up/Down: previous / next channel in the sidebar.
    let step = ctx.input_mut(|i| {
        if i.consume_key(Modifiers::ALT, Key::ArrowUp) {
            -1
        } else if i.consume_key(Modifiers::ALT, Key::ArrowDown) {
            1
        } else {
            0
        }
    });
    if step != 0 {
        let order = sidebar_order(app);
        if let Some(pos) = order.iter().position(|c| *c == app.d.current) {
            let next = (pos as i64 + step).rem_euclid(order.len() as i64) as usize;
            app.actions.push(Action::OpenChannel(order[next].clone()));
        }
    }
    // Files dropped on the window go to the open channel (or thread).
    let dropped = ctx.input(|i| i.raw.dropped_files.clone());
    if !dropped.is_empty() && app.d.logged_in {
        let key = match &app.d.rhs {
            Rhs::Thread(root) if ctx.pointer_hover_pos().is_some_and(|p| p.x > ctx.content_rect().right() - 430.0) => format!("t:{root}"),
            _ => format!("c:{}", app.d.current),
        };
        for f in dropped {
            let path = f.path().to_path_buf();
            if path.as_os_str().is_empty() {
                continue;
            }
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
            app.actions.push(Action::Upload { key: key.clone(), name, data: UploadData::Path(path) });
        }
    }
}

fn sidebar_order(app: &App) -> Vec<String> {
    let d = &app.d;
    let mut channels: Vec<_> = d.channels.values().filter(|c| !c.is_direct() && c.team_id == d.team).collect();
    channels.sort_by_key(|c| (!d.unread(c).0, d.channel_name(c).to_lowercase()));
    let mut directs: Vec<_> = d.channels.values().filter(|c| c.is_direct()).collect();
    directs.sort_by_key(|c| std::cmp::Reverse(c.last_post_at));
    channels.into_iter().chain(directs).map(|c| c.id.clone()).collect()
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    drop_target(app, ctx);
    header_popup(app, ctx);
    header_editor(app, ctx);
    settings(app, ctx);
    post_menu(app, ctx);
    profile_card(app, ctx);
    emoji_picker(app, ctx);
    confirm_delete(app, ctx);
    switcher(app, ctx);
    browse(app, ctx);
    lightbox(app, ctx);
    toasts(app, ctx);
}

fn drop_target(app: &App, ctx: &egui::Context) {
    if ctx.input(|i| i.raw.hovered_files.is_empty()) {
        return;
    }
    let t = &app.d.theme;
    let name = app.d.current_channel().map(|c| app.d.channel_name(c)).unwrap_or_default();
    let painter = ctx.layer_painter(egui::LayerId::new(Order::Foreground, Id::new("drop")));
    let rect = ctx.content_rect();
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha(120));
    painter.rect_stroke(rect.shrink(16.0), 12.0, Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
    painter.text(rect.center(), Align2::CENTER_CENTER, format!("Drop to upload to {name}"), semibold(22.0), Color32::WHITE);
}

fn header_popup(app: &mut App, ctx: &egui::Context) {
    if !app.u.header_popup {
        return;
    }
    let Some(ch) = app.d.current_channel().cloned() else { return };
    let t = app.d.theme.clone();
    let mut open = true;
    let mut edit = false;
    let mut link = None;
    let resp = egui::Area::new(Id::new("header-popup"))
        .order(Order::Foreground)
        .fixed_pos(app.u.header_anchor.unwrap_or(egui::pos2(ctx.content_rect().left() + 320.0, 58.0)))
        .show(ctx, |ui| {
            Frame::popup(ui.style()).fill(t.bg).inner_margin(Margin::same(16)).show(ui, |ui| {
                ui.set_max_width(520.0);
                ui.label(RichText::new("Header").font(semibold(12.0)).color(t.text_dim));
                let doc = markup::parse(&ch.header);
                let rc = RenderCtx { theme: &t, size: 14.0, me: &app.d.me.username, color: t.text, highlight: &[], emoji: &app.d.custom_emoji, paint: None };
                link = markup::show(ui, &doc, &rc);
                if !ch.purpose.is_empty() {
                    ui.add_space(8.0);
                    ui.label(RichText::new("Purpose").font(semibold(12.0)).color(t.text_dim));
                    ui.label(&ch.purpose);
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if !ch.is_direct() && ui.button("Edit header").clicked() {
                        edit = true;
                    }
                    if ui.button("Close").clicked() {
                        open = false;
                    }
                });
            });
        });
    if let Some(l) = link {
        app.actions.push(Action::Link(l));
    }
    if edit {
        app.u.header_editor = Some((ch.id.clone(), ch.header.clone()));
        open = false;
    }
    if ctx.input(|i| i.pointer.any_pressed()) && !resp.response.hovered() && !resp.response.contains_pointer() {
        let over_header = ctx.pointer_interact_pos().is_some_and(|p| p.y < 54.0);
        if !over_header {
            open = false;
        }
    }
    app.u.header_popup = open;
}

fn header_editor(app: &mut App, ctx: &egui::Context) {
    let Some((channel_id, text)) = app.u.header_editor.as_mut() else { return };
    let t = app.d.theme.clone();
    let mut close = false;
    let mut save = false;
    let modal = egui::Modal::new(Id::new("header-editor")).frame(Frame::popup(&ctx.global_style()).fill(t.bg).inner_margin(Margin::same(20))).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.label(RichText::new("Edit channel header").font(semibold(17.0)));
        ui.label(RichText::new("Markdown is supported. Shown at the top of the channel.").size(12.5).color(t.text_dim));
        ui.add_space(8.0);
        ui.add(egui::TextEdit::multiline(text).desired_rows(4).desired_width(f32::INFINITY));
        ui.label(RichText::new(format!("{} / 1024", text.chars().count())).size(11.5).color(t.text_dim));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                close = true;
            }
            if ui.add(egui::Button::new(RichText::new("Save").color(t.accent_text)).fill(t.accent)).clicked() {
                save = true;
            }
        });
    });
    if modal.should_close() {
        close = true;
    }
    if save {
        let header: String = text.chars().take(1024).collect();
        app.actions.push(Action::SaveHeader { channel_id: channel_id.clone(), header });
        close = true;
    }
    if close {
        app.u.header_editor = None;
    }
}

fn settings(app: &mut App, ctx: &egui::Context) {
    if !app.u.settings_open {
        return;
    }
    let t = app.d.theme.clone();
    let left = if app.d.teams.len() > 1 { 70.0 } else { 10.0 };
    let resp = egui::Area::new(Id::new("settings"))
        .order(Order::Foreground)
        .fixed_pos(egui::pos2(left + 150.0, 40.0))
        .show(ctx, |ui| {
            Frame::popup(ui.style()).fill(t.bg).inner_margin(Margin::same(12)).show(ui, |ui| {
                ui.set_width(250.0);
                ui.label(RichText::new(app.d.me.display_name()).font(semibold(14.0)));
                ui.label(RichText::new(format!("@{}", app.d.me.username)).size(12.5).color(t.text_dim));
                if let Some((server, _)) = &app.session {
                    ui.label(RichText::new(server).size(12.0).color(t.text_dim));
                }
                ui.separator();
                ui.label(RichText::new("Theme").font(semibold(12.0)).color(t.text_dim));
                ui.horizontal(|ui| {
                    for (pref, icon, label) in [(ThemePref::System, Icon::Monitor, "System"), (ThemePref::Light, Icon::Sun, "Light"), (ThemePref::Dark, Icon::Moon, "Dark")] {
                        let selected = app.config.theme == pref;
                        let btn = egui::Button::image_and_text(icon.image(t.text, 14.0), label).selected(selected);
                        if ui.add(btn).clicked() {
                            app.actions.push(Action::SetTheme(pref));
                        }
                    }
                });
                ui.separator();
                if ui.add(egui::Button::image_and_text(Icon::Logout.image(t.error, 14.0), RichText::new("Sign out").color(t.error)).frame(false)).clicked() {
                    app.actions.push(Action::Logout);
                    app.u.settings_open = false;
                }
            });
        });
    if ctx.input(|i| i.pointer.any_pressed()) && !resp.response.contains_pointer() {
        let p = ctx.pointer_interact_pos();
        if !p.is_some_and(|p| p.y < 50.0 && p.x < left + 260.0) {
            app.u.settings_open = false;
        }
    }
}

fn post_menu(app: &mut App, ctx: &egui::Context) {
    let Some(menu) = &app.u.menu else { return };
    let Some(post) = app.d.find_post(&menu.post_id).cloned() else {
        app.u.menu = None;
        return;
    };
    let t = app.d.theme.clone();
    let mine = post.user_id == app.d.me.id;
    let admin = app.d.me.is_admin();
    let mut close = false;
    let resp = egui::Area::new(Id::new("post-menu"))
        .order(Order::Foreground)
        .fixed_pos(menu.pos - vec2(140.0, -4.0))
        .constrain(true)
        .show(ctx, |ui| {
            Frame::popup(ui.style()).fill(t.bg).inner_margin(Margin::same(6)).show(ui, |ui| {
                ui.set_width(180.0);
                let item = |ui: &mut egui::Ui, icon: Icon, label: &str, color: Color32| {
                    ui.add_sized(
                        vec2(ui.available_width(), 28.0),
                        egui::Button::image_and_text(icon.image(color, 14.0), RichText::new(label).color(color)).frame(false),
                    )
                    .clicked()
                };
                if item(ui, Icon::Reply, "Reply", t.text) {
                    let root = if post.root_id.is_empty() { post.id.clone() } else { post.root_id.clone() };
                    app.actions.push(Action::OpenThread(root));
                    close = true;
                }
                if item(ui, Icon::Copy, "Copy text", t.text) {
                    ui.ctx().copy_text(post.message.clone());
                    close = true;
                }
                if let Some((server, _)) = &app.session {
                    let team = app.d.teams.iter().find(|x| x.id == app.d.team).map(|x| x.name.clone()).unwrap_or_default();
                    if item(ui, Icon::External, "Copy link", t.text) {
                        ui.ctx().copy_text(format!("{server}/{team}/pl/{}", post.id));
                        close = true;
                    }
                }
                let pin_label = if post.is_pinned { "Unpin" } else { "Pin to channel" };
                if item(ui, Icon::Pin, pin_label, t.text) {
                    app.actions.push(Action::Pin { post_id: post.id.clone(), pinned: !post.is_pinned });
                    close = true;
                }
                if mine && item(ui, Icon::Edit, "Edit", t.text) {
                    app.u.editing = Some((post.id.clone(), post.message.clone()));
                    close = true;
                }
                if (mine || admin) && item(ui, Icon::Trash, "Delete", t.error) {
                    app.u.confirm_delete = Some(post.id.clone());
                    close = true;
                }
            });
        });
    if ctx.input(|i| i.pointer.any_pressed()) && !resp.response.contains_pointer() {
        close = true;
    }
    if close {
        app.u.menu = None;
    }
}

fn profile_card(app: &mut App, ctx: &egui::Context) {
    let Some((user_id, pos)) = app.u.profile.clone() else { return };
    let t = app.d.theme.clone();
    let user = app.d.users.get(&user_id).cloned().unwrap_or_default();
    let mut close = false;
    let resp = egui::Area::new(Id::new("profile-card"))
        .order(Order::Foreground)
        .fixed_pos(pos + vec2(6.0, 0.0))
        .constrain(true)
        .show(ctx, |ui| {
            Frame::popup(ui.style()).fill(t.bg).inner_margin(Margin::same(16)).show(ui, |ui| {
                ui.set_width(260.0);
                ui.horizontal(|ui| {
                    super::avatar(ui, &mut app.u.images, &app.d, &user_id, 64.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new(user.display_name()).font(semibold(16.0)));
                        ui.label(RichText::new(format!("@{}", user.username)).color(t.text_dim));
                        if !user.nickname.is_empty() {
                            ui.label(RichText::new(&user.nickname).size(12.5).color(t.text_dim));
                        }
                        if !user.position.is_empty() {
                            ui.label(RichText::new(&user.position).size(12.5));
                        }
                    });
                });
                if !user.email.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(&user.email).size(12.5).color(t.text_dim));
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if user_id != app.d.me.id
                        && ui.add(egui::Button::new(RichText::new("Message").color(t.accent_text)).fill(t.accent)).clicked()
                    {
                        app.actions.push(Action::OpenDm(user_id.clone()));
                        close = true;
                    }
                    if ui.button("Copy @username").clicked() {
                        ui.ctx().copy_text(format!("@{}", user.username));
                        close = true;
                    }
                });
            });
        });
    // The click that opened it registers on release, after its press, so a
    // press outside the card is always a new one.
    if close || (ctx.input(|i| i.pointer.any_pressed()) && !resp.response.contains_pointer()) {
        app.u.profile = None;
    }
}

const QUICK: [&str; 8] = ["+1", "white_check_mark", "heart", "joy", "tada", "eyes", "pray", "rocket"];

fn emoji_picker(app: &mut App, ctx: &egui::Context) {
    let Some(Picker { post_id, pos, mut query }) = app.u.picker.take() else { return };
    let query = &mut query;
    let t = app.d.theme.clone();
    let images = &mut app.u.images;
    let custom_emoji = &app.d.custom_emoji;
    let mut chosen: Option<String> = None;
    let resp = egui::Area::new(Id::new("emoji-picker"))
        .order(Order::Foreground)
        .fixed_pos(pos - vec2(260.0, -4.0))
        .constrain(true)
        .show(ctx, |ui| {
            Frame::popup(ui.style()).fill(t.bg).inner_margin(Margin::same(10)).show(ui, |ui| {
                ui.set_width(300.0);
                let r = ui.add(egui::TextEdit::singleline(query).hint_text("Search emoji").desired_width(f32::INFINITY));
                if !r.has_focus() && query.is_empty() {
                    r.request_focus();
                }
                ui.add_space(6.0);
                let q = query.to_lowercase();
                let list: Vec<(String, String)> = if q.is_empty() {
                    QUICK.iter().filter_map(|n| markup::emoji_for(n).map(|e| (n.to_string(), e.to_string()))).collect()
                } else {
                    emojis::iter()
                        .filter_map(|e| e.shortcodes().find(|c| c.contains(&q)).map(|c| (c.to_string(), e.as_str().to_string())))
                        .take(48)
                        .collect()
                };
                let mut custom: Vec<(&String, &String)> = if q.is_empty() {
                    Vec::new()
                } else {
                    custom_emoji.iter().filter(|(name, _)| name.contains(&q)).collect()
                };
                custom.sort();
                custom.truncate(40);
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                        for (code, glyph) in &list {
                            let b = egui::Button::new(RichText::new(glyph).size(22.0)).frame(false).min_size(vec2(34.0, 34.0));
                            if ui.add(b).on_hover_text(format!(":{code}:")).clicked() {
                                chosen = Some(code.clone());
                            }
                        }
                        for (name, id) in &custom {
                            let clicked = match images.get(ui.ctx(), &ImageKey::Emoji((*id).clone())) {
                                Some(tex) => ui
                                    .add(egui::Button::image(egui::Image::new(&tex).fit_to_exact_size(vec2(24.0, 24.0))).frame(false).min_size(vec2(34.0, 34.0)))
                                    .on_hover_text(format!(":{name}:"))
                                    .clicked(),
                                None => ui.small_button(format!(":{name}:")).clicked(),
                            };
                            if clicked {
                                chosen = Some((*name).clone());
                            }
                        }
                    });
                });
                if ui.input(|i| i.key_pressed(Key::Enter))
                    && let Some((code, _)) = list.first()
                {
                    chosen = Some(code.clone());
                }
            });
        });
    let picker = Picker { post_id: post_id.clone(), pos, query: query.clone() };
    if let Some(code) = chosen {
        let mine = app.d.find_post(&post_id).is_some_and(|p| p.metadata.reactions.iter().any(|r| r.user_id == app.d.me.id && r.emoji_name == code));
        app.actions.push(Action::React { post_id, emoji: code, add: !mine });
    } else if !(ctx.input(|i| i.pointer.any_pressed()) && !resp.response.contains_pointer()) {
        app.u.picker = Some(picker);
    }
}

fn confirm_delete(app: &mut App, ctx: &egui::Context) {
    let Some(post_id) = app.u.confirm_delete.clone() else { return };
    let t = app.d.theme.clone();
    let mut close = false;
    let modal = egui::Modal::new(Id::new("confirm-delete")).frame(Frame::popup(&ctx.global_style()).fill(t.bg).inner_margin(Margin::same(20))).show(ctx, |ui| {
        ui.set_width(360.0);
        ui.label(RichText::new("Delete message?").font(semibold(17.0)));
        ui.label(RichText::new("This removes it for everyone in the channel.").color(t.text_dim));
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            if ui.button("Cancel").clicked() {
                close = true;
            }
            if ui.add(egui::Button::new(RichText::new("Delete").color(Color32::WHITE)).fill(t.error)).clicked() {
                app.actions.push(Action::Delete(post_id.clone()));
                close = true;
            }
        });
    });
    if modal.should_close() || close {
        app.u.confirm_delete = None;
    }
}

fn fuzzy(hay: &str, needle: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }
    let hay = hay.to_lowercase();
    if let Some(i) = hay.find(needle) {
        return Some(100 - i as i32);
    }
    let mut it = hay.chars();
    for c in needle.chars() {
        it.find(|h| *h == c)?;
    }
    Some(0)
}

fn switcher(app: &mut App, ctx: &egui::Context) {
    let Some(sw) = &mut app.u.switcher else { return };
    let d = &app.d;
    let t = d.theme.clone();
    let q = sw.query.to_lowercase();
    let mut items: Vec<(i32, String, String, bool)> = d
        .channels
        .values()
        .filter(|c| c.is_direct() || c.team_id == d.team || d.teams.len() > 1)
        .filter_map(|c| {
            let name = d.channel_name(c);
            let score = fuzzy(&name, &q).or_else(|| if c.is_direct() { None } else { fuzzy(&c.name, &q) })?;
            let unread = d.unread(c).0;
            let team = if c.is_direct() || c.team_id == d.team {
                String::new()
            } else {
                d.teams.iter().find(|x| x.id == c.team_id).map(|x| x.display_name.clone()).unwrap_or_default()
            };
            let label = match c.kind.as_str() {
                "O" => format!("# {name}"),
                "P" => format!("🔒 {name}"),
                _ => format!("@ {name}"),
            };
            let label = if team.is_empty() { label } else { format!("{label}   ({team})") };
            Some((score * 2 + unread as i32 + (c.last_post_at / 1_000_000_000) as i32 / 1000, c.id.clone(), label, unread))
        })
        .collect();
    // People without a conversation yet: picking one starts a DM.
    if q.chars().count() >= 2 {
        if sw.asked != q {
            sw.asked = q.clone();
            app.actions.push(Action::Autocomplete(q.clone()));
        }
        let me = d.me.id.clone();
        let with_dm: std::collections::HashSet<String> = d.channels.values().filter_map(|c| c.dm_partner(&me)).collect();
        let mut seen = std::collections::HashSet::new();
        for u in d.users.values().chain(d.ac_users.iter()) {
            if u.id == me || u.delete_at > 0 || with_dm.contains(&u.id) || !seen.insert(u.id.clone()) {
                continue;
            }
            let name = u.display_name();
            if let Some(score) = fuzzy(&name, &q).or_else(|| fuzzy(&u.username, &q)) {
                items.push((score * 2 - 1, format!("user:{}", u.id), format!("@ {name}   (new message)"), false));
            }
        }
    }
    items.sort_by(|a, b| b.0.cmp(&a.0).then(a.2.cmp(&b.2)));
    items.truncate(12);
    if sw.selected >= items.len() {
        sw.selected = items.len().saturating_sub(1);
    }
    let (down, up, enter) = ctx.input_mut(|i| {
        (
            i.consume_key(Modifiers::NONE, Key::ArrowDown),
            i.consume_key(Modifiers::NONE, Key::ArrowUp),
            i.consume_key(Modifiers::NONE, Key::Enter),
        )
    });
    if down && !items.is_empty() {
        sw.selected = (sw.selected + 1) % items.len();
    }
    if up && !items.is_empty() {
        sw.selected = (sw.selected + items.len() - 1) % items.len();
    }
    let mut open: Option<String> = None;
    if enter {
        open = items.get(sw.selected).map(|x| x.1.clone());
    }
    let screen = ctx.content_rect();
    let modal = egui::Modal::new(Id::new("switcher"))
        .area(egui::Modal::default_area(Id::new("switcher-area")).fixed_pos(egui::pos2(screen.center().x - 260.0, screen.top() + 90.0)).pivot(Align2::LEFT_TOP))
        .frame(Frame::popup(&ctx.global_style()).fill(t.bg).inner_margin(Margin::same(12)))
        .show(ctx, |ui| {
            ui.set_width(520.0);
            ui.label(RichText::new("Switch channels or message someone").font(semibold(15.0)));
            ui.add_space(6.0);
            let r = ui.add(egui::TextEdit::singleline(&mut sw.query).hint_text("Type a channel or person").desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)));
            r.request_focus();
            ui.add_space(6.0);
            for (i, (_, id, label, unread)) in items.iter().enumerate() {
                let (r, b) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                if i == sw.selected || b.hovered() {
                    ui.painter().rect_filled(r, 6.0, if i == sw.selected { t.reaction_mine } else { t.hover_bg });
                }
                let font = if *unread { semibold(14.0) } else { egui::FontId::proportional(14.0) };
                ui.painter().text(egui::pos2(r.left() + 10.0, r.center().y), Align2::LEFT_CENTER, label, font, t.text);
                if b.clicked() {
                    open = Some(id.clone());
                }
            }
            if items.is_empty() {
                ui.label(RichText::new("No matches").color(t.text_dim));
            }
            ui.add_space(4.0);
            ui.label(RichText::new("↑↓ to move · Enter to open · Esc to close").size(11.5).color(t.text_dim));
        });
    if modal.should_close() {
        app.u.switcher = None;
    }
    if let Some(id) = open {
        app.u.switcher = None;
        match id.strip_prefix("user:") {
            Some(user) => app.actions.push(Action::OpenDm(user.to_string())),
            None => app.actions.push(Action::OpenChannel(id)),
        }
    }
}

fn browse(app: &mut App, ctx: &egui::Context) {
    let Some(b) = &mut app.d.browse else { return };
    let t = app.d.theme.clone();
    let team = app.d.teams.iter().find(|x| x.id == app.d.team).map(|x| x.display_name.clone()).unwrap_or_default();
    let mut search = false;
    let mut join = None;
    let mut open = None;
    let joined: std::collections::HashSet<String> = app.d.channels.keys().cloned().collect();
    let modal = egui::Modal::new(Id::new("browse")).frame(Frame::popup(&ctx.global_style()).fill(t.bg).inner_margin(Margin::same(18))).show(ctx, |ui| {
        ui.set_width(560.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Browse channels").font(semibold(17.0)));
            ui.label(RichText::new(&team).color(t.text_dim));
        });
        ui.add_space(6.0);
        let r = ui.add(egui::TextEdit::singleline(&mut b.query).hint_text("Search channels").desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)));
        if !r.has_focus() && b.query.is_empty() {
            r.request_focus();
        }
        if r.changed() {
            search = true;
        }
        ui.add_space(6.0);
        egui::ScrollArea::vertical().max_height(420.0).min_scrolled_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
            if b.loading && b.results.is_empty() {
                super::spinner(ui, t.text_dim);
            } else if b.results.is_empty() {
                ui.label(RichText::new("No channels found.").color(t.text_dim));
            }
            for c in &b.results {
                let member = joined.contains(&c.id);
                ui.horizontal(|ui| {
                    let icon = if c.kind == "P" { Icon::Lock } else { Icon::Hash };
                    ui.add(icon.image(t.text_dim, 14.0));
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 90.0);
                        ui.label(RichText::new(&c.display_name).font(semibold(14.0)));
                        if !c.purpose.is_empty() {
                            let g = super::elided(ui, &c.purpose, egui::FontId::proportional(12.5), t.text_dim, ui.available_width());
                            ui.label(g);
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if member {
                            if ui.button("Open").clicked() {
                                open = Some(c.id.clone());
                            }
                        } else if ui.add(egui::Button::new(RichText::new("Join").color(t.accent_text)).fill(t.accent)).clicked() {
                            join = Some(c.id.clone());
                        }
                    });
                });
                ui.separator();
            }
        });
    });
    if search {
        app.actions.push(Action::BrowseSearch);
    }
    if let Some(id) = join {
        app.actions.push(Action::Join(id));
    }
    if let Some(id) = open {
        app.d.browse = None;
        app.actions.push(Action::OpenChannel(id));
    } else if modal.should_close() {
        app.d.browse = None;
    }
}

fn lightbox(app: &mut App, ctx: &egui::Context) {
    let Some(lb) = &mut app.u.lightbox else { return };
    let t = app.d.theme.clone();
    let n = lb.files.len();
    let (left, right) = ctx.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowLeft), i.consume_key(Modifiers::NONE, Key::ArrowRight)));
    if left && lb.index > 0 {
        lb.index -= 1;
        lb.zoom = 1.0;
        lb.pan = egui::Vec2::ZERO;
    }
    if right && lb.index + 1 < n {
        lb.index += 1;
        lb.zoom = 1.0;
        lb.pan = egui::Vec2::ZERO;
    }
    let file = lb.files[lb.index].clone();
    let screen = ctx.content_rect();
    let mut close = false;
    let mut action = None;
    egui::Area::new(Id::new("lightbox")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
        let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
        ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(245));
        let area = rect.shrink2(vec2(60.0, 70.0));
        // Full resolution if we have it, the preview meanwhile.
        let tex = if file.is_gif() {
            app.u.images.frame(ui.ctx(), &ImageKey::Animated(file.id.clone()), app.u.now)
        } else {
            let full = app.u.images.get(ui.ctx(), &ImageKey::Full(file.id.clone()));
            full.or_else(|| app.u.images.get(ui.ctx(), &ImageKey::Preview(file.id.clone())))
        };
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y + i.zoom_delta().ln() * 200.0);
            if scroll != 0.0 {
                lb.zoom = (lb.zoom * (1.0 + scroll * 0.002)).clamp(0.2, 8.0);
            }
        }
        if resp.dragged() {
            lb.pan += resp.drag_delta();
        }
        if resp.double_clicked() {
            lb.zoom = if lb.zoom > 1.01 { 1.0 } else { 2.0 };
            lb.pan = egui::Vec2::ZERO;
        }
        match tex {
            Some(tex) => {
                let size = vec2(file.width.max(1) as f32, file.height.max(1) as f32);
                let fit = (area.width() / size.x).min(area.height() / size.y).min(2.0) * lb.zoom;
                let img_rect = Rect::from_center_size(area.center() + lb.pan, size * fit);
                egui::Image::new(&tex).paint_at(ui, img_rect);
            }
            None => {
                ui.put(Rect::from_center_size(area.center(), vec2(30.0, 30.0)), egui::Spinner::new().size(28.0).color(Color32::WHITE));
            }
        }
        // Top bar.
        let bar = Rect::from_min_size(rect.min, vec2(rect.width(), 52.0));
        ui.painter().rect_filled(bar, 0.0, Color32::from_black_alpha(140));
        ui.painter().text(
            egui::pos2(bar.left() + 20.0, bar.center().y),
            Align2::LEFT_CENTER,
            format!("{}   {} · {}×{}{}", file.name, human_size(file.size), file.width, file.height, if n > 1 { format!("   {}/{}", lb.index + 1, n) } else { String::new() }),
            semibold(14.0),
            Color32::WHITE,
        );
        let mut buttons = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(vec2(14.0, 8.0))).layout(egui::Layout::right_to_left(egui::Align::Center)));
        if icon_button(&mut buttons, Icon::Close, Color32::WHITE, 20.0, "Close (Esc)").clicked() {
            close = true;
        }
        if icon_button(&mut buttons, Icon::External, Color32::WHITE, 18.0, "Open").clicked() {
            action = Some(Action::Download { file: file.clone(), open: true });
        }
        if icon_button(&mut buttons, Icon::Download, Color32::WHITE, 18.0, "Download").clicked() {
            action = Some(Action::Download { file: file.clone(), open: false });
        }
        if n > 1 {
            let mid = rect.center().y;
            let mut step = 0i64;
            for (x, icon, enabled, delta) in [
                (rect.left() + 30.0, Icon::ChevronLeft, lb.index > 0, -1),
                (rect.right() - 30.0, Icon::ChevronRight, lb.index + 1 < n, 1),
            ] {
                let r = Rect::from_center_size(egui::pos2(x, mid), vec2(44.0, 44.0));
                let b = ui.put(r, egui::Button::image(icon.image(Color32::WHITE, 26.0)).frame(false));
                if enabled && b.clicked() {
                    step = delta;
                }
            }
            if step != 0 {
                lb.index = (lb.index as i64 + step) as usize;
                lb.zoom = 1.0;
                lb.pan = egui::Vec2::ZERO;
            }
        }
        if resp.clicked() && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| !area.contains(p) && p.y > bar.bottom()) {
            close = true;
        }
    });
    let _ = t;
    if let Some(a) = action {
        app.actions.push(a);
    }
    if close {
        app.u.lightbox = None;
    }
}

pub fn toasts(app: &mut App, ctx: &egui::Context) {
    if app.u.toasts.is_empty() {
        return;
    }
    let t = app.d.theme.clone();
    let screen = ctx.content_rect();
    egui::Area::new(Id::new("toasts"))
        .order(Order::Tooltip)
        .anchor(Align2::CENTER_TOP, vec2(0.0, 64.0))
        .show(ctx, |ui| {
            ui.set_max_width(screen.width() * 0.6);
            for (text, _) in &app.u.toasts {
                Frame::new()
                    .fill(if t.dark { Color32::from_rgb(0x33, 0x38, 0x42) } else { Color32::from_rgb(0x22, 0x26, 0x2e) })
                    .corner_radius(8)
                    .inner_margin(Margin::symmetric(14, 10))
                    .show(ui, |ui| {
                        ui.label(RichText::new(text).color(Color32::WHITE).size(13.5));
                    });
                ui.add_space(6.0);
            }
        });
}
