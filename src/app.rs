//! Application state. Views read [`Data`], keep their own widget state in
//! [`UiState`], and push [`Action`]s; `App` applies actions after the frame
//! and backend events before it. Nothing here waits on the network.

use crate::backend::{Backend, Command, Event, LoginMethod, PostsMode, UploadData};
use crate::config::{self, Config, Dirs, ThemePref};
use crate::images::Images;
use crate::markup::{self, Doc, Link};
use crate::model::*;
use crate::script::Script;
use crate::theme::Theme;
use crate::ui::list::VList;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Center {
    Channel,
    Threads,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Rhs {
    None,
    Thread(String),
    Search,
    Pinned(String),
}

#[derive(Default)]
pub struct ChannelView {
    /// Root posts, oldest first. Pending sends sit at the end.
    pub posts: Vec<Post>,
    pub reached_start: bool,
    pub loading_older: bool,
    pub loaded: bool,
    pub server_loaded: bool,
    /// Posts after this time (not ours) are new since the last visit.
    pub unread_after: i64,
    pub reveal: Option<String>,
    pub reveal_tries: u8,
    pub highlight: Option<(String, f64)>,
}

impl ChannelView {
    fn upsert(&mut self, post: Post) {
        if !post.pending_post_id.is_empty()
            && let Some(i) = self.posts.iter().position(|p| p.pending && p.id == post.pending_post_id)
        {
            self.posts.remove(i);
        }
        if let Some(existing) = self.posts.iter_mut().find(|p| p.id == post.id) {
            *existing = post;
            return;
        }
        let at = self.posts.iter().rposition(|p| !p.pending && p.create_at <= post.create_at).map(|i| i + 1).unwrap_or(0);
        let at = if post.pending { self.posts.len() } else { at };
        self.posts.insert(at, post);
    }
}

pub struct ThreadData {
    pub root_id: String,
    pub posts: Vec<Post>,
    pub loaded: bool,
}

impl ThreadData {
    fn upsert(&mut self, post: Post) {
        if !post.pending_post_id.is_empty()
            && let Some(i) = self.posts.iter().position(|p| p.pending && p.id == post.pending_post_id)
        {
            self.posts.remove(i);
        }
        if let Some(existing) = self.posts.iter_mut().find(|p| p.id == post.id) {
            *existing = post;
        } else {
            self.posts.push(post);
            self.posts.sort_by_key(|p| (p.pending, p.create_at));
        }
    }
}

pub struct Typing {
    pub channel_id: String,
    pub parent_id: String,
    pub user_id: String,
    pub until: f64,
}

#[derive(Default)]
pub struct SearchData {
    pub seq: u64,
    pub terms: String,
    pub local: Vec<Post>,
    pub server: Option<Vec<Post>>,
    pub files: Vec<FileInfo>,
    pub loading: bool,
    pub error: Option<String>,
}

pub struct Data {
    pub theme: Theme,
    pub me: User,
    pub teams: Vec<Team>,
    pub team: String,
    pub channels: HashMap<String, Channel>,
    pub members: HashMap<String, ChannelMember>,
    pub users: HashMap<String, User>,
    pub by_name: HashMap<String, String>,
    pub current: String,
    pub center: Center,
    pub views: HashMap<String, ChannelView>,
    pub rhs: Rhs,
    pub thread: Option<ThreadData>,
    pub typing: Vec<Typing>,
    pub connected: bool,
    pub conn_detail: String,
    pub synced: bool,
    pub have_data: bool,
    pub stats: HashMap<String, ChannelStats>,
    pub pinned: HashMap<String, Vec<Post>>,
    pub threads: Option<UserThreads>,
    pub search: SearchData,
    pub ac_users: Vec<User>,
    pub logged_in: bool,
    /// Sidebar categories per team, in the user's order.
    pub categories: HashMap<String, Vec<Category>>,
    pub browse: Option<BrowseData>,
    /// Custom emoji, name to id.
    pub custom_emoji: HashMap<String, String>,
}

#[derive(Default)]
pub struct BrowseData {
    pub query: String,
    pub sent: Option<String>,
    pub results: Vec<Channel>,
    pub loading: bool,
}

impl Data {
    pub fn user_name(&self, id: &str) -> String {
        self.users.get(id).map(|u| u.display_name()).unwrap_or_else(|| "…".into())
    }

    pub fn channel_name(&self, ch: &Channel) -> String {
        match ch.kind.as_str() {
            "D" => match ch.dm_partner(&self.me.id) {
                Some(id) if id == self.me.id => format!("{} (you)", self.me.display_name()),
                Some(id) => self.users.get(&id).map(|u| u.display_name()).unwrap_or_else(|| "Direct message".into()),
                None => ch.display_name.clone(),
            },
            "G" => ch
                .display_name
                .split(", ")
                .filter(|n| *n != self.me.username)
                .map(|n| self.by_name.get(n).and_then(|id| self.users.get(id)).map(|u| u.first_name.clone()).filter(|f| !f.is_empty()).unwrap_or_else(|| n.to_string()))
                .collect::<Vec<_>>()
                .join(", "),
            _ => {
                if ch.display_name.is_empty() {
                    ch.name.clone()
                } else {
                    ch.display_name.clone()
                }
            }
        }
    }

    /// (unread, mentions) as the sidebar shows them. Collapsed threads: only
    /// root posts count for channels; every message counts in a DM.
    pub fn unread(&self, ch: &Channel) -> (bool, i64) {
        let Some(m) = self.members.get(&ch.id) else { return (false, 0) };
        if m.muted() {
            let mentions = if ch.is_direct() { m.mention_count } else { m.mention_count_root };
            return (false, mentions);
        }
        if ch.is_direct() {
            (ch.total_msg_count > m.msg_count, m.mention_count)
        } else {
            (ch.total_msg_count_root > m.msg_count_root, m.mention_count_root)
        }
    }

    pub fn muted(&self, channel_id: &str) -> bool {
        self.members.get(channel_id).is_some_and(|m| m.muted())
    }

    pub fn favorite(&self, channel_id: &str) -> bool {
        self.categories
            .get(&self.team)
            .is_some_and(|cats| cats.iter().any(|c| c.kind == "favorites" && c.channel_ids.iter().any(|id| id == channel_id)))
    }

    /// Whether a new post here should raise a desktop notification: the
    /// channel's own setting, else the account's, and never when muted.
    pub fn should_notify(&self, channel_id: &str, mentions_me: bool, is_dm: bool) -> bool {
        let Some(m) = self.members.get(channel_id) else { return mentions_me || is_dm };
        if m.muted() {
            return false;
        }
        let channel = m.notify_props["desktop"].as_str().unwrap_or("default");
        let level = if channel == "default" || channel.is_empty() {
            self.me.notify_props["desktop"].as_str().unwrap_or("mention")
        } else {
            channel
        };
        match level {
            "all" => true,
            "none" => false,
            _ => mentions_me || is_dm,
        }
    }

    pub fn current_channel(&self) -> Option<&Channel> {
        self.channels.get(&self.current)
    }

    pub fn find_post(&self, id: &str) -> Option<&Post> {
        for v in self.views.values() {
            if let Some(p) = v.posts.iter().find(|p| p.id == id) {
                return Some(p);
            }
        }
        if let Some(t) = &self.thread
            && let Some(p) = t.posts.iter().find(|p| p.id == id)
        {
            return Some(p);
        }
        None
    }

    fn for_each_post(&mut self, id: &str, mut f: impl FnMut(&mut Post)) {
        for v in self.views.values_mut() {
            for p in v.posts.iter_mut().filter(|p| p.id == id) {
                f(p);
            }
        }
        if let Some(t) = &mut self.thread {
            for p in t.posts.iter_mut().filter(|p| p.id == id) {
                f(p);
            }
        }
        for list in self.pinned.values_mut() {
            for p in list.iter_mut().filter(|p| p.id == id) {
                f(p);
            }
        }
        if let Some(server) = &mut self.search.server {
            for p in server.iter_mut().filter(|p| p.id == id) {
                f(p);
            }
        }
        for p in self.search.local.iter_mut().filter(|p| p.id == id) {
            f(p);
        }
    }
}

#[derive(Default)]
pub struct Upload {
    pub local_id: u64,
    pub name: String,
    pub done: Option<Result<FileInfo, String>>,
}

#[derive(Default)]
pub struct Composer {
    pub text: String,
    pub uploads: Vec<Upload>,
}

pub struct Lightbox {
    pub files: Vec<FileInfo>,
    pub index: usize,
    pub zoom: f32,
    pub pan: egui::Vec2,
}

#[derive(Default)]
pub struct Switcher {
    pub query: String,
    pub selected: usize,
    /// The query people were last looked up for on the server.
    pub asked: String,
}

pub struct PostMenu {
    pub post_id: String,
    pub pos: egui::Pos2,
}

pub struct Picker {
    pub post_id: String,
    pub pos: egui::Pos2,
    pub query: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SearchTab {
    #[default]
    Messages,
    Files,
}

#[derive(Default)]
pub struct LoginForm {
    /// Single sign-on in progress: started at (s), waiting for the browser.
    pub sso_since: Option<f64>,
    pub server: String,
    pub login_id: String,
    pub password: String,
    pub mfa: String,
    pub token: String,
    pub use_token: bool,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct UiState {
    pub images: Images,
    pub docs: HashMap<(String, i64), Arc<Doc>>,
    pub lists: HashMap<String, VList>,
    pub composers: HashMap<String, Composer>,
    pub search_text: String,
    pub search_tab: SearchTab,
    pub focus_search: bool,
    pub focus_composer: bool,
    pub lightbox: Option<Lightbox>,
    pub switcher: Option<Switcher>,
    pub editing: Option<(String, String)>,
    pub header_editor: Option<(String, String)>,
    pub header_popup: bool,
    pub menu: Option<PostMenu>,
    pub picker: Option<Picker>,
    pub confirm_delete: Option<String>,
    pub toasts: Vec<(String, f64)>,
    pub login: LoginForm,
    pub ac_selected: usize,
    pub now: f64,
    pub focused: bool,
    pub settings_open: bool,
    pub header_anchor: Option<egui::Pos2>,
    /// Sidebar categories folded by the user.
    pub collapsed: std::collections::HashSet<String>,
    /// The profile card on screen: user and where it opened.
    pub profile: Option<(String, egui::Pos2)>,
    pub badge: i64,
}

impl UiState {
    pub fn doc(&mut self, post: &Post) -> Arc<Doc> {
        if self.docs.len() > 4000 {
            self.docs.clear();
        }
        self.docs.entry((post.id.clone(), post.edit_at.max(post.update_at))).or_insert_with(|| markup::parse(&post.message)).clone()
    }

    pub fn toast(&mut self, text: impl Into<String>) {
        self.toasts.push((text.into(), self.now + 4.0));
    }
}

pub enum Action {
    ToggleFavorite(String),
    ToggleMute(String),
    OpenBrowse,
    BrowseSearch,
    Join(String),
    Leave(String),
    OpenDm(String),
    StartSso,
    CancelSso,
    SelectTeam(String),
    OpenChannel(String),
    ShowThreads,
    OpenThread(String),
    CloseRhs,
    OpenPinned,
    Send(String),
    SaveEdit { post_id: String, text: String },
    Delete(String),
    React { post_id: String, emoji: String, add: bool },
    Pin { post_id: String, pinned: bool },
    LoadOlder(String),
    OpenLightbox { files: Vec<FileInfo>, index: usize },
    Download { file: FileInfo, open: bool },
    Attach(String),
    Upload { key: String, name: String, data: UploadData },
    Search { server: bool },
    JumpTo(Post),
    SaveHeader { channel_id: String, header: String },
    Link(Link),
    Login,
    Logout,
    SetTheme(ThemePref),
    Retry(String),
    Typing(String),
    MarkThreadRead(String),
    Autocomplete(String),
}

pub struct App {
    pub backend: Backend,
    pub d: Data,
    pub u: UiState,
    pub actions: Vec<Action>,
    pub config: Config,
    pub dirs: Dirs,
    pub session: Option<(String, String)>,
    pub script: Option<Script>,
    pub scrolling: fastframe_scroll::Scrolling,
    next_local: u64,
    last_typing: f64,
    last_view_sent: HashMap<String, f64>,
    system_dark: Option<bool>,
    signed_in_at: Option<std::time::Instant>,
    keychain: Option<std::sync::mpsc::Receiver<Option<String>>>,
    tray: Option<fastframe_tray::Tray>,
    /// Quit, rather than hide, when the window closes.
    pub quit_requested: bool,
    /// The tray or the Dock asked for the window while none exists.
    wants_show: bool,
    has_window: bool,
    started: std::time::Instant,
}

impl App {
    /// Made once, before any window: the backend, the tray and the session
    /// outlive windows. Each window then calls [`App::attach`].
    pub fn new(args: crate::Args, waker: &fastframe_shell::Waker) -> Self {
        let dirs = Dirs::new();
        let config = Config::load(&dirs);
        let backend = Backend::spawn(waker.clone(), dirs.data.clone(), dirs.cache.clone());
        let theme = Theme::light();
        let wake = waker.clone();
        let tray = fastframe_tray::Tray::spawn(
            fastframe_tray::Config {
                id: "mmfast",
                title: "mmfast".into(),
                icon: crate::icon::app_rgba,
                template_icon: Some(crate::icon::template_rgba),
                themed_icon: false,
                menu_on_click: false,
                menu: vec![
                    fastframe_tray::MenuItem::action("show", "Show mmfast"),
                    fastframe_tray::MenuItem::Separator,
                    fastframe_tray::MenuItem::action("quit", "Quit mmfast"),
                ],
            },
            move || wake.wake(),
        );
        let mut app = Self {
            backend,
            d: Data {
                theme,
                me: User::default(),
                teams: Vec::new(),
                team: String::new(),
                channels: HashMap::new(),
                members: HashMap::new(),
                users: HashMap::new(),
                by_name: HashMap::new(),
                current: String::new(),
                center: Center::Channel,
                views: HashMap::new(),
                rhs: Rhs::None,
                thread: None,
                typing: Vec::new(),
                connected: false,
                conn_detail: String::new(),
                synced: false,
                have_data: false,
                stats: HashMap::new(),
                pinned: HashMap::new(),
                threads: None,
                search: SearchData::default(),
                ac_users: Vec::new(),
                logged_in: false,
                categories: HashMap::new(),
                browse: None,
                custom_emoji: HashMap::new(),
            },
            u: UiState::default(),
            actions: Vec::new(),
            config,
            dirs,
            session: None,
            script: args.script.as_deref().map(Script::parse),
            scrolling: fastframe_scroll::Scrolling::default(),
            next_local: 1,
            last_typing: 0.0,
            last_view_sent: HashMap::new(),
            system_dark: None,
            signed_in_at: None,
            keychain: None,
            tray,
            quit_requested: false,
            wants_show: false,
            has_window: false,
            started: std::time::Instant::now(),
        };
        if let Some(t) = &args.theme {
            app.config.theme = match t.as_str() {
                "dark" => ThemePref::Dark,
                "light" => ThemePref::Light,
                _ => ThemePref::System,
            };
        }

        // Where the session comes from: environment (development), the
        // seeded test server, or the keychain.
        let mut session = None;
        if let (Ok(server), Ok(token)) = (std::env::var("MMFAST_SERVER"), std::env::var("MMFAST_TOKEN")) {
            session = Some((crate::api::normalize_server(&server), token));
        } else if args.dev {
            session = dev_session();
        } else if !app.config.server.is_empty() {
            // The keychain may ask the user first (it does after every
            // rebuild); read it off the UI thread so the window still draws.
            let (tx, rx) = std::sync::mpsc::channel();
            let server = app.config.server.clone();
            let wake = waker.clone();
            std::thread::spawn(move || {
                let _ = tx.send(config::load_token(&server));
                wake.wake();
            });
            app.keychain = Some(rx);
            app.d.logged_in = true;
        }
        app.u.login.server = if app.config.server.is_empty() { "https://".into() } else { app.config.server.clone() };
        if let Some((server, token)) = session {
            app.start_session(server, token);
        }
        app
    }

    fn start_session(&mut self, server: String, token: String) {
        self.backend.send(Command::Start { server: server.clone(), token: token.clone() });
        self.session = Some((server, token));
        self.d.logged_in = true;
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        let dark = match self.config.theme {
            ThemePref::Dark => true,
            ThemePref::Light => false,
            ThemePref::System => ctx.system_theme().map(|t| t == egui::Theme::Dark).unwrap_or(false),
        };
        if self.system_dark != Some(dark) {
            self.system_dark = Some(dark);
            self.d.theme = if dark { Theme::dark() } else { Theme::light() };
            ctx.set_visuals(self.d.theme.visuals());
        }
    }

    // ------------------------------------------------------------ events

    pub fn drain_events(&mut self, ctx: &egui::Context) {
        let events: Vec<Event> = self.backend.poll().collect();
        for event in events {
            self.on_event(ctx, event);
        }
    }

    fn on_event(&mut self, ctx: &egui::Context, event: Event) {
        let d = &mut self.d;
        match event {
            Event::SsoUrl(url) => {
                self.u.login.busy = false;
                self.u.login.sso_since = Some(self.u.now);
                if let Err(err) = open::that_detached(&url) {
                    self.u.login.error = Some(format!("Could not open the browser: {err}"));
                    self.u.login.sso_since = None;
                }
            }
            Event::LoginFailed(err) => {
                self.u.login.sso_since = None;
                self.u.login.busy = false;
                self.u.login.error = Some(err);
            }
            Event::LoggedIn { server, token } => {
                self.signed_in_at = Some(std::time::Instant::now());
                self.u.login.busy = false;
                self.u.login.error = None;
                self.u.login.password.clear();
                self.u.login.token.clear();
                config::save_token(&server, &token);
                self.config.server = server.clone();
                self.config.save(&self.dirs);
                self.start_session(server, token);
            }
            Event::Snapshot { snap, from_cache } => {
                let first = !d.have_data;
                if first || (!from_cache && !d.synced) {
                    log::info!("{} sidebar after {} ms", if from_cache { "cached" } else { "synced" }, crate::since_start_ms());
                    if let Some(at) = self.signed_in_at.take() {
                        log::info!("sidebar {} ms after the server accepted the sign-in", at.elapsed().as_millis());
                    }
                }
                d.have_data = true;
                if !from_cache {
                    d.synced = true;
                }
                d.me = snap.me;
                d.teams = snap.teams;
                d.teams.sort_by_key(|t| t.display_name.to_lowercase());
                d.channels = snap.channels.into_iter().map(|c| (c.id.clone(), c)).collect();
                // Keep counts we already cleared for the open channel.
                for m in snap.members {
                    d.members.insert(m.channel_id.clone(), m);
                }
                for u in snap.users {
                    d.by_name.insert(u.username.clone(), u.id.clone());
                    d.users.insert(u.id.clone(), u);
                }
                d.by_name.insert(d.me.username.clone(), d.me.id.clone());
                d.users.insert(d.me.id.clone(), d.me.clone());
                if d.team.is_empty() || !d.teams.iter().any(|t| t.id == d.team) {
                    d.team = if d.teams.iter().any(|t| t.id == self.config.last_team) {
                        self.config.last_team.clone()
                    } else {
                        d.teams.first().map(|t| t.id.clone()).unwrap_or_default()
                    };
                }
                if first || !from_cache {
                    self.backend.send(Command::LoadCategories { team_id: d.team.clone() });
                }
                if first {
                    self.backend.send(Command::LoadEmoji);
                }
                if first || !d.channels.contains_key(&d.current) {
                    let team = d.team.clone();
                    if let Some(id) = self.default_channel(&team) {
                        self.actions.push(Action::OpenChannel(id));
                    }
                } else if !d.current.is_empty() {
                    // The open channel stays read.
                    self.mark_read_locally(&self.d.current.clone());
                }
            }
            Event::ChannelPosts { channel_id, posts, mode, reached_start } => {
                let view = d.views.entry(channel_id.clone()).or_default();
                if !view.loaded {
                    log::info!("first posts of a channel ({} posts, {:?}) after {} ms", posts.len(), mode, crate::since_start_ms());
                }
                match mode {
                    PostsMode::Latest => {
                        let pending: Vec<Post> = view.posts.drain(..).filter(|p| p.pending).collect();
                        view.posts = posts;
                        view.posts.extend(pending);
                        if let Some(r) = reached_start {
                            view.reached_start = r;
                            view.server_loaded = true;
                        }
                    }
                    PostsMode::Merge => {
                        for p in posts {
                            view.upsert(p);
                        }
                        view.server_loaded = true;
                    }
                    PostsMode::Older => {
                        view.loading_older = false;
                        let mut older: Vec<Post> =
                            posts.into_iter().filter(|p| !view.posts.iter().any(|q| q.id == p.id)).collect();
                        older.append(&mut view.posts);
                        view.posts = older;
                        if let Some(r) = reached_start {
                            view.reached_start = r;
                        }
                    }
                }
                view.loaded = true;
            }
            Event::ThreadPosts { root_id, posts } => {
                if let Some(t) = &mut d.thread
                    && t.root_id == root_id
                {
                    let pending: Vec<Post> = t.posts.drain(..).filter(|p| p.pending).collect();
                    t.posts = posts;
                    t.posts.sort_by_key(|p| p.create_at);
                    t.posts.extend(pending);
                    t.loaded = true;
                    let count = t.posts.iter().filter(|p| !p.root_id.is_empty() && !p.pending).count() as i64;
                    d.for_each_post(&root_id, |p| p.reply_count = count);
                }
            }
            Event::PostUpsert { post, mentions_me, live } => self.on_post(ctx, post, mentions_me, live),
            Event::PostDeleted { post } => {
                for v in d.views.values_mut() {
                    v.posts.retain(|p| p.id != post.id);
                }
                if let Some(t) = &mut d.thread {
                    if t.root_id == post.id {
                        d.thread = None;
                        if matches!(d.rhs, Rhs::Thread(_)) {
                            d.rhs = Rhs::None;
                        }
                    } else {
                        t.posts.retain(|p| p.id != post.id);
                    }
                }
                if !post.root_id.is_empty() {
                    d.for_each_post(&post.root_id, |p| p.reply_count = (p.reply_count - 1).max(0));
                }
            }
            Event::Reaction { reaction, added } => {
                d.for_each_post(&reaction.post_id.clone(), |p| {
                    p.metadata.reactions.retain(|r| !(r.user_id == reaction.user_id && r.emoji_name == reaction.emoji_name));
                    if added {
                        p.metadata.reactions.push(reaction.clone());
                    }
                });
            }
            Event::Typing { channel_id, parent_id, user_id } => {
                d.typing.retain(|t| !(t.user_id == user_id && t.channel_id == channel_id));
                d.typing.push(Typing { channel_id, parent_id, user_id, until: self.u.now + 5.0 });
                ctx.request_repaint_after(std::time::Duration::from_secs(5));
            }
            Event::ChannelUpdated(channel) => {
                d.channels.insert(channel.id.clone(), channel);
            }
            Event::ChannelViewed { channel_id } => self.mark_read_locally(&channel_id),
            Event::Users(users) => {
                for u in users {
                    if u.id == d.me.id {
                        d.me = u.clone();
                    }
                    d.by_name.insert(u.username.clone(), u.id.clone());
                    d.users.insert(u.id.clone(), u);
                }
            }
            Event::Image { key, image } => self.u.images.deliver(key, image),
            Event::Animation { key, frames } => self.u.images.deliver_animation(key, frames),
            Event::SearchResults { seq, posts, files, server, error } => {
                if seq == d.search.seq {
                    if server {
                        d.search.server = Some(posts);
                        d.search.files = files;
                        d.search.loading = false;
                        d.search.error = error;
                    } else {
                        d.search.local = posts;
                    }
                }
            }
            Event::Pinned { channel_id, posts } => {
                d.pinned.insert(channel_id, posts);
            }
            Event::Stats(stats) => {
                d.stats.insert(stats.channel_id.clone(), stats);
            }
            Event::Threads { team_id, threads } => {
                if team_id == d.team {
                    d.threads = Some(threads);
                }
            }
            Event::ThreadUpdated => {
                if d.center == Center::Threads {
                    self.backend.send(Command::LoadThreads { team_id: d.team.clone() });
                }
            }
            Event::UploadDone { local_id, result } => {
                for c in self.u.composers.values_mut() {
                    for up in c.uploads.iter_mut().filter(|u| u.local_id == local_id) {
                        if let Err(e) = &result {
                            self.u.toasts.push((format!("Upload failed: {e}"), self.u.now + 5.0));
                        }
                        up.done = Some(result.clone());
                    }
                }
            }
            Event::Downloaded { name, result } => match result {
                Ok(path) => self.u.toast(format!("Saved {name} to {}", path.parent().map(|p| p.display().to_string()).unwrap_or_default())),
                Err(e) => self.u.toast(format!("Download failed: {e}")),
            },
            Event::Connection { connected, detail } => {
                d.connected = connected;
                d.conn_detail = detail;
            }
            Event::Reconnected => {
                if !d.current.is_empty() {
                    self.backend.send(Command::OpenChannel { channel_id: d.current.clone() });
                }
                if let Some(t) = &d.thread {
                    self.backend.send(Command::OpenThread { root_id: t.root_id.clone() });
                }
            }
            Event::SendFailed { pending_id, error } => {
                d.for_each_post(&pending_id, |p| p.failed = true);
                self.u.toast(format!("Message not sent: {error}"));
            }
            Event::Autocomplete { users } => {
                for u in &users {
                    d.by_name.insert(u.username.clone(), u.id.clone());
                    d.users.insert(u.id.clone(), u.clone());
                }
                d.ac_users = users;
            }
            Event::Toast(text) => self.u.toast(text),
            Event::JumpPost(post) => {
                if d.channels.contains_key(&post.channel_id) {
                    self.actions.push(Action::JumpTo(post));
                } else {
                    self.u.toast("That message is in a channel you are not in.");
                }
            }
            Event::CustomEmoji(list) => {
                d.custom_emoji = list.into_iter().collect();
            }
            Event::Categories { team_id, categories } => {
                d.categories.insert(team_id, categories);
            }
            Event::MemberUpdated(member) => {
                d.members.insert(member.channel_id.clone(), member);
            }
            Event::BrowseResults { term, channels } => {
                if let Some(b) = &mut d.browse
                    && b.sent.as_deref() == Some(term.as_str())
                {
                    b.results = channels;
                    b.loading = false;
                }
            }
            Event::ChannelJoined { channel, member } => {
                let id = channel.id.clone();
                d.channels.insert(id.clone(), channel);
                d.members.insert(id.clone(), member);
                d.browse = None;
                self.backend.send(Command::LoadCategories { team_id: d.team.clone() });
                self.actions.push(Action::OpenChannel(id));
            }
            Event::ChannelLeft { channel_id } => {
                d.channels.remove(&channel_id);
                d.members.remove(&channel_id);
                d.views.remove(&channel_id);
                if d.current == channel_id {
                    d.current.clear();
                    let team = d.team.clone();
                    if let Some(id) = self.default_channel(&team) {
                        self.actions.push(Action::OpenChannel(id));
                    }
                }
            }
            Event::Unauthorized => {
                if let Some((server, _)) = self.session.take() {
                    config::forget_token(&server);
                }
                self.backend.send(Command::Logout);
                self.d.logged_in = false;
                self.u.login.error = Some("Your session has expired. Please sign in again.".into());
            }
        }
    }

    fn on_post(&mut self, ctx: &egui::Context, post: Post, mentions_me: bool, live: bool) {
        let d = &mut self.d;
        let from_me = post.user_id == d.me.id;
        let channel_id = post.channel_id.clone();
        if post.root_id.is_empty() {
            if let Some(view) = d.views.get_mut(&channel_id)
                && view.loaded
            {
                view.upsert(post.clone());
            }
        } else {
            if let Some(t) = &mut d.thread
                && t.root_id == post.root_id
            {
                t.upsert(post.clone());
            }
            if live {
                let (root, at) = (post.root_id.clone(), post.create_at);
                let me = d.me.id.clone();
                let uid = post.user_id.clone();
                d.for_each_post(&root, |p| {
                    p.reply_count += 1;
                    p.last_reply_at = at;
                    let parts = p.participants.get_or_insert_with(Vec::new);
                    if !parts.iter().any(|u| u.id == uid) {
                        parts.push(User { id: uid.clone(), ..Default::default() });
                    }
                    let _ = &me;
                });
            }
        }
        if !live {
            return;
        }
        let Some(ch) = d.channels.get_mut(&channel_id) else {
            self.backend.send(Command::Resync);
            return;
        };
        ch.total_msg_count += 1;
        if post.root_id.is_empty() {
            ch.total_msg_count_root += 1;
            ch.last_root_post_at = post.create_at;
        }
        ch.last_post_at = post.create_at;
        let is_dm = ch.is_direct();
        let channel_label = d.channel_name(&d.channels[&channel_id]);
        let reading = d.current == channel_id && d.center == Center::Channel && self.u.focused;
        if from_me || reading {
            self.mark_read_locally(&channel_id);
            if reading && !from_me {
                self.send_view(&channel_id);
            }
            return;
        }
        if let Some(m) = self.d.members.get_mut(&channel_id)
            && (mentions_me || is_dm)
        {
            m.mention_count += 1;
            if post.root_id.is_empty() || is_dm {
                m.mention_count_root += 1;
            }
        }
        if self.d.should_notify(&channel_id, mentions_me, is_dm) && !post.is_system() {
            let who = self.d.user_name(&post.user_id);
            let title = if is_dm { who.clone() } else { format!("{who} in {channel_label}") };
            let body = markup::plain(&post.message);
            let body = if body.is_empty() { "sent a file".to_string() } else { body };
            std::thread::spawn(move || {
                let _ = notify_rust::Notification::new().summary(&title).body(&body).show();
            });
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational));
        }
    }

    fn mark_read_locally(&mut self, channel_id: &str) {
        if let (Some(ch), Some(m)) = (self.d.channels.get(channel_id), self.d.members.get_mut(channel_id)) {
            m.msg_count = ch.total_msg_count;
            m.msg_count_root = ch.total_msg_count_root;
            m.mention_count = 0;
            m.mention_count_root = 0;
        }
    }

    fn send_view(&mut self, channel_id: &str) {
        let last = self.last_view_sent.get(channel_id).copied().unwrap_or(-10.0);
        if self.u.now - last > 1.0 {
            self.last_view_sent.insert(channel_id.to_string(), self.u.now);
            self.backend.send(Command::ViewChannel { channel_id: channel_id.to_string() });
        }
    }

    fn default_channel(&self, team: &str) -> Option<String> {
        if let Some(id) = self.config.last_channel.get(team)
            && self.d.channels.contains_key(id)
        {
            return Some(id.clone());
        }
        self.d
            .channels
            .values()
            .find(|c| c.team_id == team && c.name == "town-square")
            .or_else(|| self.d.channels.values().find(|c| c.team_id == team))
            .map(|c| c.id.clone())
    }

    // ------------------------------------------------------------ actions

    pub fn apply_actions(&mut self, ctx: &egui::Context) {
        let actions = std::mem::take(&mut self.actions);
        for action in actions {
            self.apply(ctx, action);
        }
        // Drain actions produced while applying (e.g. JumpTo -> OpenChannel).
        if !self.actions.is_empty() {
            let more = std::mem::take(&mut self.actions);
            for action in more {
                self.apply(ctx, action);
            }
        }
        for key in self.u.images.take_requests() {
            self.backend.send(Command::Image(key));
        }
    }

    fn composer_target(&self, key: &str) -> Option<(String, String)> {
        if let Some(ch) = key.strip_prefix("c:") {
            return Some((ch.to_string(), String::new()));
        }
        let root = key.strip_prefix("t:")?;
        let t = self.d.thread.as_ref()?;
        let root_post = t.posts.iter().find(|p| p.id == root)?;
        Some((root_post.channel_id.clone(), root.to_string()))
    }

    pub fn apply(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::SelectTeam(id) => {
                self.d.team = id.clone();
                self.backend.send(Command::LoadCategories { team_id: id.clone() });
                self.config.last_team = id.clone();
                self.config.save(&self.dirs);
                if self.d.center == Center::Threads {
                    self.d.threads = None;
                    self.backend.send(Command::LoadThreads { team_id: id.clone() });
                } else if let Some(ch) = self.default_channel(&id) {
                    self.apply(ctx, Action::OpenChannel(ch));
                }
            }
            Action::OpenChannel(id) => {
                let Some(ch) = self.d.channels.get(&id).cloned() else { return };
                if !ch.is_direct() && ch.team_id != self.d.team && !ch.team_id.is_empty() {
                    self.d.team = ch.team_id.clone();
                    self.config.last_team = ch.team_id.clone();
                }
                let switching = self.d.current != id || self.d.center != Center::Channel;
                self.d.center = Center::Channel;
                self.d.current = id.clone();
                if switching {
                    let (unread, _) = self.d.unread(&ch);
                    let last_viewed = self.d.members.get(&id).map(|m| m.last_viewed_at).unwrap_or(0);
                    let view = self.d.views.entry(id.clone()).or_default();
                    view.unread_after = if unread { last_viewed } else { 0 };
                    if let Rhs::Pinned(_) = self.d.rhs {
                        self.d.rhs = Rhs::Pinned(id.clone());
                        self.backend.send(Command::LoadPinned { channel_id: id.clone() });
                    }
                    self.u.header_popup = false;
                }
                self.mark_read_locally(&id);
                self.backend.send(Command::OpenChannel { channel_id: id.clone() });
                self.backend.send(Command::ViewChannel { channel_id: id.clone() });
                self.last_view_sent.insert(id.clone(), self.u.now);
                self.backend.send(Command::LoadStats { channel_id: id.clone() });
                let team = if ch.is_direct() { self.d.team.clone() } else { ch.team_id.clone() };
                self.config.last_channel.insert(team, id);
                self.config.save(&self.dirs);
                self.u.focus_composer = true;
            }
            Action::ShowThreads => {
                self.d.center = Center::Threads;
                self.backend.send(Command::LoadThreads { team_id: self.d.team.clone() });
            }
            Action::OpenThread(root_id) => {
                let seed = self.d.find_post(&root_id).cloned();
                if self.d.thread.as_ref().map(|t| t.root_id != root_id).unwrap_or(true) {
                    self.d.thread = Some(ThreadData { root_id: root_id.clone(), posts: seed.into_iter().collect(), loaded: false });
                }
                self.d.rhs = Rhs::Thread(root_id.clone());
                self.backend.send(Command::OpenThread { root_id: root_id.clone() });
                self.u.focus_composer = true;
            }
            Action::CloseRhs => {
                if self.d.rhs == Rhs::Search {
                    self.u.search_text.clear();
                    self.d.search = SearchData { seq: self.d.search.seq + 1, ..Default::default() };
                }
                self.d.rhs = Rhs::None;
            }
            Action::OpenPinned => {
                if matches!(self.d.rhs, Rhs::Pinned(_)) {
                    self.d.rhs = Rhs::None;
                } else {
                    let id = self.d.current.clone();
                    self.d.rhs = Rhs::Pinned(id.clone());
                    self.backend.send(Command::LoadPinned { channel_id: id });
                }
            }
            Action::Send(key) => self.send(key),
            Action::SaveEdit { post_id, text } => {
                let now = crate::api::now_ms();
                self.d.for_each_post(&post_id, |p| {
                    p.message = text.clone();
                    p.edit_at = now;
                });
                self.backend.send(Command::Edit { post_id, message: text });
            }
            Action::Delete(post_id) => {
                let pending = self.d.find_post(&post_id).is_some_and(|p| p.pending);
                for v in self.d.views.values_mut() {
                    v.posts.retain(|p| p.id != post_id);
                }
                if let Some(t) = &mut self.d.thread {
                    t.posts.retain(|p| p.id != post_id);
                }
                if !pending {
                    self.backend.send(Command::Delete { post_id });
                }
            }
            Action::React { post_id, emoji, add } => {
                let me = self.d.me.id.clone();
                self.d.for_each_post(&post_id, |p| {
                    p.metadata.reactions.retain(|r| !(r.user_id == me && r.emoji_name == emoji));
                    if add {
                        p.metadata.reactions.push(Reaction {
                            user_id: me.clone(),
                            post_id: p.id.clone(),
                            emoji_name: emoji.clone(),
                            create_at: crate::api::now_ms(),
                        });
                    }
                });
                self.backend.send(Command::React { post_id, emoji, add });
            }
            Action::Pin { post_id, pinned } => {
                self.d.for_each_post(&post_id, |p| p.is_pinned = pinned);
                self.backend.send(Command::Pin { post_id, pinned });
                if let Some(s) = self.d.stats.get_mut(&self.d.current) {
                    s.pinnedpost_count = (s.pinnedpost_count + if pinned { 1 } else { -1 }).max(0);
                }
            }
            Action::LoadOlder(channel_id) => {
                if let Some(view) = self.d.views.get_mut(&channel_id)
                    && !view.loading_older
                    && !view.reached_start
                    && view.server_loaded
                    && let Some(first) = view.posts.iter().find(|p| !p.pending)
                {
                    view.loading_older = true;
                    log::debug!("load older before {} ({} posts shown)", first.id, view.posts.len());
                    self.backend.send(Command::LoadOlder { channel_id, before_id: first.id.clone() });
                }
            }
            Action::OpenLightbox { files, index } => {
                self.u.lightbox = Some(Lightbox { files, index, zoom: 1.0, pan: egui::Vec2::ZERO });
            }
            Action::Download { file, open } => {
                self.u.toast(format!("Downloading {}…", file.name));
                self.backend.send(Command::Download { file, open });
            }
            Action::Attach(key) => {
                if let Some(paths) = rfd::FileDialog::new().pick_files() {
                    for path in paths {
                        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        self.apply(ctx, Action::Upload { key: key.clone(), name, data: UploadData::Path(path) });
                    }
                }
            }
            Action::Upload { key, name, data } => {
                let Some((channel_id, _)) = self.composer_target(&key) else { return };
                let local_id = self.next_local;
                self.next_local += 1;
                self.u.composers.entry(key).or_default().uploads.push(Upload { local_id, name: name.clone(), done: None });
                self.backend.send(Command::Upload { local_id, channel_id, name, data });
            }
            Action::Search { server } => {
                let terms = self.u.search_text.trim().to_string();
                let s = &mut self.d.search;
                s.seq += 1;
                if terms.is_empty() {
                    *s = SearchData { seq: s.seq, ..Default::default() };
                    return;
                }
                if server {
                    s.loading = true;
                    s.server = None;
                    s.files.clear();
                    s.error = None;
                    self.d.rhs = Rhs::Search;
                }
                s.terms = terms.clone();
                self.backend.send(Command::Search { seq: s.seq, team_id: self.d.team.clone(), terms: terms.clone(), server: false });
                if server {
                    self.backend.send(Command::Search { seq: s.seq, team_id: self.d.team.clone(), terms, server: true });
                } else if !terms.is_empty() {
                    self.d.rhs = Rhs::Search;
                }
            }
            Action::JumpTo(post) => {
                if !post.root_id.is_empty() {
                    if self.d.channels.contains_key(&post.channel_id) {
                        self.apply(ctx, Action::OpenChannel(post.channel_id.clone()));
                    }
                    self.apply(ctx, Action::OpenThread(post.root_id.clone()));
                    return;
                }
                self.apply(ctx, Action::OpenChannel(post.channel_id.clone()));
                let view = self.d.views.entry(post.channel_id.clone()).or_default();
                view.reveal = Some(post.id.clone());
                view.reveal_tries = 0;
                view.highlight = Some((post.id.clone(), self.u.now + 3.0));
            }
            Action::SaveHeader { channel_id, header } => {
                if let Some(ch) = self.d.channels.get_mut(&channel_id) {
                    ch.header = header.clone();
                }
                self.backend.send(Command::PatchHeader { channel_id, header });
            }
            Action::Link(link) => match link {
                Link::Url(url) => {
                    if !self.open_in_app(ctx, &url) {
                        let _ = open::that_detached(url);
                    }
                }
                Link::User(name) => match self.d.by_name.get(&name).cloned() {
                    Some(uid) => self.apply(ctx, Action::OpenDm(uid)),
                    None => self.u.toast(format!("@{name} is not a known user")),
                },
                Link::Channel(name) => {
                    let team = self.d.team.clone();
                    let found = self.d.channels.values().find(|c| c.name == name && (c.team_id == team || c.team_id.is_empty())).map(|c| c.id.clone());
                    match found {
                        Some(id) => self.apply(ctx, Action::OpenChannel(id)),
                        None => self.u.toast(format!("~{name} is not one of your channels")),
                    }
                }
            },
            Action::ToggleFavorite(channel_id) => {
                let favorite = !self.d.favorite(&channel_id);
                // Show it at once; the server's categories follow.
                if let Some(cats) = self.d.categories.get_mut(&self.d.team) {
                    for c in cats.iter_mut() {
                        if c.kind == "favorites" {
                            if favorite {
                                c.channel_ids.insert(0, channel_id.clone());
                            } else {
                                c.channel_ids.retain(|id| id != &channel_id);
                            }
                        } else if favorite {
                            c.channel_ids.retain(|id| id != &channel_id);
                        }
                    }
                }
                self.backend.send(Command::SetFavorite { team_id: self.d.team.clone(), channel_id, favorite });
            }
            Action::ToggleMute(channel_id) => {
                let muted = !self.d.muted(&channel_id);
                if let Some(m) = self.d.members.get_mut(&channel_id) {
                    m.notify_props["mark_unread"] = serde_json::json!(if muted { "mention" } else { "all" });
                }
                self.backend.send(Command::SetMuted { channel_id, muted });
            }
            Action::OpenBrowse => {
                self.d.browse = Some(BrowseData { sent: Some(String::new()), loading: true, ..Default::default() });
                self.backend.send(Command::BrowseChannels { team_id: self.d.team.clone(), term: String::new() });
            }
            Action::BrowseSearch => {
                if let Some(b) = &mut self.d.browse {
                    b.sent = Some(b.query.clone());
                    b.loading = true;
                    self.backend.send(Command::BrowseChannels { team_id: self.d.team.clone(), term: b.query.clone() });
                }
            }
            Action::Join(channel_id) => self.backend.send(Command::JoinChannel { channel_id }),
            Action::Leave(channel_id) => self.backend.send(Command::LeaveChannel { channel_id }),
            Action::OpenDm(user_id) => {
                let me = self.d.me.id.clone();
                let existing = self.d.channels.values().find(|c| c.dm_partner(&me).as_deref() == Some(user_id.as_str())).map(|c| c.id.clone());
                match existing {
                    Some(id) => self.apply(ctx, Action::OpenChannel(id)),
                    None => self.backend.send(Command::OpenDm { user_id }),
                }
            }
            Action::StartSso => {
                let f = &mut self.u.login;
                f.busy = true;
                f.error = None;
                self.backend.send(Command::SsoStart { server: f.server.clone() });
            }
            Action::CancelSso => {
                self.u.login.sso_since = None;
                self.u.login.busy = false;
            }
            Action::Login => {
                let f = &mut self.u.login;
                f.busy = true;
                f.error = None;
                let method = if f.use_token {
                    LoginMethod::Token(f.token.clone())
                } else {
                    LoginMethod::Password { login_id: f.login_id.clone(), password: f.password.clone(), mfa: f.mfa.clone() }
                };
                self.backend.send(Command::Login { server: f.server.clone(), method });
            }
            Action::Logout => {
                if let Some((server, _)) = self.session.take() {
                    config::forget_token(&server);
                }
                self.backend.send(Command::Logout);
                let theme = self.d.theme.clone();
                self.d = Data { theme, ..empty_data() };
                self.u = UiState { login: LoginForm { server: self.config.server.clone(), ..Default::default() }, ..Default::default() };
            }
            Action::SetTheme(pref) => {
                self.config.theme = pref;
                self.config.save(&self.dirs);
                self.system_dark = None;
                self.apply_theme(ctx);
            }
            Action::Retry(pending_id) => {
                let Some(post) = self.d.find_post(&pending_id).cloned() else { return };
                self.d.for_each_post(&pending_id, |p| p.failed = false);
                self.backend.send(Command::Send {
                    channel_id: post.channel_id,
                    root_id: post.root_id,
                    message: post.message,
                    pending_id,
                    file_ids: post.file_ids,
                });
            }
            Action::Typing(key) => {
                if self.u.now - self.last_typing > 3.0
                    && let Some((channel_id, parent_id)) = self.composer_target(&key)
                {
                    self.last_typing = self.u.now;
                    self.backend.send(Command::Typing { channel_id, parent_id });
                }
            }
            Action::MarkThreadRead(thread_id) => {
                self.backend.send(Command::MarkThreadRead { team_id: self.d.team.clone(), thread_id: thread_id.clone() });
                if let Some(t) = &mut self.d.threads
                    && let Some(th) = t.threads.iter_mut().find(|t| t.id == thread_id)
                {
                    th.unread_replies = 0;
                    th.unread_mentions = 0;
                }
            }
            Action::Autocomplete(prefix) => {
                self.backend.send(Command::Autocomplete {
                    team_id: self.d.team.clone(),
                    channel_id: self.d.current.clone(),
                    prefix,
                });
            }
        }
    }

    fn send(&mut self, key: String) {
        let Some((channel_id, root_id)) = self.composer_target(&key) else { return };
        let composer = self.u.composers.entry(key.clone()).or_default();
        let text = composer.text.trim().to_string();
        let files: Vec<FileInfo> = composer.uploads.iter().filter_map(|u| u.done.as_ref().and_then(|r| r.as_ref().ok()).cloned()).collect();
        if composer.uploads.iter().any(|u| u.done.is_none()) {
            self.u.toast("Wait for the uploads to finish");
            return;
        }
        if text.is_empty() && files.is_empty() {
            return;
        }
        composer.text.clear();
        composer.uploads.clear();
        let now = crate::api::now_ms();
        let pending_id = format!("{}:{}", self.d.me.id, now);
        let post = Post {
            id: pending_id.clone(),
            pending_post_id: pending_id.clone(),
            create_at: now,
            update_at: now,
            user_id: self.d.me.id.clone(),
            channel_id: channel_id.clone(),
            root_id: root_id.clone(),
            message: text.clone(),
            file_ids: files.iter().map(|f| f.id.clone()).collect(),
            metadata: PostMetadata { files: files.clone(), reactions: Vec::new() },
            pending: true,
            ..Default::default()
        };
        if root_id.is_empty() {
            self.d.views.entry(channel_id.clone()).or_default().upsert(post);
            if let Some(list) = self.u.lists.get_mut(&format!("c:{channel_id}")) {
                list.stick_to_bottom();
            }
        } else if let Some(t) = &mut self.d.thread {
            t.upsert(post);
            if let Some(list) = self.u.lists.get_mut(&format!("t:{root_id}")) {
                list.stick_to_bottom();
            }
        }
        self.backend.send(Command::Send {
            channel_id,
            root_id,
            message: text,
            pending_id,
            file_ids: files.into_iter().map(|f| f.id).collect(),
        });
    }

    /// Links into our own server open here: permalinks, channels, DMs.
    fn open_in_app(&mut self, ctx: &egui::Context, link: &str) -> bool {
        let Some((server, _)) = &self.session else { return false };
        let (Ok(url), Ok(base)) = (url::Url::parse(link), url::Url::parse(server)) else { return false };
        if url.host_str() != base.host_str() {
            return false;
        }
        let base_path = base.path().trim_end_matches('/');
        let rest = url.path().strip_prefix(base_path).unwrap_or(url.path());
        let parts: Vec<&str> = rest.split('/').filter(|p| !p.is_empty()).collect();
        match parts.as_slice() {
            [_team, "pl", post_id] => {
                self.backend.send(Command::JumpPost { post_id: post_id.to_string() });
                true
            }
            [team, "channels", name] => {
                let team_id = self.d.teams.iter().find(|t| &t.name == team).map(|t| t.id.clone());
                let found = self
                    .d
                    .channels
                    .values()
                    .find(|c| &c.name == name && (Some(&c.team_id) == team_id.as_ref() || c.is_direct()))
                    .map(|c| c.id.clone());
                match found {
                    Some(id) => self.apply(ctx, Action::OpenChannel(id)),
                    None => self.u.toast(format!("~{name} is not one of your channels")),
                }
                true
            }
            [_team, "messages", who] if who.starts_with('@') => {
                self.apply(ctx, Action::Link(Link::User(who[1..].to_string())));
                true
            }
            _ => false,
        }
    }

    /// A link handed to the app: the end of single sign-on. Only accepted
    /// while a sign-on we started is waiting, so a stray link cannot sign
    /// the app into someone else's account.
    pub fn on_link(&mut self, link: &str) {
        let Ok(url) = url::Url::parse(link) else { return };
        if url.scheme() != "mmauth" {
            log::warn!("ignoring a link of scheme {}", url.scheme());
            return;
        }
        let waiting = self.u.login.sso_since.is_some_and(|t| self.u.now - t < 900.0);
        if !waiting || self.d.logged_in {
            log::warn!("ignoring a sign-in link nobody asked for");
            return;
        }
        let query: HashMap<String, String> = url.query_pairs().into_owned().collect();
        let Some(token) = query.get("MMAUTHTOKEN").filter(|t| !t.is_empty()) else {
            self.u.login.error = Some("The server did not send a session back.".into());
            self.u.login.sso_since = None;
            return;
        };
        if let Some(srv) = query.get("srv")
            && crate::api::normalize_server(srv) != crate::api::normalize_server(&self.u.login.server)
        {
            self.u.login.error = Some(format!("The sign-in came back from {srv}, not the server you chose."));
            self.u.login.sso_since = None;
            return;
        }
        self.u.login.sso_since = None;
        self.u.login.token = token.clone();
        self.u.login.use_token = true;
        self.actions.push(Action::Login);
    }

    pub fn frame_start(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "macos")]
        for link in crate::macos::links::take() {
            self.on_link(&link);
        }
        self.u.now = ctx.input(|i| i.time);
        self.u.focused = ctx.input(|i| i.focused);
        self.u.images.begin_frame(ctx.cumulative_frame_nr());
        self.apply_theme(ctx);
        let now = self.u.now;
        self.d.typing.retain(|t| t.until > now);
        self.u.toasts.retain(|(_, until)| *until > now);
        if !self.u.toasts.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        // Coming back to the window reads the open channel.
        if self.u.focused && self.d.center == Center::Channel && !self.d.current.is_empty() {
            let id = self.d.current.clone();
            if let Some(ch) = self.d.channels.get(&id)
                && self.d.unread(ch).0
            {
                self.mark_read_locally(&id);
                self.send_view(&id);
            }
        }
    }

    pub fn frame_end(&mut self) {
        self.u.images.sweep();
    }

}

fn empty_data() -> Data {
    Data {
        theme: Theme::light(),
        me: User::default(),
        teams: Vec::new(),
        team: String::new(),
        channels: HashMap::new(),
        members: HashMap::new(),
        users: HashMap::new(),
        by_name: HashMap::new(),
        current: String::new(),
        center: Center::Channel,
        views: HashMap::new(),
        rhs: Rhs::None,
        thread: None,
        typing: Vec::new(),
        connected: false,
        conn_detail: String::new(),
        synced: false,
        have_data: false,
        stats: HashMap::new(),
        pinned: HashMap::new(),
        threads: None,
        search: SearchData::default(),
        ac_users: Vec::new(),
        logged_in: false,
        categories: HashMap::new(),
        browse: None,
        custom_emoji: HashMap::new(),
    }
}

/// Where single sign-on comes back to: a link the server accepts by default
/// (NativeAppSettings.AppCustomURLSchemes) and our app bundle answers.
pub const SSO_REDIRECT: &str = "mmauth://callback";

/// The seeded local test server (dev/seed.py), for `--dev`.
fn dev_session() -> Option<(String, String)> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("dev/seed-credentials.json");
    let creds: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let server = creds["server"].as_str()?.to_string();
    let token = creds["users"]["aritra"]["access_token"].as_str()?.to_string();
    Some((server, token))
}

impl App {
    /// A new window exists: fonts, menus, theme and textures belong to it.
    pub fn attach(&mut self, ctx: &egui::Context) {
        crate::ui::install_fonts(ctx);
        self.system_dark = None;
        self.apply_theme(ctx);
        self.u.images = Images::default();
        self.has_window = true;
        if let Some(tray) = &mut self.tray {
            tray.attach();
        }
    }

    /// Closing the window keeps the app in the menu bar when it can come back.
    fn keeps_running(&self) -> bool {
        self.config.keep_running && self.tray.as_ref().is_some_and(fastframe_tray::Tray::is_shown) && self.d.logged_in
    }

    /// Work a window's frame and a headless tick share: the backend's events,
    /// the tray, the Dock badge.
    fn tick(&mut self, ctx: &egui::Context) {
        if !self.has_window {
            self.u.now = self.started.elapsed().as_secs_f64();
            self.u.focused = false;
        }
        if let Some(rx) = &self.keychain
            && let Ok(token) = rx.try_recv()
        {
            self.keychain = None;
            match token {
                Some(token) => self.start_session(self.config.server.clone(), token),
                None => self.d.logged_in = false,
            }
        }
        self.drain_events(ctx);
        let events = self.tray.as_ref().map(|t| t.events()).unwrap_or_default();
        for event in events {
            log::debug!("tray: {event:?} (window {})", if self.has_window { "open" } else { "closed" });
            match event {
                fastframe_tray::Event::Toggle | fastframe_tray::Event::Menu("show") => {
                    if self.has_window {
                        if self.keeps_running() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    } else {
                        self.wants_show = true;
                    }
                }
                fastframe_tray::Event::Show => {
                    if self.has_window {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    } else {
                        self.wants_show = true;
                    }
                }
                fastframe_tray::Event::Menu("quit") => {
                    self.quit_requested = true;
                    if self.has_window {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                fastframe_tray::Event::Menu(_) => {}
            }
        }
        if !self.has_window {
            self.apply_actions(ctx);
        }
        self.update_badge();
    }

    fn update_badge(&mut self) {
        let mentions: i64 = if self.d.logged_in { self.d.channels.values().map(|c| self.d.unread(c).1).sum() } else { 0 };
        if mentions != self.u.badge {
            log::debug!("dock badge {mentions} (window {})", if self.has_window { "open" } else { "closed" });
            self.u.badge = mentions;
            #[cfg(target_os = "macos")]
            crate::macos::set_badge(mentions);
        }
    }

    pub fn frame(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if std::env::var_os("MMFAST_FRAMES").is_some() {
            let cause = ctx.repaint_causes();
            log::info!("frame {} at {:.2}s causes {:?}", ctx.cumulative_frame_nr(), ctx.input(|i| i.time), cause);
        }
        self.frame_start(&ctx);
        self.scrolling.apply(&ctx);
        self.tick(&ctx);
        crate::ui::show(self, ui);
        self.apply_actions(&ctx);
        self.frame_end();
        if let Some(mut script) = self.script.take() {
            let done = script.step(self, &ctx);
            if !done {
                self.script = Some(script);
            }
        }
    }
}

impl fastframe_shell::Resident for App {
    fn closed(&self) -> fastframe_shell::Closed {
        if self.quit_requested || !self.keeps_running() {
            fastframe_shell::Closed::Quit
        } else {
            fastframe_shell::Closed::Hide
        }
    }

    fn window_gone(&mut self) {
        self.has_window = false;
        self.wants_show = false;
        self.u.images = Images::default();
        self.u.lightbox = None;
        self.u.menu = None;
        self.u.picker = None;
        log::info!("window closed; running in the menu bar");
    }

    fn headless_frame(&mut self, ctx: &egui::Context) -> fastframe_shell::Headless {
        self.tick(ctx);
        if self.quit_requested {
            fastframe_shell::Headless::Quit
        } else if self.wants_show {
            fastframe_shell::Headless::Show
        } else {
            fastframe_shell::Headless::Wait
        }
    }

    fn shutdown(&mut self) {
        self.config.save(&self.dirs);
    }
}

/// One window over the app; dropping it hands the app back to the shell.
pub struct Window {
    pub app: fastframe_shell::Held<App>,
}

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.frame(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(id: &str, at: i64) -> Post {
        Post { id: id.into(), create_at: at, channel_id: "c".into(), ..Default::default() }
    }

    #[test]
    fn server_echo_replaces_the_pending_post_in_place() {
        let mut v = ChannelView::default();
        v.upsert(post("a", 1));
        let mut pending = post("me:100", 100);
        pending.pending = true;
        pending.pending_post_id = "me:100".into();
        v.upsert(pending);
        let mut confirmed = post("real", 100);
        confirmed.pending_post_id = "me:100".into();
        v.upsert(confirmed);
        let ids: Vec<&str> = v.posts.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "real"]);
    }

    #[test]
    fn late_posts_sort_by_time_and_pending_stays_last() {
        let mut v = ChannelView::default();
        v.upsert(post("a", 1));
        let mut pending = post("me:5", 5);
        pending.pending = true;
        v.upsert(pending);
        v.upsert(post("c", 3));
        v.upsert(post("b", 2));
        let ids: Vec<&str> = v.posts.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, vec!["a", "b", "c", "me:5"]);
    }

    #[test]
    fn edits_replace_without_duplicating() {
        let mut v = ChannelView::default();
        v.upsert(post("a", 1));
        let mut edited = post("a", 1);
        edited.message = "new".into();
        v.upsert(edited);
        assert_eq!(v.posts.len(), 1);
        assert_eq!(v.posts[0].message, "new");
    }
}
