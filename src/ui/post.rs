//! The message list and one message: avatar, name, markdown body, images,
//! file cards, reactions, the thread summary, and the hover action bar.

use super::list::{Row, VList};
use super::{Icon, avatar, day_label, day_number, full_time, icon_button, medium, semibold, time_hm};
use crate::app::{Action, App, Data, PostMenu, Picker, UiState};
use crate::markup::{self, RenderCtx};
use crate::model::*;
use egui::{Align, Color32, Frame, Layout, Margin, Rect, RichText, Sense, Stroke, Ui, UiBuilder, pos2, vec2};

#[derive(Clone, Copy, Default)]
pub struct RowOpts {
    pub compact: bool,
    pub in_thread: bool,
    pub highlight: bool,
    /// Search and pinned results: no hover bar, a jump button instead.
    pub result: bool,
}

enum Kind {
    Start,
    Loading,
    Date(i64),
    NewLine,
    Post { idx: usize, compact: bool },
    /// A run of join/leave messages, shown as one summary line.
    Activity { start: usize, end: usize },
}

/// Joins, leaves, adds and removals: Mattermost folds runs of these into one line.
pub fn is_membership(p: &Post) -> bool {
    matches!(
        p.kind.as_str(),
        "system_join_team"
            | "system_leave_team"
            | "system_join_channel"
            | "system_leave_channel"
            | "system_add_to_channel"
            | "system_remove_from_channel"
            | "system_add_to_team"
            | "system_remove_from_team"
    )
}

/// "alice, bob and 3 others joined the team. carol left the team."
fn activity_summary(d: &Data, posts: &[Post]) -> (String, String) {
    let mut groups: Vec<(&'static str, Vec<String>)> = Vec::new();
    let mut details = Vec::new();
    for p in posts {
        let verb = match p.kind.as_str() {
            "system_join_team" => "joined the team",
            "system_leave_team" => "left the team",
            "system_join_channel" => "joined the channel",
            "system_leave_channel" => "left the channel",
            "system_add_to_channel" => "were added to the channel",
            "system_remove_from_channel" => "were removed from the channel",
            "system_add_to_team" => "were added to the team",
            _ => "were removed from the team",
        };
        let who = ["addedUsername", "removedUsername", "username"]
            .iter()
            .find_map(|k| p.props[*k].as_str().filter(|s| !s.is_empty()).map(str::to_string))
            .or_else(|| p.message.split_whitespace().next().map(str::to_string))
            .unwrap_or_else(|| d.user_name(&p.user_id));
        details.push(format!("{}  {}", super::day_label(p.create_at), p.message));
        match groups.iter_mut().find(|(v, _)| *v == verb) {
            Some((_, names)) => {
                if !names.contains(&who) {
                    names.push(who)
                }
            }
            None => groups.push((verb, vec![who])),
        }
    }
    let parts: Vec<String> = groups
        .into_iter()
        .map(|(verb, names)| {
            let verb = if names.len() == 1 { verb.replace("were ", "was ") } else { verb.to_string() };
            let who = match names.len() {
                1 => names[0].clone(),
                2 => format!("{} and {}", names[0], names[1]),
                3 => format!("{}, {} and {}", names[0], names[1], names[2]),
                n => format!("{}, {} and {} others", names[0], names[1], n - 2),
            };
            format!("{who} {verb}.")
        })
        .collect();
    let tip = if details.len() > 40 {
        let more = details.len() - 40;
        details.truncate(40);
        format!("{}\n… and {more} more", details.join("\n"))
    } else {
        details.join("\n")
    };
    (parts.join(" "), tip)
}

fn activity_row(ui: &mut Ui, d: &Data, posts: &[Post]) {
    let t = &d.theme;
    let (text, tip) = activity_summary(d, posts);
    Frame::new().inner_margin(Margin { left: 16, right: 16, top: 3, bottom: 3 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(36.0, 18.0), Layout::top_down(Align::Center), |ui| {
                ui.add(Icon::Users.image(t.text_dim, 13.0));
            });
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new(text).size(13.0).color(t.text_dim)).wrap()).on_hover_text(tip);
        });
    });
}

const COMPACT_GAP_MS: i64 = 5 * 60 * 1000;

pub fn grouped(prev: Option<&Post>, p: &Post) -> bool {
    match prev {
        Some(q) => {
            q.user_id == p.user_id
                && !q.is_system()
                && !p.is_system()
                && p.create_at - q.create_at < COMPACT_GAP_MS
                && day_number(p.create_at) == day_number(q.create_at)
        }
        None => false,
    }
}

pub fn row_key(p: &Post, compact: bool) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}{}",
        p.id,
        p.edit_at.max(p.update_at),
        p.metadata.reactions.len(),
        p.reply_count,
        compact as u8,
        p.pending as u8,
        p.failed as u8
    )
}

pub fn estimate(p: &Post, compact: bool, width: f32) -> f32 {
    let chars_per_line = ((width - 90.0) / 7.2).max(20.0);
    let lines = p.message.lines().map(|l| (l.chars().count() as f32 / chars_per_line).ceil().max(1.0)).sum::<f32>().max(1.0);
    if p.is_system() {
        return 24.0;
    }
    let mut h = if compact { 6.0 } else { 34.0 } + lines * 20.0;
    for f in &p.metadata.files {
        h += if f.is_image() { 230.0 } else { 62.0 };
    }
    if !p.metadata.reactions.is_empty() {
        h += 30.0;
    }
    if p.reply_count > 0 {
        h += 26.0;
    }
    h
}

pub fn channel_list(app: &mut App, ui: &mut Ui) {
    let ch_id = app.d.current.clone();
    let App { d, u, actions, .. } = app;
    let t = &d.theme;
    let Some(view) = d.views.get(&ch_id).filter(|v| v.loaded) else {
        ui.add_space(30.0);
        ui.vertical_centered(|ui| super::spinner(ui, t.text_dim));
        return;
    };
    let me = d.me.id.clone();
    let width = ui.available_width();
    let mut kinds = vec![if view.reached_start { Kind::Start } else { Kind::Loading }];
    let mut prev: Option<&Post> = None;
    let mut new_line = view.unread_after == 0;
    let mut idx = 0;
    while idx < view.posts.len() {
        let p = &view.posts[idx];
        if is_membership(p) {
            // Fold the whole run, across days, into one row under one date.
            let mut end = idx + 1;
            while end < view.posts.len() && is_membership(&view.posts[end]) {
                end += 1;
            }
            if prev.is_none_or(|q| day_number(q.create_at) != day_number(p.create_at)) {
                kinds.push(Kind::Date(p.create_at));
            }
            kinds.push(Kind::Activity { start: idx, end });
            prev = Some(&view.posts[end - 1]);
            idx = end;
            continue;
        }
        let day = day_number(p.create_at);
        let mut separated = false;
        if prev.is_none_or(|q| day_number(q.create_at) != day) {
            kinds.push(Kind::Date(p.create_at));
            separated = true;
        }
        if !new_line && p.create_at > view.unread_after && p.user_id != me && !p.pending {
            kinds.push(Kind::NewLine);
            new_line = true;
            separated = true;
        }
        let compact = !separated && grouped(prev, p);
        kinds.push(Kind::Post { idx, compact });
        prev = Some(p);
        idx += 1;
    }
    let rows: Vec<Row> = kinds
        .iter()
        .map(|k| match k {
            Kind::Start => Row { key: "start".into(), estimate: 90.0 },
            Kind::Loading => Row { key: "loading".into(), estimate: 40.0 },
            Kind::Date(ms) => Row { key: format!("d{}", day_number(*ms)), estimate: 40.0 },
            Kind::NewLine => Row { key: "new".into(), estimate: 28.0 },
            Kind::Post { idx, compact } => {
                let p = &view.posts[*idx];
                Row { key: row_key(p, *compact), estimate: estimate(p, *compact, width) }
            }
            Kind::Activity { start, end } => {
                Row { key: format!("act{}:{}", view.posts[*start].id, end - start), estimate: 24.0 }
            }
        })
        .collect();

    // Jumping to a post: page back until it is loaded.
    let mut reveal = None;
    if let Some(target) = &view.reveal {
        reveal = kinds.iter().position(|k| matches!(k, Kind::Post { idx, .. } if &view.posts[*idx].id == target));
        if reveal.is_none() && view.server_loaded && !view.reached_start && !view.loading_older {
            actions.push(Action::LoadOlder(ch_id.clone()));
        }
    }
    let highlight = view.highlight.as_ref().filter(|(_, until)| *until > u.now).map(|(id, _)| id.clone());
    if highlight.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(400));
    }

    let list_key = format!("c:{ch_id}");
    let mut list: VList = u.lists.remove(&list_key).unwrap_or_default();
    let out = list.show(ui, ("channel-list", &ch_id), &rows, reveal, |ui, i| match &kinds[i] {
        Kind::Start => channel_intro(ui, d, &ch_id),
        Kind::Loading => {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| super::spinner(ui, t.text_dim));
            ui.add_space(12.0);
        }
        Kind::Date(ms) => date_separator(ui, d, *ms),
        Kind::NewLine => new_messages_line(ui, d),
        Kind::Activity { start, end } => activity_row(ui, d, &view.posts[*start..*end]),
        Kind::Post { idx, compact } => {
            let p = &view.posts[*idx];
            let opts = RowOpts { compact: *compact, highlight: highlight.as_deref() == Some(p.id.as_str()), ..Default::default() };
            post_row(ui, d, u, actions, p, opts);
        }
    });
    u.lists.insert(list_key, list);
    if out.near_top && !view.reached_start && !view.loading_older && view.server_loaded {
        actions.push(Action::LoadOlder(ch_id.clone()));
    }
    if reveal.is_some()
        && let Some(v) = d.views.get_mut(&ch_id)
    {
        v.reveal = None;
    }
}

fn channel_intro(ui: &mut Ui, d: &Data, ch_id: &str) {
    let t = &d.theme;
    let Some(ch) = d.channels.get(ch_id) else { return };
    ui.add_space(24.0);
    ui.horizontal(|ui| {
        ui.add_space(18.0);
        ui.vertical(|ui| {
            ui.label(RichText::new(format!("Beginning of {}", d.channel_name(ch))).font(semibold(18.0)).color(t.text));
            let note = if ch.is_direct() {
                "This is the start of your conversation.".to_string()
            } else if !ch.purpose.is_empty() {
                ch.purpose.clone()
            } else {
                "This is the start of the channel.".to_string()
            };
            ui.label(RichText::new(note).color(t.text_dim));
        });
    });
    ui.add_space(12.0);
}

fn date_separator(ui: &mut Ui, d: &Data, ms: i64) {
    let t = &d.theme;
    ui.add_space(10.0);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    let label = day_label(ms);
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(label, semibold(12.0), t.text));
    let pill = Rect::from_center_size(rect.center(), galley.size() + vec2(20.0, 6.0));
    let y = rect.center().y;
    ui.painter().hline(rect.left() + 18.0..=pill.left() - 8.0, y, Stroke::new(1.0, t.border));
    ui.painter().hline(pill.right() + 8.0..=rect.right() - 18.0, y, Stroke::new(1.0, t.border));
    ui.painter().rect(pill, 10.0, t.bg, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
    ui.painter().galley(pill.center() - galley.size() / 2.0, galley, t.text);
    ui.add_space(4.0);
}

fn new_messages_line(ui: &mut Ui, d: &Data) {
    let t = &d.theme;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::hover());
    let y = rect.center().y;
    ui.painter().hline(rect.left() + 18.0..=rect.right() - 110.0, y, Stroke::new(1.0, t.unread_line));
    ui.painter().text(
        pos2(rect.right() - 18.0, y),
        egui::Align2::RIGHT_CENTER,
        "New messages",
        semibold(11.5),
        t.unread_line,
    );
}

/// Search terms worth marking in results (modifiers like `from:` left out).
pub fn search_terms(terms: &str) -> Vec<String> {
    terms
        .split_whitespace()
        .filter(|w| !w.contains(':') && !w.starts_with('-'))
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|w| w.len() >= 2)
        .collect()
}

/// Joins, leaves, header changes: one quiet line, no avatar.
fn system_row(ui: &mut Ui, d: &Data, post: &Post) {
    let t = &d.theme;
    Frame::new().inner_margin(Margin { left: 16, right: 16, top: 3, bottom: 3 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(38.0, 18.0), Layout::top_down(Align::Center), |ui| {
                ui.add(Icon::Info.image(t.text_dim, 13.0));
            });
            ui.add_space(6.0);
            let text = crate::markup::plain(&post.message);
            ui.add(egui::Label::new(RichText::new(text).size(13.0).color(t.text_dim)).wrap())
                .on_hover_text(full_time(post.create_at));
        });
    });
}

pub fn post_row(ui: &mut Ui, d: &Data, u: &mut UiState, actions: &mut Vec<Action>, post: &Post, o: RowOpts) {
    let t = &d.theme;
    if post.is_system() {
        system_row(ui, d, post);
        return;
    }
    let full = ui.max_rect();
    let bg_idx = ui.painter().add(egui::Shape::Noop);
    let top = if o.compact { 1 } else { 7 };
    let inner = Frame::new().inner_margin(Margin { left: 16, right: 16, top, bottom: 3 }).show(ui, |ui| {
        ui.set_width(full.width() - 32.0);
        ui.horizontal_top(|ui| {
            if o.compact {
                ui.allocate_exact_size(vec2(36.0, 1.0), Sense::hover());
            } else {
                let r = avatar(ui, &mut u.images, d, &post.user_id, 36.0).on_hover_cursor(egui::CursorIcon::PointingHand);
                if r.clicked() {
                    u.profile = Some((post.user_id.clone(), r.rect.right_top()));
                }
            }
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                if !o.compact {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        let name = d.user_name(&post.user_id);
                        let r = ui
                            .add(egui::Label::new(RichText::new(name).font(semibold(14.5)).color(t.text)).sense(Sense::click()))
                            .on_hover_cursor(egui::CursorIcon::PointingHand);
                        if r.clicked() {
                            u.profile = Some((post.user_id.clone(), r.rect.left_bottom()));
                        }
                        if d.users.get(&post.user_id).is_some_and(|u| u.is_bot) {
                            ui.label(RichText::new("BOT").font(semibold(10.0)).color(t.text_dim));
                        }
                        ui.label(RichText::new(time_hm(post.create_at)).size(12.0).color(t.text_dim))
                            .on_hover_text(full_time(post.create_at));
                        if post.is_pinned {
                            ui.add(Icon::Pin.image(t.text_dim, 12.0)).on_hover_text("Pinned");
                        }
                    });
                }
                body(ui, d, u, actions, post, o);
            });
        });
    });
    let rect = Rect::from_x_y_ranges(full.x_range(), inner.response.rect.y_range());
    let menu_here = u.menu.as_ref().is_some_and(|m| m.post_id == post.id)
        || u.picker.as_ref().is_some_and(|p| p.post_id == post.id);
    let overlay_open = u.menu.is_some() || u.picker.is_some() || u.lightbox.is_some();
    let hovered = ui.rect_contains_pointer(rect) && !overlay_open;
    let fill = if o.highlight {
        Some(t.highlight_bg)
    } else if hovered || menu_here {
        Some(t.hover_bg)
    } else {
        None
    };
    if let Some(fill) = fill {
        ui.painter().set(bg_idx, egui::Shape::rect_filled(rect, 0.0, fill));
    }
    if o.compact && hovered {
        ui.painter().text(
            pos2(rect.left() + 35.0, rect.top() + 12.0),
            egui::Align2::CENTER_CENTER,
            time_hm(post.create_at),
            egui::FontId::proportional(10.5),
            t.text_dim,
        );
    }
    if (hovered || menu_here) && !o.result && !post.pending && !post.is_system() {
        action_bar(ui, d, u, actions, post, rect, o);
    }
}

fn body(ui: &mut Ui, d: &Data, u: &mut UiState, actions: &mut Vec<Action>, post: &Post, o: RowOpts) {
    let t = &d.theme;
    if post.is_system() {
        ui.label(RichText::new(&post.message).italics().color(t.text_dim));
        return;
    }
    let editing = u.editing.as_ref().is_some_and(|(id, _)| id == &post.id);
    if editing {
        edit_box(ui, d, u, actions, post);
    } else if !post.message.is_empty() {
        let doc = u.doc(post);
        let color = if post.pending { t.text_dim } else { t.text };
        let terms: Vec<String> = if o.result { search_terms(&d.search.terms) } else { Vec::new() };
        let paint = std::cell::RefCell::new(Vec::new());
        let rc = RenderCtx { theme: t, size: 14.5, me: &d.me.username, color, highlight: &terms, emoji: &d.custom_emoji, paint: Some(&paint) };
        if let Some(link) = markup::show(ui, &doc, &rc) {
            actions.push(Action::Link(link));
        }
        paint_emoji(ui, u, paint.into_inner());
        if post.edit_at > 0 {
            ui.label(RichText::new("(edited)").size(11.5).color(t.text_dim)).on_hover_text(full_time(post.edit_at));
        }
    }
    let files = &post.metadata.files;
    if !files.is_empty() {
        attachments(ui, d, u, actions, files, o.result);
    }
    if !post.metadata.reactions.is_empty() {
        reactions(ui, d, actions, u, post);
    }
    if !o.in_thread && post.root_id.is_empty() && post.reply_count > 0 {
        thread_summary(ui, d, u, actions, post);
    }
    if post.failed {
        ui.horizontal(|ui| {
            ui.add(Icon::Alert.image(t.error, 14.0));
            ui.label(RichText::new("Message failed to send.").size(12.5).color(t.error));
            if ui.link("Retry").clicked() {
                actions.push(Action::Retry(post.id.clone()));
            }
            if ui.link("Discard").clicked() {
                actions.push(Action::Delete(post.id.clone()));
            }
        });
    } else if post.pending {
        ui.label(RichText::new("Sending…").size(11.5).color(t.text_dim));
    }
}

/// Paint custom emoji over the placeholders laid out for them.
pub fn paint_emoji(ui: &Ui, u: &mut UiState, at: Vec<(Rect, String)>) {
    for (rect, id) in at {
        if let Some(tex) = u.images.get(ui.ctx(), &ImageKey::Emoji(id)) {
            egui::Image::new(&tex).paint_at(ui, rect);
        }
    }
}

fn edit_box(ui: &mut Ui, d: &Data, u: &mut UiState, actions: &mut Vec<Action>, post: &Post) {
    let t = &d.theme;
    let id = ui.make_persistent_id(("edit", &post.id));
    let mut save = false;
    let mut cancel = false;
    if ui.memory(|m| m.has_focus(id)) {
        ui.input_mut(|i| {
            i.events.retain(|e| match e {
                egui::Event::Key { key: egui::Key::Enter, pressed: true, modifiers, .. } if !modifiers.shift => {
                    save = true;
                    false
                }
                egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => {
                    cancel = true;
                    false
                }
                _ => true,
            })
        });
    }
    let Some((_, text)) = u.editing.as_mut() else { return };
    Frame::new().stroke(Stroke::new(1.0, t.accent)).corner_radius(6).inner_margin(Margin::same(6)).show(ui, |ui| {
        let resp = ui.add(egui::TextEdit::multiline(text).id(id).frame(egui::Frame::NONE).desired_rows(1).desired_width(f32::INFINITY));
        if !resp.has_focus() && !resp.lost_focus() {
            resp.request_focus();
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Enter to save · Esc to cancel").size(11.5).color(t.text_dim));
        if ui.small_button("Cancel").clicked() {
            cancel = true;
        }
        if ui.small_button("Save").clicked() {
            save = true;
        }
    });
    if save {
        let text = text.trim().to_string();
        if text != post.message && !text.is_empty() {
            actions.push(Action::SaveEdit { post_id: post.id.clone(), text });
        }
        u.editing = None;
    } else if cancel {
        u.editing = None;
    }
}

fn image_size(f: &FileInfo, max: egui::Vec2) -> egui::Vec2 {
    let (w, h) = (f.width.max(1) as f32, f.height.max(1) as f32);
    let scale = (max.x / w).min(max.y / h).min(1.0);
    vec2((w * scale).max(40.0), (h * scale).max(30.0))
}

pub fn attachments(ui: &mut Ui, d: &Data, u: &mut UiState, actions: &mut Vec<Action>, files: &[FileInfo], small: bool) {
    let t = &d.theme;
    let images: Vec<FileInfo> = files.iter().filter(|f| f.is_image()).cloned().collect();
    let others: Vec<&FileInfo> = files.iter().filter(|f| !f.is_image()).collect();
    if !images.is_empty() {
        ui.add_space(2.0);
        let max = if small {
            vec2(160.0, 110.0)
        } else if images.len() == 1 {
            vec2(ui.available_width().min(480.0), 340.0)
        } else {
            vec2(230.0, 170.0)
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for (i, f) in images.iter().enumerate() {
                let size = image_size(f, max);
                let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
                let key = if f.is_gif() { ImageKey::Animated(f.id.clone()) } else { ImageKey::Preview(f.id.clone()) };
                let tex = if f.is_gif() { u.images.frame(ui.ctx(), &key, u.now) } else { u.images.get(ui.ctx(), &key) };
                match tex {
                    Some(tex) => egui::Image::new(&tex).corner_radius(6).paint_at(ui, rect),
                    None => {
                        ui.painter().rect_filled(rect, 6.0, t.code_block_bg);
                        let icon = if u.images.failed(&key) { Icon::Alert } else { Icon::Image };
                        icon.image(t.text_dim, 22.0).paint_at(ui, Rect::from_center_size(rect.center(), vec2(22.0, 22.0)));
                    }
                }
                ui.painter().rect_stroke(rect, 6.0, Stroke::new(1.0, t.border), egui::StrokeKind::Inside);
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(&f.name);
                if resp.clicked() {
                    actions.push(Action::OpenLightbox { files: images.clone(), index: i });
                }
            }
        });
    }
    for f in others {
        file_card(ui, d, actions, f);
    }
}

pub fn file_card(ui: &mut Ui, d: &Data, actions: &mut Vec<Action>, f: &FileInfo) {
    let t = &d.theme;
    ui.add_space(2.0);
    let resp = Frame::new()
        .fill(t.bg)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(8)
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width().min(340.0));
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(34.0, 34.0), Sense::hover());
                let ext = f.extension.to_ascii_uppercase();
                let color = match ext.as_str() {
                    "PDF" => Color32::from_rgb(0xd9, 0x48, 0x3b),
                    "CSV" | "XLSX" | "XLS" => Color32::from_rgb(0x1f, 0x9d, 0x55),
                    "ZIP" | "TAR" | "GZ" => Color32::from_rgb(0x8a, 0x6d, 0x3b),
                    "PY" | "RS" | "C" | "CPP" | "JSON" | "TXT" | "LOG" => Color32::from_rgb(0x4a, 0x6f, 0xd1),
                    _ => t.text_dim,
                };
                ui.painter().rect_filled(r, 6.0, color.gamma_multiply(0.15));
                Icon::File.image(color, 18.0).paint_at(ui, Rect::from_center_size(r.center(), vec2(18.0, 18.0)));
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width() - 64.0);
                    let g = super::elided(ui, &f.name, medium(13.5), t.text, ui.available_width());
                    ui.label(g);
                    ui.label(RichText::new(format!("{} · {}", ext, human_size(f.size))).size(11.5).color(t.text_dim));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_button(ui, Icon::Download, t.text_dim, 16.0, "Download").clicked() {
                        actions.push(Action::Download { file: f.clone(), open: false });
                    }
                    if icon_button(ui, Icon::External, t.text_dim, 16.0, "Open").clicked() {
                        actions.push(Action::Download { file: f.clone(), open: true });
                    }
                });
            });
        })
        .response;
    let _ = resp;
}

fn reactions(ui: &mut Ui, d: &Data, actions: &mut Vec<Action>, u: &mut UiState, post: &Post) {
    let t = &d.theme;
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    for r in &post.metadata.reactions {
        match groups.iter_mut().find(|(e, _)| e == &r.emoji_name) {
            Some((_, users)) => users.push(r.user_id.clone()),
            None => groups.push((r.emoji_name.clone(), vec![r.user_id.clone()])),
        }
    }
    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
        for (emoji, users) in &groups {
            let mine = users.iter().any(|u| u == &d.me.id);
            let custom = markup::emoji_for(emoji).is_none().then(|| d.custom_emoji.get(emoji.as_str())).flatten();
            let glyph = markup::emoji_for(emoji).map(str::to_string).unwrap_or_else(|| if custom.is_some() { String::new() } else { format!(":{emoji}:") });
            let text = RichText::new(format!("{glyph} {}", users.len())).size(13.0).color(if mine { t.link } else { t.text });
            let image = custom.and_then(|id| u.images.get(ui.ctx(), &ImageKey::Emoji(id.clone())));
            let btn = match &image {
                Some(tex) => egui::Button::image_and_text(egui::Image::new(tex).fit_to_exact_size(vec2(16.0, 16.0)), text),
                None => egui::Button::new(text),
            };
            let btn = btn
                .fill(if mine { t.reaction_mine } else { t.reaction_bg })
                .stroke(Stroke::new(1.0, if mine { t.accent.gamma_multiply(0.6) } else { t.border }))
                .corner_radius(10)
                .min_size(vec2(0.0, 24.0));
            let names: Vec<String> = users.iter().map(|id| d.user_name(id)).collect();
            let resp = ui.add(btn).on_hover_text(format!("{} reacted with :{emoji}:", names.join(", ")));
            if resp.clicked() {
                actions.push(Action::React { post_id: post.id.clone(), emoji: emoji.clone(), add: !mine });
            }
        }
        let add = egui::Button::image(Icon::React.image(t.text_dim, 14.0))
            .fill(t.reaction_bg)
            .stroke(Stroke::new(1.0, t.border))
            .corner_radius(10)
            .min_size(vec2(30.0, 24.0));
        let resp = ui.add(add).on_hover_text("Add a reaction");
        if resp.clicked() {
            u.picker = Some(Picker { post_id: post.id.clone(), pos: resp.rect.left_bottom(), query: String::new() });
        }
    });
}

fn thread_summary(ui: &mut Ui, d: &Data, u: &mut UiState, actions: &mut Vec<Action>, post: &Post) {
    let t = &d.theme;
    ui.add_space(2.0);
    let resp = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if let Some(parts) = &post.participants {
                for p in parts.iter().take(4) {
                    super::avatar(ui, &mut u.images, d, &p.id, 20.0);
                }
            }
            ui.add_space(4.0);
            let n = post.reply_count;
            let label = RichText::new(format!("{n} {}", if n == 1 { "reply" } else { "replies" }))
                .font(semibold(13.0))
                .color(t.link);
            let r = ui.add(egui::Label::new(label).sense(Sense::click()));
            if post.last_reply_at > 0 {
                ui.label(RichText::new(format!("Last reply {}", super::relative(post.last_reply_at))).size(12.0).color(t.text_dim));
            }
            r
        })
        .inner;
    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        actions.push(Action::OpenThread(post.id.clone()));
    }
}

fn action_bar(ui: &mut Ui, d: &Data, u: &mut UiState, actions: &mut Vec<Action>, post: &Post, rect: Rect, o: RowOpts) {
    let t = &d.theme;
    let w = 104.0;
    let bar = Rect::from_min_size(pos2(rect.right() - w - 16.0, rect.top() + 2.0), vec2(w, 30.0));
    let mut child = ui.new_child(UiBuilder::new().max_rect(bar).layout(Layout::left_to_right(Align::Center)));
    Frame::new()
        .fill(t.bg)
        .stroke(Stroke::new(1.0, t.border))
        .corner_radius(6)
        .inner_margin(Margin::symmetric(2, 0))
        .shadow(egui::Shadow { offset: [0, 1], blur: 4, spread: 0, color: Color32::from_black_alpha(25) })
        .show(&mut child, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let react = icon_button(ui, Icon::React, t.text_dim, 16.0, "Add reaction");
            if react.clicked() {
                u.picker = Some(Picker { post_id: post.id.clone(), pos: react.rect.left_bottom(), query: String::new() });
                u.menu = None;
            }
            if !o.in_thread {
                let root = if post.root_id.is_empty() { post.id.clone() } else { post.root_id.clone() };
                if icon_button(ui, Icon::Reply, t.text_dim, 16.0, "Reply in thread").clicked() {
                    actions.push(Action::OpenThread(root));
                }
            } else {
                ui.add_space(26.0);
            }
            let more = icon_button(ui, Icon::More, t.text_dim, 16.0, "More");
            if more.clicked() {
                u.menu = Some(PostMenu { post_id: post.id.clone(), pos: more.rect.left_bottom() });
                u.picker = None;
            }
        });
}
