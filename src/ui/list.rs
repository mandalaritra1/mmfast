//! A virtualized, bottom-anchored list of variable-height rows.
//!
//! Only rows near the viewport are laid out. Each row's measured height is
//! cached by key (which includes whatever changes its height), unknown rows
//! use an estimate, and the first visible row is held in place across
//! frames, so prepending older history or a row growing never makes the
//! view jump.

use egui::{Rect, ScrollArea, Ui, UiBuilder, vec2};
use std::collections::HashMap;

pub struct Row {
    pub key: String,
    pub estimate: f32,
}

#[derive(Default)]
pub struct VList {
    heights: HashMap<String, f32>,
    width: f32,
    /// First visible row, its offset from the viewport top, and its top.
    anchor: Option<(String, f32, f32)>,
    at_bottom: bool,
    force_bottom: bool,
    reveal_done: Option<String>,
    shown: bool,
}

pub struct ListOutput {
    pub near_top: bool,
}

impl VList {
    pub fn stick_to_bottom(&mut self) {
        self.force_bottom = true;
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        id: impl std::hash::Hash + std::fmt::Debug,
        rows: &[Row],
        reveal: Option<usize>,
        mut draw: impl FnMut(&mut Ui, usize),
    ) -> ListOutput {
        let width = ui.available_width();
        if (width - self.width).abs() > 0.5 {
            // Wrapped text changes height with width.
            self.heights.clear();
            self.width = width;
        }
        if reveal.is_none() {
            // The next jump, even to the same row, is a new request.
            self.reveal_done = None;
        }
        if self.heights.len() > 4 * rows.len().max(500) {
            // Stale revisions of rows: keep what is still listed.
            let live: std::collections::HashSet<&str> = rows.iter().map(|r| r.key.as_str()).collect();
            self.heights.retain(|k, _| live.contains(k.as_str()));
        }
        let heights: Vec<f32> = rows.iter().map(|r| self.heights.get(&r.key).copied().unwrap_or(r.estimate)).collect();
        let mut tops = Vec::with_capacity(rows.len());
        let mut y = 0.0;
        for h in &heights {
            tops.push(y);
            y += h;
        }
        let total = y;

        // Sticking to the bottom is ours, not egui's: egui's version overrides
        // any explicit offset while stuck, which would defeat jumps and the
        // anchor correction.
        let mut area = ScrollArea::vertical().id_salt(id).auto_shrink([false, false]);
        let viewport_h = ui.available_height();
        let mut target: Option<f32> = None;
        if !self.shown {
            self.shown = true;
            self.force_bottom = reveal.is_none();
        }
        if self.force_bottom {
            target = Some((total - viewport_h).max(0.0));
            self.force_bottom = false;
        } else if let Some(i) = reveal {
            let key = rows.get(i).map(|r| r.key.clone());
            if key != self.reveal_done {
                target = Some((tops[i] - viewport_h * 0.3).max(0.0));
                self.reveal_done = key;
            }
        } else if self.at_bottom {
            target = Some((total - viewport_h).max(0.0));
        } else if let Some((key, offset, old_top)) = &self.anchor
            && let Some(i) = rows.iter().position(|r| &r.key == key)
            && (tops[i] - old_top).abs() > 0.5
        {
            // Rows above the anchor changed height or were prepended.
            target = Some((tops[i] - offset).max(0.0));
        }
        if let Some(t) = target {
            area = area.vertical_scroll_offset(t);
        }

        let mut measured_changed = false;
        let output = area.show_viewport(ui, |ui, viewport| {
            ui.set_min_size(vec2(width, total));
            let origin = ui.min_rect().min;
            let margin = 600.0;
            let start = tops.partition_point(|&t| t < viewport.min.y - margin).saturating_sub(1);
            let mut i = start;
            while i < rows.len() && tops[i] < viewport.max.y + margin {
                let rect = Rect::from_min_size(origin + vec2(0.0, tops[i]), vec2(width, f32::INFINITY));
                // A stable identity per row (its id, not its revision or its
                // place among drawn rows) keeps selection and focus with it.
                let stable = rows[i].key.split('|').next().unwrap_or_default();
                let mut child = ui.new_child(
                    UiBuilder::new().id_salt(stable).max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)),
                );
                child.set_width(width);
                draw(&mut child, i);
                let h = child.min_rect().height().max(1.0);
                if (h - heights[i]).abs() > 0.5 {
                    self.heights.insert(rows[i].key.clone(), h);
                    measured_changed = true;
                }
                i += 1;
            }
            // The first row whose bottom is below the viewport top.
            let first = tops.iter().zip(&heights).position(|(t, h)| t + h > viewport.min.y);
            self.anchor = first.map(|i| (rows[i].key.clone(), tops[i] - viewport.min.y, tops[i]));
            viewport
        });
        let viewport = output.inner;
        let content = output.content_size.y;
        self.at_bottom = viewport.max.y >= content - 4.0 || content <= output.inner_rect.height();
        if measured_changed {
            ui.ctx().request_repaint();
        }
        ListOutput { near_top: viewport.min.y < 300.0 }
    }
}
