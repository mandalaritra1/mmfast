//! Sign in: password (with MFA), or a token for SSO servers.

use super::semibold;
use crate::app::{Action, App};
use egui::{Frame, Margin, RichText, Ui, vec2};

pub fn show(app: &mut App, ui: &mut Ui) {
    let t = app.d.theme.clone();
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, t.panel_bg);
    let busy = app.u.login.busy;
    let mut submit = false;
    let mut sso_start = false;
    let mut sso_again = false;
    let mut sso_cancel = false;
    egui::Area::new(egui::Id::new("login"))
        .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, -20.0))
        .show(ui.ctx(), |ui| {
            Frame::new()
                .fill(t.bg)
                .stroke(egui::Stroke::new(1.0, t.border))
                .corner_radius(12)
                .inner_margin(Margin::same(28))
                .show(ui, |ui| {
                    ui.set_width(380.0);
                    let f = &mut app.u.login;
                    ui.label(RichText::new("mmfast").font(semibold(26.0)).color(t.text));
                    ui.label(RichText::new("A fast, native client for Mattermost").color(t.text_dim));
                    ui.add_space(18.0);
                    ui.label(RichText::new("Server").font(semibold(12.5)).color(t.text_dim));
                    if f.server == "https://" {
                        f.server = "https://mattermost.web.cern.ch".into();
                    }
                    ui.add(egui::TextEdit::singleline(&mut f.server).hint_text("https://mattermost.example.org").desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)));
                    ui.add_space(12.0);
                    if let Some(since) = f.sso_since {
                        Frame::new().fill(t.reaction_mine).corner_radius(8).inner_margin(Margin::same(12)).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                super::spinner(ui, t.accent);
                                ui.label(RichText::new("Finish signing in in your browser…").font(semibold(13.5)));
                            });
                            ui.label(
                                RichText::new("When your browser asks to open mmfast, allow it.")
                                    .size(12.0)
                                    .color(t.text_dim),
                            );
                            ui.horizontal(|ui| {
                                if ui.small_button("Open the browser again").clicked() {
                                    sso_again = true;
                                }
                                if ui.small_button("Cancel").clicked() {
                                    sso_cancel = true;
                                }
                            });
                        });
                        let _ = since;
                    } else {
                        let btn = egui::Button::new(RichText::new("Sign in with browser (SSO)").color(t.accent_text).font(semibold(14.0)))
                            .fill(t.accent)
                            .min_size(vec2(ui.available_width(), 36.0));
                        if ui.add_enabled(!busy, btn).on_hover_text("CERN and other single sign-on servers").clicked() {
                            sso_start = true;
                        }
                    }
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("or sign in with").size(12.0).color(t.text_dim));
                        ui.selectable_value(&mut f.use_token, false, "Password");
                        ui.selectable_value(&mut f.use_token, true, "Token");
                    });
                    ui.add_space(6.0);
                    let enter = |r: &egui::Response| r.lost_focus() && r.ctx.input(|i| i.key_pressed(egui::Key::Enter));
                    if f.use_token {
                        ui.label(RichText::new("Access token").font(semibold(12.5)).color(t.text_dim));
                        let r = ui.add(egui::TextEdit::singleline(&mut f.token).password(true).desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)));
                        submit |= enter(&r);
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "A personal access token (Profile → Security), or the MMAUTHTOKEN cookie \
                                 of a browser session.",
                            )
                            .size(12.0)
                            .color(t.text_dim),
                        );
                    } else {
                        ui.label(RichText::new("Email or username").font(semibold(12.5)).color(t.text_dim));
                        ui.add(egui::TextEdit::singleline(&mut f.login_id).desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)));
                        ui.add_space(6.0);
                        ui.label(RichText::new("Password").font(semibold(12.5)).color(t.text_dim));
                        let r = ui.add(egui::TextEdit::singleline(&mut f.password).password(true).desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)));
                        submit |= enter(&r);
                        ui.add_space(6.0);
                        ui.label(RichText::new("MFA code (if enabled)").font(semibold(12.5)).color(t.text_dim));
                        let r = ui.add(egui::TextEdit::singleline(&mut f.mfa).desired_width(f32::INFINITY).margin(vec2(8.0, 6.0)));
                        submit |= enter(&r);
                    }
                    if let Some(err) = &f.error {
                        ui.add_space(8.0);
                        ui.label(RichText::new(err).color(t.error).size(13.0));
                    }
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        let label = if busy { "Signing in…" } else { "Sign in" };
                        let btn = egui::Button::new(RichText::new(label).color(t.accent_text).font(semibold(14.0)))
                            .fill(t.accent)
                            .min_size(vec2(120.0, 34.0));
                        if ui.add_enabled(!busy, btn).clicked() {
                            submit = true;
                        }
                        if busy {
                            super::spinner(ui, t.text_dim);
                        }
                    });
                });
        });
    if submit && !busy {
        app.actions.push(Action::Login);
    }
    if sso_start || sso_again {
        app.actions.push(Action::StartSso);
    }
    if sso_cancel {
        app.actions.push(Action::CancelSso);
    }
}
