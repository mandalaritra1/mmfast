//! The message box: Enter sends, Shift+Enter breaks the line, Up edits
//! your last message; @user, ~channel and :emoji: complete; files attach
//! from the picker, by drag and drop, or by pasting an image.

use super::{Icon, icon_button, semibold};
use crate::app::{Action, App};
use crate::backend::UploadData;
use crate::model::User;
use egui::text::{CCursor, CCursorRange};
use egui::{Align, Event, Frame, Key, Layout, Margin, RichText, Stroke, Ui, vec2};

#[derive(Clone)]
enum Completion {
    User(User),
    Channel(String, String),
    Emoji(String, String),
}

impl Completion {
    fn insert(&self) -> String {
        match self {
            Completion::User(u) => format!("@{} ", u.username),
            Completion::Channel(name, _) => format!("~{name} "),
            Completion::Emoji(code, _) => format!(":{code}: "),
        }
    }
}

/// The token being typed at the end of the text, if it can be completed.
fn token(text: &str) -> Option<(char, String)> {
    let last = text.rsplit(|c: char| c.is_whitespace()).next()?;
    let mut chars = last.chars();
    let sigil = chars.next()?;
    let rest: String = chars.collect();
    match sigil {
        '@' | '~' => Some((sigil, rest)),
        ':' if rest.chars().count() >= 2 && !rest.contains(':') => Some((sigil, rest)),
        _ => None,
    }
}

fn completions(app: &App, sigil: char, prefix: &str) -> Vec<Completion> {
    let d = &app.d;
    let p = prefix.to_lowercase();
    match sigil {
        '@' => {
            let mut users: Vec<&User> = d
                .users
                .values()
                .filter(|u| u.delete_at == 0)
                .filter(|u| {
                    u.username.to_lowercase().starts_with(&p)
                        || u.first_name.to_lowercase().starts_with(&p)
                        || u.last_name.to_lowercase().starts_with(&p)
                })
                .collect();
            users.sort_by_key(|u| (u.id == d.me.id, u.username.clone()));
            users.dedup_by_key(|u| u.id.clone());
            let mut out: Vec<Completion> = users.into_iter().take(8).cloned().map(Completion::User).collect();
            for special in ["here", "channel", "all"] {
                if special.starts_with(&p) && !p.is_empty() {
                    out.push(Completion::User(User { username: special.into(), first_name: format!("Notify everyone ({special})"), ..Default::default() }));
                }
            }
            out
        }
        '~' => {
            let mut chans: Vec<(String, String)> = d
                .channels
                .values()
                .filter(|c| !c.is_direct() && c.team_id == d.team)
                .filter(|c| c.name.to_lowercase().starts_with(&p) || c.display_name.to_lowercase().starts_with(&p))
                .map(|c| (c.name.clone(), c.display_name.clone()))
                .collect();
            chans.sort();
            chans.into_iter().take(8).map(|(n, dn)| Completion::Channel(n, dn)).collect()
        }
        _ => {
            let mut out = Vec::new();
            for e in emojis::iter() {
                for code in e.shortcodes() {
                    if code.starts_with(&p) {
                        out.push(Completion::Emoji(code.to_string(), e.as_str().to_string()));
                        break;
                    }
                }
                if out.len() >= 8 {
                    break;
                }
            }
            let mut custom: Vec<&String> = d.custom_emoji.keys().filter(|n| n.starts_with(&p)).collect();
            custom.sort();
            for name in custom.into_iter().take(8usize.saturating_sub(out.len()).max(3)) {
                out.push(Completion::Emoji(name.clone(), String::new()));
            }
            out
        }
    }
}

pub fn show(app: &mut App, ui: &mut Ui, key: &str) {
    let t = app.d.theme.clone();
    let id = egui::Id::new(("composer", key));
    let has_focus = ui.memory(|m| m.has_focus(id));

    // Who is typing here.
    let (channel_id, parent_id) = match key.strip_prefix("t:") {
        Some(root) => (app.d.thread.as_ref().and_then(|t| t.posts.first()).map(|p| p.channel_id.clone()).unwrap_or_default(), root.to_string()),
        None => (key.trim_start_matches("c:").to_string(), String::new()),
    };
    let typing: Vec<String> = app
        .d
        .typing
        .iter()
        .filter(|x| x.channel_id == channel_id && (parent_id.is_empty() || x.parent_id == parent_id))
        .map(|x| app.d.user_name(&x.user_id))
        .collect();
    let typing_text = match typing.len() {
        0 => String::new(),
        1 => format!("{} is typing…", typing[0]),
        2 => format!("{} and {} are typing…", typing[0], typing[1]),
        _ => "Several people are typing…".to_string(),
    };
    ui.label(RichText::new(typing_text).size(11.5).color(t.text_dim));

    let text_now = app.u.composers.get(key).map(|c| c.text.clone()).unwrap_or_default();
    let tok = if has_focus { token(&text_now) } else { None };
    let comps = match &tok {
        Some((sigil, prefix)) => completions(app, *sigil, prefix),
        None => Vec::new(),
    };
    if let Some(('@', prefix)) = &tok
        && !prefix.is_empty()
        && ui.ctx().input(|i| i.events.iter().any(|e| matches!(e, Event::Text(_) | Event::Key { key: Key::Backspace, .. })))
    {
        app.actions.push(Action::Autocomplete(prefix.clone()));
    }
    if app.u.ac_selected >= comps.len().max(1) {
        app.u.ac_selected = 0;
    }

    // Keys the text box must not see.
    let mut send = false;
    let mut pick: Option<Completion> = None;
    let mut edit_last = false;
    let mut paste_check = false;
    if has_focus {
        let n = comps.len();
        let selected = &mut app.u.ac_selected;
        let empty = text_now.is_empty();
        ui.input_mut(|i| {
            i.events.retain(|e| match e {
                Event::Key { key: Key::Enter | Key::Tab, pressed: true, modifiers, .. } if n > 0 && !modifiers.shift => {
                    pick = comps.get(*selected).cloned();
                    false
                }
                Event::Key { key: Key::ArrowDown, pressed: true, .. } if n > 0 => {
                    *selected = (*selected + 1) % n;
                    false
                }
                Event::Key { key: Key::ArrowUp, pressed: true, .. } if n > 0 => {
                    *selected = (*selected + n - 1) % n;
                    false
                }
                Event::Key { key: Key::ArrowUp, pressed: true, modifiers, .. } if empty && modifiers.is_none() => {
                    edit_last = true;
                    false
                }
                Event::Key { key: Key::Enter, pressed: true, modifiers, .. } if !modifiers.shift => {
                    send = true;
                    false
                }
                Event::Key { key: Key::V, pressed: false, modifiers, .. } if modifiers.command => {
                    paste_check = true;
                    true
                }
                _ => true,
            })
        });
    }

    if edit_last {
        let me = app.d.me.id.clone();
        let last = if let Some(root) = key.strip_prefix("t:") {
            app.d.thread.as_ref().and_then(|t| t.posts.iter().rev().find(|p| p.user_id == me && !p.pending && p.root_id == root).cloned())
        } else {
            app.d.views.get(&channel_id).and_then(|v| v.posts.iter().rev().find(|p| p.user_id == me && !p.pending).cloned())
        };
        if let Some(p) = last {
            app.u.editing = Some((p.id.clone(), p.message.clone()));
        }
    }
    #[cfg(target_os = "macos")]
    if has_focus && crate::macos::take_image_paste() {
        paste_check = true;
    }
    if paste_check
        && let Some(png) = clipboard_image()
    {
        let name = format!("pasted-image-{}.png", crate::api::now_ms() / 1000);
        app.actions.push(Action::Upload { key: key.to_string(), name, data: UploadData::Bytes(png) });
    }

    let placeholder = match key.strip_prefix("t:") {
        Some(_) => "Reply…".to_string(),
        None => match app.d.channels.get(&channel_id) {
            Some(c) if c.is_direct() => format!("Write to {}", app.d.channel_name(c)),
            Some(c) => format!("Write to {}", app.d.channel_name(c)),
            None => "Write a message".into(),
        },
    };
    let focus_now = std::mem::take(&mut app.u.focus_composer);
    let App { u, actions, .. } = app;
    let composer = u.composers.entry(key.to_string()).or_default();

    if let Some(c) = &pick {
        if let Some((_, prefix)) = &tok {
            let cut = composer.text.len() - prefix.len() - 1;
            composer.text.truncate(cut);
            composer.text.push_str(&c.insert());
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                let end = CCursor::new(composer.text.chars().count());
                state.cursor.set_char_range(Some(CCursorRange::one(end)));
                state.store(ui.ctx(), id);
            }
        }
        u.ac_selected = 0;
    }

    let border = if has_focus { t.accent.gamma_multiply(0.7) } else { t.border_strong };
    let frame = Frame::new().fill(t.bg).stroke(Stroke::new(1.0, border)).corner_radius(10).inner_margin(Margin::symmetric(10, 6));
    let resp = frame.show(ui, |ui| {
        if !composer.uploads.is_empty() {
            ui.horizontal_wrapped(|ui| {
                let mut remove = None;
                for (i, up) in composer.uploads.iter().enumerate() {
                    Frame::new().fill(t.reaction_bg).corner_radius(6).inner_margin(Margin::symmetric(8, 4)).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            match &up.done {
                                None => super::spinner(ui, t.text_dim),
                                Some(Ok(_)) => {
                                    ui.add(Icon::File.image(t.text_dim, 14.0));
                                }
                                Some(Err(_)) => {
                                    ui.add(Icon::Alert.image(t.error, 14.0));
                                }
                            }
                            ui.label(RichText::new(&up.name).size(12.5));
                            if ui.add(egui::Button::image(Icon::Close.image(t.text_dim, 12.0)).frame(false)).clicked() {
                                remove = Some(i);
                            }
                        });
                    });
                }
                if let Some(i) = remove {
                    composer.uploads.remove(i);
                }
            });
            ui.add_space(4.0);
        }
        let edit = egui::TextEdit::multiline(&mut composer.text)
            .id(id)
            .hint_text(placeholder)
            .frame(egui::Frame::NONE)
            .desired_rows(1)
            .desired_width(f32::INFINITY)
            .lock_focus(true)
            .margin(vec2(2.0, 6.0));
        let resp = ui.add(edit);
        if focus_now {
            resp.request_focus();
        }
        if resp.changed() && !composer.text.is_empty() {
            actions.push(Action::Typing(key.to_string()));
        }
        ui.horizontal(|ui| {
            if icon_button(ui, Icon::Attach, t.text_dim, 17.0, "Attach files").clicked() {
                actions.push(Action::Attach(key.to_string()));
            }
            if ui.available_width() > 260.0 {
                ui.label(RichText::new("Shift+Enter for a new line").size(11.0).color(t.text_dim.gamma_multiply(0.8)));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let ready = !composer.text.trim().is_empty() || composer.uploads.iter().any(|u| matches!(u.done, Some(Ok(_))));
                let color = if ready { t.accent_text } else { t.text_dim };
                let btn = egui::Button::image(Icon::Send.image(color, 15.0))
                    .fill(if ready { t.accent } else { t.reaction_bg })
                    .corner_radius(6)
                    .min_size(vec2(36.0, 28.0));
                if ui.add_enabled(ready, btn).on_hover_text("Send").clicked() {
                    send = true;
                }
            });
        });
    });

    // Completion list above the box.
    if !comps.is_empty() {
        let rect = resp.response.rect;
        let height = comps.len() as f32 * 30.0 + 12.0;
        egui::Area::new(egui::Id::new(("ac", key)))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.left_top() - vec2(0.0, height + 4.0))
            .show(ui.ctx(), |ui| {
                Frame::popup(ui.style()).fill(t.bg).inner_margin(Margin::same(6)).show(ui, |ui| {
                    ui.set_width(rect.width().min(420.0));
                    for (i, c) in comps.iter().enumerate() {
                        let selected = i == u.ac_selected;
                        let label = match c {
                            Completion::User(user) => format!("@{}   {}", user.username, user.display_name()),
                            Completion::Channel(n, dn) => format!("~{n}   {dn}"),
                            Completion::Emoji(code, glyph) => format!("{glyph}  :{code}:"),
                        };
                        let text = RichText::new(label).size(13.5);
                        let r = ui.add_sized(vec2(ui.available_width(), 28.0), egui::Button::selectable(selected, text));
                        if r.clicked() {
                            u.ac_selected = i;
                        }
                    }
                });
            });
        let _ = semibold;
    }

    if send {
        actions.push(Action::Send(key.to_string()));
    }
}

/// An image on the clipboard (and no text), encoded as PNG.
fn clipboard_image() -> Option<Vec<u8>> {
    let mut clip = arboard::Clipboard::new().ok()?;
    if clip.get_text().is_ok_and(|t| !t.is_empty()) {
        return None;
    }
    let img = clip.get_image().ok()?;
    let buf = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())?;
    let mut out = std::io::Cursor::new(Vec::new());
    buf.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::token;

    #[test]
    fn completes_the_last_token_only() {
        assert_eq!(token("hi @al"), Some(('@', "al".into())));
        assert_eq!(token("see ~tt"), Some(('~', "tt".into())));
        assert_eq!(token("nice :ta"), Some((':', "ta".into())));
        assert_eq!(token("time 10:3"), None);
        assert_eq!(token("done :tada: "), None);
        assert_eq!(token("plain words"), None);
    }
}
