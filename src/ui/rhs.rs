//! The right-hand panel: a thread with its own composer, search results,
//! or the channel's pinned messages.

use super::list::{Row, VList};
use super::post::{RowOpts, attachments, estimate, grouped, post_row, row_key};
use super::{Icon, icon_button, semibold};
use crate::app::{Action, App, Data, Rhs, SearchTab, UiState};
use crate::model::Post;
use egui::{Frame, Margin, RichText, Stroke, Ui, vec2};

pub fn show(app: &mut App, ui: &mut Ui) {
    let t = app.d.theme.clone();
    // A left border for the panel.
    let r = ui.max_rect();
    ui.painter().vline(r.left(), r.y_range(), Stroke::new(1.0, t.border));
    match app.d.rhs.clone() {
        Rhs::Thread(root) => thread(app, ui, &root),
        Rhs::Search => search(app, ui),
        Rhs::Pinned(ch) => pinned(app, ui, &ch),
        Rhs::None => {}
    }
}

fn title_bar(ui: &mut Ui, app: &mut App, title: &str, subtitle: &str) {
    let t = app.d.theme.clone();
    egui::Panel::top(egui::Id::new(("rhs-title", title)))
        .exact_size(54.0)
        .frame(Frame::new().fill(t.bg).inner_margin(Margin::symmetric(16, 0)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new(title).font(semibold(16.0)).color(t.text));
                if !subtitle.is_empty() {
                    ui.label(RichText::new(subtitle).size(13.0).color(t.text_dim));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Close, t.text_dim, 18.0, "Close").clicked() {
                        app.actions.push(Action::CloseRhs);
                    }
                });
            });
        });
}

fn thread(app: &mut App, ui: &mut Ui, root: &str) {
    let t = app.d.theme.clone();
    let channel = app
        .d
        .thread
        .as_ref()
        .and_then(|th| th.posts.first())
        .and_then(|p| app.d.channels.get(&p.channel_id))
        .map(|c| {
            let name = app.d.channel_name(c);
            if c.is_direct() { name } else { format!("~{name}") }
        })
        .unwrap_or_default();
    title_bar(ui, app, "Thread", &channel);
    let key = format!("t:{root}");
    egui::Panel::bottom(egui::Id::new("thread-composer"))
        .frame(Frame::new().fill(t.bg).inner_margin(Margin { left: 14, right: 14, top: 2, bottom: 14 }))
        .show_separator_line(false)
        .show(ui, |ui| super::composer::show(app, ui, &key));
    egui::CentralPanel::default().frame(Frame::new().fill(t.bg)).show(ui, |ui| {
        let App { d, u, actions, .. } = app;
        let Some(th) = d.thread.as_ref().filter(|th| th.root_id == root) else { return };
        if th.posts.is_empty() {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| super::spinner(ui, t.text_dim));
            return;
        }
        let width = ui.available_width();
        // Root, a divider, then the replies.
        enum K {
            Post(usize, bool),
            Divider(usize),
        }
        let mut kinds = Vec::new();
        let mut prev: Option<&Post> = None;
        for (i, p) in th.posts.iter().enumerate() {
            if i == 1 {
                kinds.push(K::Divider(th.posts.len() - 1));
                prev = None;
            }
            kinds.push(K::Post(i, i > 0 && grouped(prev, p)));
            prev = Some(p);
        }
        let rows: Vec<Row> = kinds
            .iter()
            .map(|k| match k {
                K::Post(i, c) => Row { key: row_key(&th.posts[*i], *c), estimate: estimate(&th.posts[*i], *c, width) },
                K::Divider(n) => Row { key: format!("div{n}"), estimate: 30.0 },
            })
            .collect();
        let list_key = format!("t:{root}");
        let mut list: VList = u.lists.remove(&list_key).unwrap_or_default();
        list.show(ui, ("thread-list", root), &rows, None, |ui, i| match &kinds[i] {
            K::Post(idx, compact) => {
                let opts = RowOpts { compact: *compact, in_thread: true, ..Default::default() };
                post_row(ui, d, u, actions, &th.posts[*idx], opts);
            }
            K::Divider(n) => {
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    let label = format!("{n} {}", if *n == 1 { "reply" } else { "replies" });
                    ui.label(RichText::new(label).size(12.5).color(t.text_dim));
                    let r = ui.available_rect_before_wrap();
                    ui.painter().hline(r.left() + 4.0..=r.right() - 16.0, r.center().y, Stroke::new(1.0, t.border));
                });
                ui.add_space(4.0);
            }
        });
        u.lists.insert(list_key, list);
        if !th.loaded {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
    });
}

fn result_card(ui: &mut Ui, d: &Data, u: &mut UiState, actions: &mut Vec<Action>, post: &Post) {
    let t = &d.theme;
    let channel = d.channels.get(&post.channel_id).map(|c| {
        let name = d.channel_name(c);
        if c.is_direct() { name } else { format!("~{name}") }
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.label(RichText::new(channel.unwrap_or_default()).font(semibold(12.0)).color(t.text_dim));
        ui.label(RichText::new(super::day_label(post.create_at)).size(12.0).color(t.text_dim));
        if !post.root_id.is_empty() {
            ui.label(RichText::new("in a thread").size(12.0).color(t.text_dim));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(12.0);
            if ui.small_button("Jump").clicked() {
                actions.push(Action::JumpTo(post.clone()));
            }
        });
    });
    post_row(ui, d, u, actions, post, RowOpts { result: true, in_thread: true, ..Default::default() });
    ui.add_space(4.0);
    let r = ui.available_rect_before_wrap();
    ui.painter().hline(r.left() + 16.0..=r.right() - 16.0, r.top(), Stroke::new(1.0, t.border));
}

fn search(app: &mut App, ui: &mut Ui) {
    let t = app.d.theme.clone();
    let terms = app.d.search.terms.clone();
    let subtitle = if terms.starts_with('"') { terms.clone() } else { format!("“{terms}”") };
    title_bar(ui, app, "Search", &subtitle);
    let App { d, u, actions, .. } = app;
    let s = &d.search;
    let posts: &[Post] = s.server.as_deref().unwrap_or(&s.local);
    ui.horizontal(|ui| {
        ui.add_space(14.0);
        let n = posts.len();
        if ui.selectable_label(u.search_tab == SearchTab::Messages, format!("Messages {n}")).clicked() {
            u.search_tab = SearchTab::Messages;
        }
        if ui.selectable_label(u.search_tab == SearchTab::Files, format!("Files {}", s.files.len())).clicked() {
            u.search_tab = SearchTab::Files;
        }
        if s.loading {
            super::spinner(ui, t.text_dim);
            ui.label(RichText::new(if s.local.is_empty() { "Searching…" } else { "Showing cached matches, searching server…" }).size(12.0).color(t.text_dim));
        } else if s.server.is_none() && !s.local.is_empty() {
            ui.label(RichText::new("Cached matches. Press Enter to search the server.").size(12.0).color(t.text_dim));
        }
    });
    if let Some(err) = &s.error {
        ui.horizontal(|ui| {
            ui.add_space(14.0);
            ui.label(RichText::new(err).color(t.error).size(12.5));
        });
    }
    ui.separator();
    egui::ScrollArea::vertical().id_salt("search-results").auto_shrink([false, false]).show(ui, |ui| {
        match u.search_tab {
            SearchTab::Messages => {
                if posts.is_empty() && !s.loading {
                    let note = if s.server.is_some() { "No matching messages." } else { "Nothing in the local cache. Press Enter to search the server." };
                    super::centered_note(ui, note, t.text_dim);
                }
                for p in posts.iter().take(60) {
                    result_card(ui, d, u, actions, p);
                }
            }
            SearchTab::Files => {
                if s.files.is_empty() && !s.loading {
                    super::centered_note(ui, if s.server.is_some() { "No matching files." } else { "Press Enter to search files on the server." }, t.text_dim);
                }
                Frame::new().inner_margin(Margin::symmetric(16, 8)).show(ui, |ui| {
                    for f in &s.files {
                        let channel = d.channels.get(&f.channel_id).map(|c| d.channel_name(c)).unwrap_or_default();
                        ui.label(RichText::new(format!("{channel} · {}", super::day_label(f.create_at))).size(11.5).color(t.text_dim));
                        attachments(ui, d, u, actions, std::slice::from_ref(f), true);
                        ui.add_space(6.0);
                    }
                });
            }
        }
        ui.add_space(20.0);
    });
    let _ = vec2;
}

fn pinned(app: &mut App, ui: &mut Ui, channel_id: &str) {
    let t = app.d.theme.clone();
    let name = app.d.channels.get(channel_id).map(|c| app.d.channel_name(c)).unwrap_or_default();
    title_bar(ui, app, "Pinned messages", &name);
    let App { d, u, actions, .. } = app;
    egui::ScrollArea::vertical().id_salt("pinned").auto_shrink([false, false]).show(ui, |ui| {
        match d.pinned.get(channel_id) {
            None => {
                ui.add_space(30.0);
                ui.vertical_centered(|ui| super::spinner(ui, t.text_dim));
            }
            Some(list) if list.is_empty() => super::centered_note(ui, "Nothing is pinned in this channel yet.", t.text_dim),
            Some(list) => {
                for p in list {
                    result_card(ui, d, u, actions, p);
                }
            }
        }
    });
}
