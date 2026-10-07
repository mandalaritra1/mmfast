//! Colours for light and dark, and the egui visuals that follow them.

use egui::{Color32, CornerRadius, Stroke, Visuals};

#[derive(Clone, Debug)]
pub struct Theme {
    pub dark: bool,
    pub sidebar_bg: Color32,
    pub team_bar_bg: Color32,
    pub sidebar_text: Color32,
    pub sidebar_dim: Color32,
    pub sidebar_active: Color32,
    pub sidebar_hover: Color32,
    pub bg: Color32,
    pub panel_bg: Color32,
    pub hover_bg: Color32,
    pub highlight_bg: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub border: Color32,
    pub border_strong: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub link: Color32,
    pub mention_bg: Color32,
    pub mention_text: Color32,
    pub badge: Color32,
    pub code_bg: Color32,
    pub code_block_bg: Color32,
    pub code_text: Color32,
    pub math: Color32,
    pub unread_line: Color32,
    pub online: Color32,
    pub error: Color32,
    pub reaction_bg: Color32,
    pub reaction_mine: Color32,
}

impl Theme {
    pub fn light() -> Self {
        Self {
            dark: false,
            sidebar_bg: Color32::from_rgb(0x1f, 0x26, 0x33),
            team_bar_bg: Color32::from_rgb(0x17, 0x1c, 0x26),
            sidebar_text: Color32::from_rgb(0xe8, 0xec, 0xf2),
            sidebar_dim: Color32::from_rgb(0x96, 0xa0, 0xb0),
            sidebar_active: Color32::from_rgb(0x2f, 0x5f, 0xd0),
            sidebar_hover: Color32::from_rgb(0x2a, 0x33, 0x44),
            bg: Color32::WHITE,
            panel_bg: Color32::from_rgb(0xf7, 0xf8, 0xfa),
            hover_bg: Color32::from_rgb(0xf4, 0xf5, 0xf7),
            highlight_bg: Color32::from_rgb(0xff, 0xf6, 0xd6),
            text: Color32::from_rgb(0x1d, 0x21, 0x28),
            text_dim: Color32::from_rgb(0x6b, 0x72, 0x80),
            border: Color32::from_rgb(0xe3, 0xe6, 0xeb),
            border_strong: Color32::from_rgb(0xc5, 0xcb, 0xd5),
            accent: Color32::from_rgb(0x2f, 0x6f, 0xeb),
            accent_text: Color32::WHITE,
            link: Color32::from_rgb(0x1f, 0x60, 0xd8),
            mention_bg: Color32::from_rgb(0xff, 0xe5, 0x99),
            mention_text: Color32::from_rgb(0x3d, 0x2c, 0x00),
            badge: Color32::from_rgb(0xe0, 0x3e, 0x3e),
            code_bg: Color32::from_rgb(0xee, 0xf0, 0xf3),
            code_block_bg: Color32::from_rgb(0xf6, 0xf7, 0xf9),
            code_text: Color32::from_rgb(0xb4, 0x2a, 0x55),
            math: Color32::from_rgb(0x2b, 0x4c, 0x9e),
            unread_line: Color32::from_rgb(0xe0, 0x3e, 0x3e),
            online: Color32::from_rgb(0x2e, 0xb8, 0x6b),
            error: Color32::from_rgb(0xd2, 0x3b, 0x3b),
            reaction_bg: Color32::from_rgb(0xf0, 0xf2, 0xf5),
            reaction_mine: Color32::from_rgb(0xe0, 0xeb, 0xff),
        }
    }

    pub fn dark() -> Self {
        Self {
            dark: true,
            sidebar_bg: Color32::from_rgb(0x16, 0x19, 0x20),
            team_bar_bg: Color32::from_rgb(0x10, 0x12, 0x17),
            sidebar_text: Color32::from_rgb(0xe3, 0xe6, 0xeb),
            sidebar_dim: Color32::from_rgb(0x8a, 0x92, 0xa0),
            sidebar_active: Color32::from_rgb(0x2f, 0x5f, 0xd0),
            sidebar_hover: Color32::from_rgb(0x23, 0x28, 0x32),
            bg: Color32::from_rgb(0x1e, 0x21, 0x28),
            panel_bg: Color32::from_rgb(0x1a, 0x1d, 0x23),
            hover_bg: Color32::from_rgb(0x25, 0x29, 0x31),
            highlight_bg: Color32::from_rgb(0x3a, 0x34, 0x1c),
            text: Color32::from_rgb(0xe3, 0xe6, 0xeb),
            text_dim: Color32::from_rgb(0x95, 0x9d, 0xab),
            border: Color32::from_rgb(0x2e, 0x33, 0x3d),
            border_strong: Color32::from_rgb(0x48, 0x50, 0x5e),
            accent: Color32::from_rgb(0x4c, 0x8d, 0xff),
            accent_text: Color32::WHITE,
            link: Color32::from_rgb(0x6a, 0xa4, 0xff),
            mention_bg: Color32::from_rgb(0x5c, 0x4a, 0x12),
            mention_text: Color32::from_rgb(0xff, 0xe7, 0xa3),
            badge: Color32::from_rgb(0xe0, 0x4a, 0x4a),
            code_bg: Color32::from_rgb(0x2a, 0x2e, 0x37),
            code_block_bg: Color32::from_rgb(0x23, 0x27, 0x2f),
            code_text: Color32::from_rgb(0xf0, 0x8d, 0xb0),
            math: Color32::from_rgb(0x9d, 0xbc, 0xff),
            unread_line: Color32::from_rgb(0xe0, 0x4a, 0x4a),
            online: Color32::from_rgb(0x3c, 0xc8, 0x7a),
            error: Color32::from_rgb(0xf0, 0x5c, 0x5c),
            reaction_bg: Color32::from_rgb(0x2a, 0x2e, 0x37),
            reaction_mine: Color32::from_rgb(0x23, 0x3a, 0x66),
        }
    }

    pub fn visuals(&self) -> Visuals {
        let mut v = if self.dark { Visuals::dark() } else { Visuals::light() };
        v.panel_fill = self.bg;
        v.window_fill = self.bg;
        v.extreme_bg_color = if self.dark { Color32::from_rgb(0x15, 0x17, 0x1c) } else { Color32::WHITE };
        v.faint_bg_color = self.panel_bg;
        v.hyperlink_color = self.link;
        v.override_text_color = Some(self.text);
        v.selection.bg_fill = self.accent.gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0, self.accent);
        v.window_stroke = Stroke::new(1.0, self.border);
        v.window_corner_radius = CornerRadius::same(8);
        v.menu_corner_radius = CornerRadius::same(6);
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        v.widgets.inactive.corner_radius = CornerRadius::same(5);
        v.widgets.hovered.corner_radius = CornerRadius::same(5);
        v.widgets.active.corner_radius = CornerRadius::same(5);
        v.widgets.inactive.weak_bg_fill = self.panel_bg;
        v.widgets.inactive.bg_fill = self.panel_bg;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, self.border);
        // A blinking caret repaints the window twice a second; a solid one
        // lets an idle window cost nothing.
        v.text_cursor.blink = false;
        v
    }

    /// A stable colour per user for avatars without a picture.
    pub fn user_color(&self, id: &str) -> Color32 {
        const PALETTE: [Color32; 8] = [
            Color32::from_rgb(0x3b, 0x82, 0xf6),
            Color32::from_rgb(0x10, 0xb9, 0x81),
            Color32::from_rgb(0xf5, 0x9e, 0x0b),
            Color32::from_rgb(0xef, 0x44, 0x44),
            Color32::from_rgb(0x8b, 0x5c, 0xf6),
            Color32::from_rgb(0x06, 0xb6, 0xd4),
            Color32::from_rgb(0xec, 0x48, 0x99),
            Color32::from_rgb(0x84, 0xcc, 0x16),
        ];
        let h = id.bytes().fold(5381u32, |h, b| h.wrapping_mul(33) ^ b as u32);
        PALETTE[(h % 8) as usize]
    }
}
