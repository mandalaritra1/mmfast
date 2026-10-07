//! Followed threads (collapsed reply threads), newest activity first.

use super::semibold;
use crate::app::{Action, App};
use crate::markup;
use egui::{Frame, Margin, RichText, Sense, Stroke, Ui, vec2};

pub fn show(app: &mut App, ui: &mut Ui) {
    let App { d, u, actions, .. } = app;
    let t = d.theme.clone();
    egui::Panel::top("threads-title")
        .exact_size(54.0)
        .frame(Frame::new().fill(t.bg).inner_margin(Margin::symmetric(18, 0)))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.add(super::Icon::Threads.image(t.text, 17.0));
                ui.label(RichText::new("Followed threads").font(semibold(16.5)).color(t.text));
            });
        });
    egui::CentralPanel::default().frame(Frame::new().fill(t.bg)).show(ui, |ui| {
        let Some(threads) = &d.threads else {
            ui.add_space(30.0);
            ui.vertical_centered(|ui| super::spinner(ui, t.text_dim));
            return;
        };
        if threads.threads.is_empty() {
            super::centered_note(ui, "You are not following any threads yet. Reply to a message to follow its thread.", t.text_dim);
            return;
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(8.0);
            for th in &threads.threads {
                let post = &th.post;
                let channel = d.channels.get(&post.channel_id).map(|c| d.channel_name(c)).unwrap_or_default();
                let unread = th.unread_replies > 0;
                let resp = Frame::new()
                    .inner_margin(Margin::symmetric(18, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            if unread {
                                let (r, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                                ui.painter().circle_filled(r.center(), 4.0, t.accent);
                            }
                            ui.label(RichText::new(&channel).font(semibold(12.0)).color(t.text_dim));
                            ui.label(RichText::new(super::relative(th.last_reply_at)).size(12.0).color(t.text_dim));
                            if th.unread_mentions > 0 {
                                ui.label(RichText::new(format!("@{}", th.unread_mentions)).font(semibold(12.0)).color(t.badge));
                            }
                        });
                        ui.horizontal(|ui| {
                            super::avatar(ui, &mut u.images, d, &post.user_id, 22.0);
                            let name = d.user_name(&post.user_id);
                            ui.label(RichText::new(name).font(semibold(14.0)).color(t.text));
                        });
                        let preview = markup::plain(&post.message);
                        let g = super::elided(
                            ui,
                            if preview.is_empty() { "Attachment" } else { &preview },
                            egui::FontId::proportional(14.0),
                            if unread { t.text } else { t.text_dim },
                            ui.available_width(),
                        );
                        ui.label(g);
                        ui.horizontal(|ui| {
                            for p in th.participants.iter().take(5) {
                                super::avatar(ui, &mut u.images, d, &p.id, 18.0);
                            }
                            let n = th.reply_count;
                            let text = if unread {
                                format!("{n} replies · {} new", th.unread_replies)
                            } else {
                                format!("{n} {}", if n == 1 { "reply" } else { "replies" })
                            };
                            ui.label(RichText::new(text).font(semibold(12.5)).color(t.link));
                        });
                    })
                    .response;
                let resp = ui.interact(resp.rect, ui.id().with(("thread-card", &th.id)), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(resp.rect, 0.0, t.hover_bg.gamma_multiply(0.6));
                }
                ui.painter().hline(resp.rect.x_range(), resp.rect.bottom(), Stroke::new(1.0, t.border));
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    actions.push(Action::OpenThread(th.id.clone()));
                    if unread {
                        actions.push(Action::MarkThreadRead(th.id.clone()));
                    }
                }
            }
        });
    });
}
