//! Views. They read `Data`, keep widget state in `UiState`, and push
//! `Action`s for `App` to apply after the frame.

pub mod composer;
pub mod header;
pub mod list;
pub mod login;
pub mod overlays;
pub mod post;
pub mod rhs;
pub mod sidebar;
pub mod threads;

use crate::app::{App, Center, Data, Rhs};
use crate::images::Images;
use crate::model::ImageKey;
use egui::text::LayoutJob;
use egui::{Align, Color32, FontId, Frame, Galley, Layout, Margin, Response, Sense, Stroke, Ui, vec2};
use std::sync::Arc;

fastframe_icons::icons! {
    /// Every icon the interface draws.
    pub enum Icon {
        prefix: "mmfast-icon-",
        directory: "../../assets/icons/",
        Hash => "hash",
        Threads => "messages-square",
        React => "smile-plus",
        Attach => "paperclip",
        Send => "send",
        Download => "download",
        File => "file",
        Reply => "reply",
        Image => "image",
        Offline => "wifi-off",
        Star => "star",
        StarFilled => "star-filled",
        Bell => "bell",
        BellOff => "bell-off",
        ChevronDown => lucide "chevron-down",
        Plus => lucide "plus",
        Lock => lucide "lock",
        Search => lucide "search",
        Pin => lucide "pin",
        Users => lucide "users",
        More => lucide "ellipsis",
        Close => lucide "x",
        Edit => lucide "pencil",
        Trash => lucide "trash-2",
        Copy => lucide "copy",
        External => lucide "external-link",
        Settings => lucide "settings",
        Moon => lucide "moon",
        Sun => lucide "sun",
        Monitor => lucide "monitor",
        Logout => lucide "log-out",
        ChevronLeft => lucide "chevron-left",
        ChevronRight => lucide "chevron-right",
        Info => lucide "info",
        Alert => lucide "circle-alert",
    }
}

pub fn install_fonts(ctx: &egui::Context) {
    #[cfg(target_os = "macos")]
    {
        crate::macos::install(ctx);
        crate::macos::links::set_repaint(ctx);
    }
    egui_extras::install_image_loaders(ctx);
    fastframe_icons::install::<Icon>(ctx);
    let mut fonts = fastframe_fonts::FontSetup::default()
        .primary(fastframe_fonts::Primary::System)
        .weights(&[fastframe_fonts::Weight::Medium, fastframe_fonts::Weight::SemiBold, fastframe_fonts::Weight::Bold])
        .definitions();
    // Math: STIX Two Math where the system has it, then the interface faces.
    let mut math_family = Vec::new();
    if let Some(path) = crate::math::font_path()
        && let Ok(bytes) = std::fs::read(path)
    {
        fonts.font_data.insert("stix-math".into(), Arc::new(egui::FontData::from_owned(bytes)));
        math_family.push("stix-math".to_string());
    }
    math_family.extend(fonts.families.get(&egui::FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(crate::math::family(), math_family);
    let rendering = fastframe_text::detect();
    rendering.apply_to(&mut fonts);
    ctx.set_fonts(fonts);
    fastframe_emoji::EmojiSetup::default().install();
    std::thread::spawn(fastframe_emoji::warm_up);
    ctx.add_plugin(fastframe_emoji::EmojiPlugin::default());
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = vec2(6.0, 4.0);
        s.spacing.button_padding = vec2(8.0, 4.0);
        s.interaction.selectable_labels = false;
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.0));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(11.5));
        s.text_styles.insert(egui::TextStyle::Monospace, FontId::monospace(13.0));
    });
}

pub fn semibold(size: f32) -> FontId {
    fastframe_fonts::Weight::SemiBold.font_id(size)
}

pub fn medium(size: f32) -> FontId {
    fastframe_fonts::Weight::Medium.font_id(size)
}

pub fn show(app: &mut App, ui: &mut Ui) {
    let ctx = ui.ctx().clone();
    if !app.d.logged_in {
        login::show(app, ui);
        overlays::toasts(app, &ctx);
        return;
    }
    overlays::shortcuts(app, &ctx);
    let t = app.d.theme.clone();
    if app.d.teams.len() > 1 {
        egui::Panel::left("teams")
            .exact_size(60.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(t.team_bar_bg).inner_margin(Margin::symmetric(10, 12)))
            .show(ui, |ui| sidebar::teams(app, ui));
    }
    egui::Panel::left("sidebar")
        .default_size(250.0)
        .size_range(200.0..=420.0)
        .resizable(true)
        .show_separator_line(false)
        .frame(Frame::new().fill(t.sidebar_bg))
        .show(ui, |ui| {
            let r = ui.max_rect();
            ui.painter().vline(r.right() - 0.5, r.y_range(), Stroke::new(1.0, if t.dark { t.border } else { t.sidebar_bg }));
            sidebar::show(app, ui)
        });
    if app.d.rhs != Rhs::None {
        egui::Panel::right("rhs")
            .default_size(430.0)
            .size_range(320.0..=760.0)
            .resizable(true)
            .frame(Frame::new().fill(t.bg))
            .show(ui, |ui| rhs::show(app, ui));
    }
    egui::CentralPanel::default().frame(Frame::new().fill(t.bg)).show(ui, |ui| match app.d.center {
        Center::Channel => channel_view(app, ui),
        Center::Threads => threads::show(app, ui),
    });
    overlays::show(app, &ctx);
}

fn channel_view(app: &mut App, ui: &mut Ui) {
    let t = app.d.theme.clone();
    if app.d.current.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label(egui::RichText::new(if app.d.have_data { "No channels" } else { "Loading…" }).color(t.text_dim));
        });
        return;
    }
    egui::Panel::top("header")
        .exact_size(54.0)
        .frame(Frame::new().fill(t.bg).inner_margin(Margin::symmetric(18, 0)))
        .show(ui, |ui| header::show(app, ui));
    let key = format!("c:{}", app.d.current);
    egui::Panel::bottom("composer")
        .frame(Frame::new().fill(t.bg).inner_margin(Margin { left: 18, right: 18, top: 2, bottom: 14 }))
        .show_separator_line(false)
        .show(ui, |ui| composer::show(app, ui, &key));
    egui::CentralPanel::default().frame(Frame::new().fill(t.bg)).show(ui, |ui| post::channel_list(app, ui));
}

// ------------------------------------------------------------------ shared widgets

/// A circular avatar: the user's picture, or their initial on their colour.
pub fn avatar(ui: &mut Ui, images: &mut Images, d: &Data, user_id: &str, size: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    paint_avatar(ui, images, d, user_id, rect);
    resp
}

pub fn paint_avatar(ui: &Ui, images: &mut Images, d: &Data, user_id: &str, rect: egui::Rect) {
    let size = rect.width();
    let updated = d.users.get(user_id).map(|u| u.last_picture_update).unwrap_or(0);
    let key = ImageKey::Avatar { user_id: user_id.to_string(), updated };
    if !user_id.is_empty()
        && let Some(tex) = images.get(ui.ctx(), &key)
    {
        egui::Image::new(&tex).corner_radius(size / 2.0).paint_at(ui, rect);
        return;
    }
    let color = d.theme.user_color(user_id);
    ui.painter().circle_filled(rect.center(), size / 2.0, color);
    let initial = d.users.get(user_id).and_then(|u| u.display_name().chars().next()).unwrap_or('?').to_uppercase().to_string();
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, initial, semibold(size * 0.45), Color32::WHITE);
}

pub fn icon_button(ui: &mut Ui, icon: Icon, color: Color32, size: f32, tip: &str) -> Response {
    let btn = egui::Button::image(icon.image(color, size)).frame(false).min_size(vec2(size + 10.0, size + 10.0));
    let resp = ui.add(btn);
    if !tip.is_empty() { resp.on_hover_text(tip) } else { resp }
}

/// A one-line galley that ends in an ellipsis when it does not fit.
pub fn elided(ui: &Ui, text: &str, font: FontId, color: Color32, max_width: f32) -> Arc<Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap.max_width = max_width.max(10.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    ui.fonts_mut(|f| f.layout_job(job))
}

pub fn badge(ui: &Ui, right_center: egui::Pos2, count: i64, fill: Color32) -> egui::Rect {
    let text = if count > 99 { "99+".to_string() } else { count.to_string() };
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(text, semibold(11.0), Color32::WHITE));
    let w = (galley.size().x + 12.0).max(20.0);
    let rect = egui::Rect::from_center_size(right_center - vec2(w / 2.0, 0.0), vec2(w, 18.0));
    ui.painter().rect_filled(rect, 9.0, fill);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, Color32::WHITE);
    rect
}

pub fn section_label(ui: &mut Ui, text: &str, color: Color32) {
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.add_space(16.0);
        ui.label(egui::RichText::new(text).font(semibold(11.5)).color(color));
    });
    ui.add_space(2.0);
}

pub fn spinner(ui: &mut Ui, color: Color32) {
    ui.add(egui::Spinner::new().size(16.0).color(color));
}

pub fn centered_note(ui: &mut Ui, text: &str, color: Color32) {
    ui.with_layout(Layout::top_down(Align::Center), |ui| {
        ui.add_space(40.0);
        ui.label(egui::RichText::new(text).color(color));
    });
}


// ------------------------------------------------------------------ time

fn zoned(ms: i64) -> Option<jiff::Zoned> {
    let ts = jiff::Timestamp::from_millisecond(ms).ok()?;
    Some(ts.to_zoned(jiff::tz::TimeZone::system()))
}

pub fn time_hm(ms: i64) -> String {
    zoned(ms).map(|z| z.strftime("%H:%M").to_string()).unwrap_or_default()
}

pub fn full_time(ms: i64) -> String {
    zoned(ms).map(|z| z.strftime("%A, %B %-d, %Y at %H:%M").to_string()).unwrap_or_default()
}

pub fn day_number(ms: i64) -> i64 {
    zoned(ms).map(|z| z.date().since(jiff::civil::date(2000, 1, 1)).map(|s| s.get_days() as i64).unwrap_or(0)).unwrap_or(0)
}

pub fn day_label(ms: i64) -> String {
    let today = day_number(crate::api::now_ms());
    let day = day_number(ms);
    if day == today {
        "Today".into()
    } else if day == today - 1 {
        "Yesterday".into()
    } else if let Some(z) = zoned(ms) {
        if today - day < 6 {
            z.strftime("%A").to_string()
        } else if z.year() == zoned(crate::api::now_ms()).map(|n| n.year()).unwrap_or(0) {
            z.strftime("%a, %b %-d").to_string()
        } else {
            z.strftime("%b %-d, %Y").to_string()
        }
    } else {
        String::new()
    }
}

pub fn relative(ms: i64) -> String {
    let secs = (crate::api::now_ms() - ms) / 1000;
    match secs {
        s if s < 60 => "just now".into(),
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86_400 => format!("{} h ago", s / 3600),
        _ => format!("{} {}", day_label(ms), time_hm(ms)),
    }
}
