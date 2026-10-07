//! GPU textures for avatars and attachments.
//!
//! The backend fetches, caches on disk and decodes off the UI thread; this
//! side only uploads the pixels and draws. Previews and full images that
//! have not been drawn for a while are dropped once more than `RESIDENT`
//! are held, so scrolling through a photo-heavy channel does not keep every
//! picture in GPU memory (ZapFast's image_cache does the same).

use crate::model::ImageKey;
use egui::{ColorImage, TextureHandle, TextureOptions};
use std::collections::HashMap;
use std::sync::Arc;

const RESIDENT: usize = 48;

#[derive(Default)]
struct Entry {
    texture: Option<TextureHandle>,
    pending: Option<Arc<ColorImage>>,
    requested: bool,
    failed: bool,
    last_frame: u64,
}

#[derive(Default)]
struct Animation {
    frames: Vec<(TextureHandle, f32)>,
    pending: Option<Vec<(Arc<ColorImage>, f32)>>,
    requested: bool,
    failed: bool,
    last_frame: u64,
    started: Option<f64>,
}

#[derive(Default)]
pub struct Images {
    entries: HashMap<ImageKey, Entry>,
    animations: HashMap<ImageKey, Animation>,
    requests: Vec<ImageKey>,
    frame: u64,
}

impl Images {
    pub fn begin_frame(&mut self, frame: u64) {
        self.frame = frame;
    }

    /// The texture for `key`, asking the backend for it the first time.
    pub fn get(&mut self, ctx: &egui::Context, key: &ImageKey) -> Option<TextureHandle> {
        let frame = self.frame;
        let entry = self.entries.entry(key.clone()).or_default();
        entry.last_frame = frame;
        if let Some(image) = entry.pending.take() {
            let name = key.cache_name();
            entry.texture = Some(ctx.load_texture(name, image, TextureOptions::LINEAR));
        }
        if entry.texture.is_none() && !entry.requested && !entry.failed {
            entry.requested = true;
            self.requests.push(key.clone());
        }
        entry.texture.clone()
    }

    pub fn failed(&self, key: &ImageKey) -> bool {
        self.entries.get(key).is_some_and(|e| e.failed) || self.animations.get(key).is_some_and(|a| a.failed)
    }

    /// The frame of an animation due at `now`, and asks for a repaint when
    /// the next one is due: an animation only costs CPU while it is drawn.
    pub fn frame(&mut self, ctx: &egui::Context, key: &ImageKey, now: f64) -> Option<TextureHandle> {
        let frame_nr = self.frame;
        let anim = self.animations.entry(key.clone()).or_default();
        anim.last_frame = frame_nr;
        if let Some(frames) = anim.pending.take() {
            let name = key.cache_name();
            anim.frames = frames
                .into_iter()
                .enumerate()
                .map(|(i, (img, d))| (ctx.load_texture(format!("{name}#{i}"), img, TextureOptions::LINEAR), d))
                .collect();
        }
        if anim.frames.is_empty() {
            if !anim.requested && !anim.failed {
                anim.requested = true;
                self.requests.push(key.clone());
            }
            return None;
        }
        let total: f32 = anim.frames.iter().map(|(_, d)| d).sum();
        let start = *anim.started.get_or_insert(now);
        if anim.frames.len() == 1 || total <= 0.0 {
            return Some(anim.frames[0].0.clone());
        }
        let mut t = ((now - start) as f32) % total;
        for (tex, d) in &anim.frames {
            if t < *d {
                ctx.request_repaint_after(std::time::Duration::from_secs_f32(d - t));
                return Some(tex.clone());
            }
            t -= d;
        }
        Some(anim.frames[0].0.clone())
    }

    pub fn deliver_animation(&mut self, key: ImageKey, frames: Option<Vec<(Arc<ColorImage>, f32)>>) {
        let anim = self.animations.entry(key).or_default();
        anim.requested = false;
        match frames {
            Some(f) => anim.pending = Some(f),
            None => anim.failed = true,
        }
    }

    pub fn deliver(&mut self, key: ImageKey, image: Option<Arc<ColorImage>>) {
        let entry = self.entries.entry(key).or_default();
        entry.requested = false;
        match image {
            Some(img) => entry.pending = Some(img),
            None => entry.failed = true,
        }
    }

    pub fn take_requests(&mut self) -> Vec<ImageKey> {
        std::mem::take(&mut self.requests)
    }

    /// Drop textures of attachments not drawn recently, keeping `RESIDENT`.
    pub fn sweep(&mut self) {
        let mut big: Vec<(u64, ImageKey)> = self
            .entries
            .iter()
            .filter(|(k, e)| e.texture.is_some() && !matches!(k, ImageKey::Avatar { .. } | ImageKey::TeamIcon { .. } | ImageKey::Emoji(_)))
            .map(|(k, e)| (e.last_frame, k.clone()))
            .collect();
        // Animations are heavy (every frame a texture): keep the visible few.
        let now = self.frame;
        self.animations.retain(|_, a| a.last_frame + 120 > now || a.requested);
        if big.len() <= RESIDENT {
            return;
        }
        big.sort_by_key(|(f, _)| std::cmp::Reverse(*f));
        for (frame, key) in big.into_iter().skip(RESIDENT) {
            if frame + 2 < self.frame {
                self.entries.remove(&key);
            }
        }
    }
}
