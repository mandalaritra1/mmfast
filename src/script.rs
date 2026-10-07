//! A scripted run for deterministic checks and screenshots:
//! `--script "channel:ttbar-analysis;wait:800;shot:out.png;quit"`.
//!
//! Steps: `wait:MS`, `channel:NAME`, `dm:USERNAME`, `thread:first|last`,
//! `find:TERMS` (cache), `search:TERMS` (server), `pinned`, `threads`,
//! `lightbox`, `switcher:QUERY`, `header`, `theme:dark|light`, `type:TEXT`,
//! `send`, `attach:PATH`, `react:EMOJI`, `edit:TEXT`, `jump`, `login-dev[:password]`, `sso-wait:SERVER`, `browse`, `profile:USERNAME`, `link:URL`, `join:NAME`, `mute`, `favorite`, `logout`, `size:WxH`, `shot:PATH`, `close`, `quit`.

use crate::app::{Action, App, Switcher};
use crate::backend::UploadData;
use crate::config::ThemePref;

pub struct Script {
    steps: Vec<String>,
    index: usize,
    wait_until: f64,
    shot: Option<String>,
    started: Option<f64>,
}

impl Script {
    pub fn parse(src: &str) -> Self {
        Self {
            steps: src.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
            index: 0,
            wait_until: 0.0,
            shot: None,
            started: None,
        }
    }

    /// Runs the next step when the previous one has settled. True when done.
    pub fn step(&mut self, app: &mut App, ctx: &egui::Context) -> bool {
        let now = ctx.input(|i| i.time);
        let started = *self.started.get_or_insert(now);
        if let Some(path) = &self.shot {
            let shot = ctx.input(|i| {
                i.raw.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            match shot {
                Some(image) => {
                    let rgba: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
                    if let Some(img) = image::RgbaImage::from_raw(image.size[0] as u32, image.size[1] as u32, rgba) {
                        match img.save(path) {
                            Ok(()) => eprintln!("script: saved {path}"),
                            Err(e) => eprintln!("script: could not save {path}: {e}"),
                        }
                    }
                    self.shot = None;
                }
                None => {
                    ctx.request_repaint();
                    return false;
                }
            }
        }
        if now < self.wait_until {
            // Sleep until the wait is over; no polling, so idle measurements hold.
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(self.wait_until - now));
            return false;
        }
        // Wait for the first sync before acting (but not forever).
        if app.d.logged_in && !app.d.synced && now - started < 15.0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            return false;
        }
        let Some(step) = self.steps.get(self.index).cloned() else { return true };
        self.index += 1;
        self.wait_until = now + 0.35;
        ctx.request_repaint();
        let (cmd, arg) = step.split_once(':').map(|(a, b)| (a.to_string(), b.to_string())).unwrap_or((step.clone(), String::new()));
        eprintln!("script: {step}");
        match cmd.as_str() {
            "wait" => self.wait_until = now + arg.parse::<f64>().unwrap_or(500.0) / 1000.0,
            "channel" => {
                let found = app
                    .d
                    .channels
                    .values()
                    .find(|c| c.name == arg || c.display_name.eq_ignore_ascii_case(&arg))
                    .map(|c| c.id.clone());
                match found {
                    Some(id) => app.actions.push(Action::OpenChannel(id)),
                    None => eprintln!("script: no channel {arg}"),
                }
            }
            "dm" => app.actions.push(Action::Link(crate::markup::Link::User(arg))),
            "thread" => {
                let view = app.d.views.get(&app.d.current);
                let mut with_replies = view.map(|v| v.posts.iter().filter(|p| p.reply_count > 0).collect::<Vec<_>>()).unwrap_or_default();
                if arg == "last" {
                    with_replies.reverse();
                }
                match with_replies.first() {
                    Some(p) => app.actions.push(Action::OpenThread(p.id.clone())),
                    None => eprintln!("script: no thread in this channel"),
                }
            }
            "find" | "search" => {
                app.u.search_text = arg;
                app.actions.push(Action::Search { server: cmd == "search" });
            }
            "login-dev" => {
                // Sign in through the login form with the seeded test token.
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("dev/seed-credentials.json");
                let creds: serde_json::Value =
                    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
                app.u.login.server = creds["server"].as_str().unwrap_or_default().to_string();
                app.u.login.token = creds["users"]["aritra"]["access_token"].as_str().unwrap_or_default().to_string();
                app.u.login.use_token = arg != "password";
                if arg == "password" {
                    app.u.login.login_id = "aritra".into();
                    app.u.login.password = creds["users"]["aritra"]["password"].as_str().unwrap_or_default().to_string();
                }
                app.actions.push(Action::Login);
            }
            "logout" => app.actions.push(Action::Logout),
            "sso-wait" => {
                // Pretend single sign-on was started for SERVER, to test the return link.
                app.u.login.server = arg;
                app.u.login.sso_since = Some(now);
            }
            "pinned" => app.actions.push(Action::OpenPinned),
            "react" => {
                let last = app.d.views.get(&app.d.current).and_then(|v| v.posts.iter().rev().find(|p| !p.is_system()).map(|p| p.id.clone()));
                if let Some(post_id) = last {
                    app.actions.push(Action::React { post_id, emoji: arg, add: true });
                }
            }
            "edit" => {
                let me = app.d.me.id.clone();
                let last = app.d.views.get(&app.d.current).and_then(|v| v.posts.iter().rev().find(|p| p.user_id == me && !p.pending).map(|p| p.id.clone()));
                if let Some(post_id) = last {
                    app.actions.push(Action::SaveEdit { post_id, text: arg });
                }
            }
            "jump" => {
                let first = app.d.search.server.as_ref().and_then(|r| r.first().cloned()).or_else(|| app.d.search.local.first().cloned());
                match first {
                    Some(p) => app.actions.push(Action::JumpTo(p)),
                    None => eprintln!("script: no search result to jump to"),
                }
            }
            "threads" => app.actions.push(Action::ShowThreads),
            "close" => {
                app.actions.push(Action::CloseRhs);
                app.u.lightbox = None;
                app.u.switcher = None;
                app.u.header_popup = false;
                app.u.profile = None;
                app.d.browse = None;
            }
            "lightbox" => {
                let files = app.d.views.get(&app.d.current).and_then(|v| {
                    v.posts.iter().rev().find(|p| p.metadata.files.iter().any(|f| f.is_image())).map(|p| p.metadata.files.iter().filter(|f| f.is_image()).cloned().collect::<Vec<_>>())
                });
                if let Some(files) = files {
                    app.actions.push(Action::OpenLightbox { files, index: 0 });
                }
            }
            "switcher" => app.u.switcher = Some(Switcher { query: arg, ..Default::default() }),
            "browse" => app.actions.push(Action::OpenBrowse),
            "profile" => {
                let user = app.d.by_name.get(&arg).cloned();
                if let Some(id) = user {
                    app.u.profile = Some((id, egui::pos2(420.0, 200.0)));
                }
            }
            "link" => app.actions.push(Action::Link(crate::markup::Link::Url(arg))),
            "join" => {
                let id = app.d.browse.as_ref().and_then(|b| b.results.iter().find(|c| c.name == arg).map(|c| c.id.clone()));
                match id {
                    Some(id) => app.actions.push(Action::Join(id)),
                    None => eprintln!("script: {arg} is not in the browse list"),
                }
            }
            "mute" => app.actions.push(Action::ToggleMute(app.d.current.clone())),
            "favorite" => app.actions.push(Action::ToggleFavorite(app.d.current.clone())),
            "header" => app.u.header_popup = true,
            "theme" => app.actions.push(Action::SetTheme(if arg == "dark" { ThemePref::Dark } else { ThemePref::Light })),
            "type" => {
                let key = format!("c:{}", app.d.current);
                app.u.composers.entry(key).or_default().text = arg;
            }
            "send" => app.actions.push(Action::Send(format!("c:{}", app.d.current))),
            "attach" => {
                let path = std::path::PathBuf::from(&arg);
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                app.actions.push(Action::Upload { key: format!("c:{}", app.d.current), name, data: UploadData::Path(path) });
            }
            "size" => {
                if let Some((w, h)) = arg.split_once('x')
                    && let (Ok(w), Ok(h)) = (w.parse::<f32>(), h.parse::<f32>())
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(w, h)));
                }
            }
            "shot" => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::new(arg.clone())));
                self.shot = Some(arg);
            }
            "quit" => {
                app.quit_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            "hide" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            other => eprintln!("script: unknown step {other}"),
        }
        false
    }
}
