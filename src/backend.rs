//! The interface's handle to a tokio runtime on its own thread.
//!
//! The two sides talk only through [`Command`] (UI to runtime) and [`Event`]
//! (runtime to UI). Every event wakes the window with `request_repaint`, so
//! the interface stays event-driven and idles at zero CPU. Nothing here ever
//! runs on the UI thread.

use crate::api::{self, Api};
use crate::model::*;
use crate::store::Store;
use crate::ws::{self, WsEvent, WsOut};
use anyhow::Result;
use egui::ColorImage;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{Semaphore, mpsc};

pub enum LoginMethod {
    Password { login_id: String, password: String, mfa: String },
    Token(String),
}

pub enum UploadData {
    Path(PathBuf),
    Bytes(Vec<u8>),
}

pub enum Command {
    Login { server: String, method: LoginMethod },
    /// Find the server's single sign-on page.
    SsoStart { server: String },
    Start { server: String, token: String },
    Logout,
    OpenChannel { channel_id: String },
    LoadOlder { channel_id: String, before_id: String },
    OpenThread { root_id: String },
    Send { channel_id: String, root_id: String, message: String, pending_id: String, file_ids: Vec<String> },
    Edit { post_id: String, message: String },
    Delete { post_id: String },
    React { post_id: String, emoji: String, add: bool },
    Pin { post_id: String, pinned: bool },
    ViewChannel { channel_id: String },
    Upload { local_id: u64, channel_id: String, name: String, data: UploadData },
    Image(ImageKey),
    Search { seq: u64, team_id: String, terms: String, server: bool },
    LoadPinned { channel_id: String },
    LoadStats { channel_id: String },
    PatchHeader { channel_id: String, header: String },
    LoadThreads { team_id: String },
    MarkThreadRead { team_id: String, thread_id: String },
    Download { file: FileInfo, open: bool },
    Autocomplete { team_id: String, channel_id: String, prefix: String },
    Typing { channel_id: String, parent_id: String },
    Resync,
    LoadCategories { team_id: String },
    SetFavorite { team_id: String, channel_id: String, favorite: bool },
    SetMuted { channel_id: String, muted: bool },
    BrowseChannels { team_id: String, term: String },
    JoinChannel { channel_id: String },
    LeaveChannel { channel_id: String },
    OpenDm { user_id: String },
    LoadEmoji,
    /// A permalink: fetch the post, then jump to it.
    JumpPost { post_id: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostsMode {
    /// The newest window of the channel; replaces what is shown.
    Latest,
    /// Updates to posts that may already be shown.
    Merge,
    /// An older page, prepended.
    Older,
}

pub enum Event {
    LoginFailed(String),
    SsoUrl(String),
    LoggedIn { server: String, token: String },
    Snapshot { snap: Snapshot, from_cache: bool },
    ChannelPosts { channel_id: String, posts: Vec<Post>, mode: PostsMode, reached_start: Option<bool> },
    ThreadPosts { root_id: String, posts: Vec<Post> },
    PostUpsert { post: Post, mentions_me: bool, live: bool },
    PostDeleted { post: Post },
    Reaction { reaction: Reaction, added: bool },
    Typing { channel_id: String, parent_id: String, user_id: String },
    ChannelUpdated(Channel),
    ChannelViewed { channel_id: String },
    Users(Vec<User>),
    Image { key: ImageKey, image: Option<Arc<ColorImage>> },
    /// Frames of an animated GIF and how long each shows (seconds).
    Animation { key: ImageKey, frames: Option<Vec<(Arc<ColorImage>, f32)>> },
    SearchResults { seq: u64, posts: Vec<Post>, files: Vec<FileInfo>, server: bool, error: Option<String> },
    Pinned { channel_id: String, posts: Vec<Post> },
    Stats(ChannelStats),
    Threads { team_id: String, threads: UserThreads },
    ThreadUpdated,
    UploadDone { local_id: u64, result: Result<FileInfo, String> },
    Downloaded { name: String, result: Result<PathBuf, String> },
    Connection { connected: bool, detail: String },
    Reconnected,
    SendFailed { pending_id: String, error: String },
    Autocomplete { users: Vec<User> },
    Categories { team_id: String, categories: Vec<Category> },
    MemberUpdated(ChannelMember),
    BrowseResults { term: String, channels: Vec<Channel> },
    /// A channel we just joined or created (a new DM); open it.
    ChannelJoined { channel: Channel, member: ChannelMember },
    ChannelLeft { channel_id: String },
    CustomEmoji(Vec<(String, String)>),
    JumpPost(Post),
    Toast(String),
    Unauthorized,
}

pub struct Backend {
    commands: mpsc::UnboundedSender<Command>,
    events: std::sync::mpsc::Receiver<Event>,
}

impl Backend {
    pub fn spawn(waker: fastframe_shell::Waker, data_dir: PathBuf, cache_dir: PathBuf) -> Self {
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("mmfast-runtime")
            .enable_all()
            .build()
            .expect("unable to start the async runtime");
        std::thread::Builder::new()
            .name("mmfast-backend".into())
            .spawn(move || {
                runtime.block_on(run(command_rx, Emitter { tx: event_tx, waker }, data_dir, cache_dir));
                runtime.shutdown_timeout(Duration::from_secs(2));
            })
            .expect("unable to start the backend thread");
        Self { commands: command_tx, events: event_rx }
    }

    pub fn send(&self, command: Command) {
        let _ = self.commands.send(command);
    }

    pub fn poll(&self) -> impl Iterator<Item = Event> + '_ {
        self.events.try_iter()
    }
}

#[derive(Clone)]
struct Emitter {
    tx: std::sync::mpsc::Sender<Event>,
    /// Repaints whichever window exists; while none does, the headless loop
    /// picks the event up on its next tick.
    waker: fastframe_shell::Waker,
}

impl Emitter {
    fn emit(&self, event: Event) {
        let _ = self.tx.send(event);
        self.waker.wake();
    }
}

struct Session {
    api: Api,
    me: Mutex<User>,
    store: Mutex<Store>,
    out: Emitter,
    files_dir: PathBuf,
    known_users: Mutex<HashSet<String>>,
    images: Semaphore,
    ws_out: mpsc::UnboundedSender<WsOut>,
    ws_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

fn server_slug(server: &str) -> String {
    api::normalize_server(server)
        .split("://")
        .nth(1)
        .unwrap_or("server")
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' })
        .collect()
}

async fn run(mut commands: mpsc::UnboundedReceiver<Command>, out: Emitter, data_dir: PathBuf, cache_dir: PathBuf) {
    let mut session: Option<Arc<Session>> = None;
    while let Some(command) = commands.recv().await {
        match command {
            Command::Login { server, method } => {
                let out = out.clone();
                tokio::spawn(async move {
                    let result = match method {
                        LoginMethod::Password { login_id, password, mfa } => {
                            Api::login(&server, &login_id, &password, &mfa).await.map(|(t, _)| t)
                        }
                        LoginMethod::Token(token) => {
                            let token = token.trim().to_string();
                            Api::new(&server, &token).me().await.map(|_| token)
                        }
                    };
                    match result {
                        Ok(token) => out.emit(Event::LoggedIn { server: api::normalize_server(&server), token }),
                        Err(err) => out.emit(Event::LoginFailed(format!("{err:#}"))),
                    }
                });
            }
            Command::SsoStart { server } => {
                let out = out.clone();
                tokio::spawn(async move {
                    match Api::sso_url(&server, crate::app::SSO_REDIRECT).await {
                        Ok(url) => out.emit(Event::SsoUrl(url)),
                        Err(err) => out.emit(Event::LoginFailed(format!("{err:#}"))),
                    }
                });
            }
            Command::Start { server, token } => {
                if let Some(old) = session.take() {
                    old.stop();
                }
                match Session::start(&server, &token, out.clone(), &data_dir, &cache_dir) {
                    Ok(s) => session = Some(s),
                    Err(err) => out.emit(Event::Toast(format!("could not open the local cache: {err:#}"))),
                }
            }
            Command::Logout => {
                if let Some(s) = session.take() {
                    s.stop();
                    tokio::spawn(async move {
                        let _ = s.api.logout().await;
                    });
                }
            }
            other => {
                if let Some(s) = &session {
                    let s = s.clone();
                    tokio::spawn(async move {
                        if let Err(err) = s.handle(other).await {
                            s.report(err);
                        }
                    });
                }
            }
        }
    }
}

impl Session {
    fn start(server: &str, token: &str, out: Emitter, data_dir: &Path, cache_dir: &Path) -> Result<Arc<Self>> {
        let slug = server_slug(server);
        let store = Store::open(&data_dir.join("servers").join(&slug).join("cache.db"))?;
        let files_dir = cache_dir.join("servers").join(&slug).join("files");
        std::fs::create_dir_all(&files_dir)?;
        let cached = store.snapshot();
        let known = store.known_users();
        let (ws_out, ws_out_rx) = mpsc::unbounded_channel();
        let session = Arc::new(Self {
            api: Api::new(server, token),
            me: Mutex::new(cached.as_ref().map(|s| s.me.clone()).unwrap_or_default()),
            store: Mutex::new(store),
            out,
            files_dir,
            known_users: Mutex::new(known),
            images: Semaphore::new(6),
            ws_out,
            ws_task: Mutex::new(None),
        });
        if let Some(snap) = cached {
            session.out.emit(Event::Snapshot { snap, from_cache: true });
        }
        let s = session.clone();
        tokio::spawn(async move {
            if let Err(err) = s.sync_snapshot().await {
                s.report(err);
                return;
            }
            s.clone().start_ws(ws_out_rx);
        });
        Ok(session)
    }

    fn stop(&self) {
        if let Some(task) = self.ws_task.lock().unwrap().take() {
            task.abort();
        }
    }

    fn me_id(&self) -> String {
        self.me.lock().unwrap().id.clone()
    }

    fn report(&self, err: anyhow::Error) {
        if api::is_unauthorized(&err) {
            self.out.emit(Event::Unauthorized);
        } else {
            log::warn!("{err:#}");
            self.out.emit(Event::Toast(format!("{err:#}")));
        }
    }

    async fn sync_snapshot(&self) -> Result<()> {
        let me = self.api.me().await?;
        *self.me.lock().unwrap() = me.clone();
        let teams = self.api.my_teams().await?;
        let mut channels: Vec<Channel> = Vec::new();
        let mut members: Vec<ChannelMember> = Vec::new();
        let fetches = teams.iter().map(|t| async {
            let (c, m) = tokio::join!(self.api.my_channels(&t.id), self.api.my_members(&t.id));
            Ok::<_, anyhow::Error>((c?, m?))
        });
        let mut seen = HashSet::new();
        let mut seen_members = HashSet::new();
        for result in futures_util::future::join_all(fetches).await {
            let (c, m) = result?;
            for ch in c {
                if ch.delete_at == 0 && seen.insert(ch.id.clone()) {
                    channels.push(ch);
                }
            }
            for mm in m {
                if seen_members.insert(mm.channel_id.clone()) {
                    members.push(mm);
                }
            }
        }
        // Profiles changed while we were away (pictures, names).
        let started = api::now_ms();
        let last = self.store.lock().unwrap().get_kv("users_sync").and_then(|v| v.parse::<i64>().ok());
        if let Some(since) = last {
            let known: Vec<String> = self.known_users.lock().unwrap().iter().cloned().collect();
            if let Ok(changed) = self.api.users_changed_since(&known, since).await
                && !changed.is_empty()
            {
                self.store.lock().unwrap().save_users(&changed);
                self.out.emit(Event::Users(changed));
            }
        }
        self.store.lock().unwrap().set_kv("users_sync", &started.to_string());
        let mut wanted: Vec<String> = channels.iter().filter_map(|c| c.dm_partner(&me.id)).collect();
        wanted.push(me.id.clone());
        self.ensure_users(wanted).await;
        let mut snap = Snapshot { me, teams, channels, members, users: Vec::new() };
        {
            let mut store = self.store.lock().unwrap();
            store.save_snapshot(&snap)?;
            snap.users = store.snapshot().map(|s| s.users).unwrap_or_default();
        }
        self.out.emit(Event::Snapshot { snap, from_cache: false });
        Ok(())
    }

    /// Fetch users we have never seen; announce them to the UI.
    async fn ensure_users(&self, ids: impl IntoIterator<Item = String>) {
        let missing: Vec<String> = {
            let known = self.known_users.lock().unwrap();
            let mut set: Vec<String> = ids.into_iter().filter(|id| !id.is_empty() && !known.contains(id)).collect();
            set.sort();
            set.dedup();
            set
        };
        if missing.is_empty() {
            return;
        }
        match self.api.users_by_ids(&missing).await {
            Ok(users) => {
                self.store.lock().unwrap().save_users(&users);
                self.known_users.lock().unwrap().extend(users.iter().map(|u| u.id.clone()));
                self.out.emit(Event::Users(users));
            }
            Err(err) => log::warn!("user lookup failed: {err:#}"),
        }
    }

    async fn users_of(&self, posts: &[Post]) {
        let mut ids: Vec<String> = posts.iter().map(|p| p.user_id.clone()).collect();
        for p in posts {
            for r in &p.metadata.reactions {
                ids.push(r.user_id.clone());
            }
            if let Some(parts) = &p.participants {
                ids.extend(parts.iter().map(|u| u.id.clone()));
            }
        }
        self.ensure_users(ids).await;
    }

    fn start_ws(self: Arc<Self>, ws_out_rx: mpsc::UnboundedReceiver<WsOut>) {
        let Ok(url) = self.api.websocket_url() else { return };
        let (tx, mut rx) = mpsc::unbounded_channel();
        let token = self.api.token().to_string();
        let socket = tokio::spawn(ws::run(url, token, tx, ws_out_rx));
        let s = self.clone();
        let pump = tokio::spawn(async move {
            let mut first = true;
            while let Some(ev) = rx.recv().await {
                s.on_ws(ev, &mut first).await;
            }
        });
        *self.ws_task.lock().unwrap() = Some(tokio::spawn(async move {
            let _ = tokio::join!(socket, pump);
        }));
    }

    async fn on_ws(&self, ev: WsEvent, first: &mut bool) {
        match ev {
            WsEvent::Connected => {
                self.out.emit(Event::Connection { connected: true, detail: String::new() });
                if !*first {
                    if let Err(err) = self.sync_snapshot().await {
                        self.report(err);
                    }
                    self.out.emit(Event::Reconnected);
                }
                *first = false;
            }
            WsEvent::Disconnected(detail) => self.out.emit(Event::Connection { connected: false, detail }),
            WsEvent::Posted { post, mentions } => {
                let me = self.me_id();
                self.store.lock().unwrap().save_posts(std::slice::from_ref(&post));
                self.users_of(std::slice::from_ref(&post)).await;
                let mentions_me = mentions.iter().any(|m| m == &me);
                self.out.emit(Event::PostUpsert { post, mentions_me, live: true });
            }
            WsEvent::PostEdited(post) => {
                self.store.lock().unwrap().save_posts(std::slice::from_ref(&post));
                self.out.emit(Event::PostUpsert { post, mentions_me: false, live: false });
            }
            WsEvent::PostDeleted(post) => {
                self.store.lock().unwrap().delete_post(&post.id);
                self.out.emit(Event::PostDeleted { post });
            }
            WsEvent::Reaction { reaction, added } => {
                {
                    let mut store = self.store.lock().unwrap();
                    if let Some(mut p) = store.post(&reaction.post_id) {
                        p.metadata.reactions.retain(|r| {
                            !(r.user_id == reaction.user_id && r.emoji_name == reaction.emoji_name)
                        });
                        if added {
                            p.metadata.reactions.push(reaction.clone());
                        }
                        store.save_posts(&[p]);
                    }
                }
                self.ensure_users([reaction.user_id.clone()]).await;
                self.out.emit(Event::Reaction { reaction, added });
            }
            WsEvent::Typing { channel_id, parent_id, user_id } => {
                if user_id != self.me_id() {
                    self.ensure_users([user_id.clone()]).await;
                    self.out.emit(Event::Typing { channel_id, parent_id, user_id });
                }
            }
            WsEvent::ChannelViewed { channel_id } => self.out.emit(Event::ChannelViewed { channel_id }),
            WsEvent::ChannelUpdated(channel) => {
                self.store.lock().unwrap().save_channel(&channel);
                self.out.emit(Event::ChannelUpdated(channel));
            }
            WsEvent::Resync => {
                if let Err(err) = self.sync_snapshot().await {
                    self.report(err);
                }
            }
            WsEvent::UserUpdated(user) => {
                self.store.lock().unwrap().save_users(std::slice::from_ref(&user));
                self.out.emit(Event::Users(vec![user]));
            }
            WsEvent::ThreadUpdated => self.out.emit(Event::ThreadUpdated),
            WsEvent::MemberUpdated(member) => {
                if member.user_id == self.me_id() {
                    self.store.lock().unwrap().save_member(&member);
                    self.out.emit(Event::MemberUpdated(member));
                }
            }
            WsEvent::CategoriesChanged { team_id } => {
                if !team_id.is_empty()
                    && let Err(err) = self.load_categories(&team_id).await
                {
                    self.report(err);
                }
            }
        }
    }

    async fn handle(&self, command: Command) -> Result<()> {
        match command {
            Command::OpenChannel { channel_id } => self.open_channel(&channel_id).await?,
            Command::LoadOlder { channel_id, before_id } => {
                let mut posts = self.api.channel_posts(&channel_id, &format!("before={before_id}&per_page=60")).await?;
                posts.reverse();
                let reached = posts.len() < 60;
                self.store.lock().unwrap().save_posts(&posts);
                self.users_of(&posts).await;
                self.out.emit(Event::ChannelPosts { channel_id, posts, mode: PostsMode::Older, reached_start: Some(reached) });
            }
            Command::OpenThread { root_id } => {
                let cached = self.store.lock().unwrap().thread(&root_id);
                if cached.len() > 1 {
                    self.out.emit(Event::ThreadPosts { root_id: root_id.clone(), posts: cached });
                }
                let mut posts = self.api.thread(&root_id).await?;
                posts.sort_by_key(|p| p.create_at);
                self.store.lock().unwrap().save_posts(&posts);
                self.users_of(&posts).await;
                self.out.emit(Event::ThreadPosts { root_id, posts });
            }
            Command::Send { channel_id, root_id, message, pending_id, file_ids } => {
                match self.api.create_post(&channel_id, &root_id, &message, &pending_id, &file_ids).await {
                    Ok(post) => {
                        self.store.lock().unwrap().save_posts(std::slice::from_ref(&post));
                        self.out.emit(Event::PostUpsert { post, mentions_me: false, live: false });
                    }
                    Err(err) => self.out.emit(Event::SendFailed { pending_id, error: format!("{err:#}") }),
                }
            }
            Command::Edit { post_id, message } => {
                let post = self.api.edit_post(&post_id, &message).await?;
                self.store.lock().unwrap().save_posts(std::slice::from_ref(&post));
                self.out.emit(Event::PostUpsert { post, mentions_me: false, live: false });
            }
            Command::Delete { post_id } => self.api.delete_post(&post_id).await?,
            Command::React { post_id, emoji, add } => {
                let me = self.me_id();
                if add {
                    self.api.add_reaction(&me, &post_id, &emoji).await?
                } else {
                    self.api.remove_reaction(&me, &post_id, &emoji).await?
                }
            }
            Command::Pin { post_id, pinned } => {
                self.api.set_pinned(&post_id, pinned).await?;
                let post = self.api.post_by_id(&post_id).await?;
                self.store.lock().unwrap().save_posts(std::slice::from_ref(&post));
                self.out.emit(Event::PostUpsert { post, mentions_me: false, live: false });
            }
            Command::ViewChannel { channel_id } => self.api.view_channel(&channel_id).await?,
            Command::Upload { local_id, channel_id, name, data } => {
                let result = async {
                    let bytes = match data {
                        UploadData::Path(p) => tokio::fs::read(&p).await?,
                        UploadData::Bytes(b) => b,
                    };
                    self.api.upload(&channel_id, &name, bytes).await
                }
                .await
                .map_err(|e| format!("{e:#}"));
                self.out.emit(Event::UploadDone { local_id, result });
            }
            Command::Image(key) => self.load_image(key).await,
            Command::Search { seq, team_id, terms, server } => {
                if server {
                    let (posts, files) =
                        tokio::join!(self.api.search_posts(&team_id, &terms), self.api.search_files(&team_id, &terms));
                    let error = posts.as_ref().err().map(|e| format!("{e:#}"));
                    let posts = posts.unwrap_or_default();
                    self.store.lock().unwrap().save_posts(&posts);
                    self.users_of(&posts).await;
                    self.out.emit(Event::SearchResults {
                        seq,
                        posts,
                        files: files.unwrap_or_default(),
                        server: true,
                        error,
                    });
                } else {
                    let posts = self.store.lock().unwrap().search(&terms, 40);
                    self.out.emit(Event::SearchResults { seq, posts, files: Vec::new(), server: false, error: None });
                }
            }
            Command::LoadPinned { channel_id } => {
                let mut posts = self.api.pinned(&channel_id).await?;
                posts.sort_by_key(|p| std::cmp::Reverse(p.create_at));
                self.users_of(&posts).await;
                self.out.emit(Event::Pinned { channel_id, posts });
            }
            Command::LoadStats { channel_id } => {
                let stats = self.api.channel_stats(&channel_id).await?;
                self.out.emit(Event::Stats(stats));
            }
            Command::PatchHeader { channel_id, header } => {
                let channel = self.api.patch_channel_header(&channel_id, &header).await?;
                self.store.lock().unwrap().save_channel(&channel);
                self.out.emit(Event::ChannelUpdated(channel));
            }
            Command::LoadThreads { team_id } => {
                let me = self.me_id();
                let threads = self.api.user_threads(&me, &team_id).await?;
                let posts: Vec<Post> = threads.threads.iter().map(|t| t.post.clone()).collect();
                let mut ids: Vec<String> = threads.threads.iter().flat_map(|t| t.participants.iter().map(|u| u.id.clone())).collect();
                ids.extend(posts.iter().map(|p| p.user_id.clone()));
                self.ensure_users(ids).await;
                self.out.emit(Event::Threads { team_id, threads });
            }
            Command::MarkThreadRead { team_id, thread_id } => {
                let me = self.me_id();
                self.api.mark_thread_read(&me, &team_id, &thread_id).await?;
            }
            Command::Download { file, open } => {
                let result = self.download(&file).await;
                if let (true, Ok(path)) = (open, &result) {
                    let _ = open::that_detached(path);
                }
                self.out.emit(Event::Downloaded { name: file.name.clone(), result: result.map_err(|e| format!("{e:#}")) });
            }
            Command::Autocomplete { team_id, channel_id, prefix } => {
                let users = self.api.autocomplete_users(&team_id, &channel_id, &prefix).await?;
                self.store.lock().unwrap().save_users(&users);
                self.known_users.lock().unwrap().extend(users.iter().map(|u| u.id.clone()));
                let _ = prefix;
                self.out.emit(Event::Autocomplete { users });
            }
            Command::Typing { channel_id, parent_id } => {
                let _ = self.ws_out.send(WsOut::Typing { channel_id, parent_id });
            }
            Command::Resync => self.sync_snapshot().await?,
            Command::LoadCategories { team_id } => {
                let cached = self.store.lock().unwrap().get_kv(&format!("cats:{team_id}"));
                if let Some(categories) = cached.and_then(|j| serde_json::from_str(&j).ok()) {
                    self.out.emit(Event::Categories { team_id: team_id.clone(), categories });
                }
                self.load_categories(&team_id).await?;
            }
            Command::SetFavorite { team_id, channel_id, favorite } => {
                let me = self.me_id();
                let mut cats = self.api.categories(&team_id).await?;
                let fav = cats.iter().position(|c| c.kind == "favorites");
                let home = |c: &Category, ch: &Channel| match ch.kind.as_str() {
                    "D" | "G" => c.kind == "direct_messages",
                    _ => c.kind == "channels",
                };
                let channel = self.api.channel(&channel_id).await?;
                let mut changed = Vec::new();
                for (i, c) in cats.iter_mut().enumerate() {
                    let had = c.channel_ids.contains(&channel_id);
                    let want = if Some(i) == fav { favorite } else if favorite { false } else { had || home(c, &channel) };
                    if had != want {
                        if want {
                            c.channel_ids.insert(0, channel_id.clone());
                        } else {
                            c.channel_ids.retain(|id| id != &channel_id);
                        }
                        changed.push(c.clone());
                    }
                }
                if !changed.is_empty() {
                    self.api.update_categories(&me, &team_id, &changed).await?;
                }
                self.load_categories(&team_id).await?;
            }
            Command::SetMuted { channel_id, muted } => {
                let me = self.me_id();
                let mark = if muted { "mention" } else { "all" };
                self.api.set_notify_props(&channel_id, &me, serde_json::json!({ "mark_unread": mark })).await?;
                let member = self.api.member(&channel_id).await?;
                self.store.lock().unwrap().save_member(&member);
                self.out.emit(Event::MemberUpdated(member));
            }
            Command::BrowseChannels { team_id, term } => {
                let mut channels = self.api.browse_channels(&team_id, &term).await?;
                channels.retain(|c| c.delete_at == 0);
                channels.sort_by_key(|c| c.display_name.to_lowercase());
                self.out.emit(Event::BrowseResults { term, channels });
            }
            Command::JoinChannel { channel_id } => {
                let me = self.me_id();
                self.api.join_channel(&channel_id, &me).await?;
                self.joined(&channel_id).await?;
            }
            Command::LeaveChannel { channel_id } => {
                let me = self.me_id();
                self.api.leave_channel(&channel_id, &me).await?;
                self.out.emit(Event::ChannelLeft { channel_id });
            }
            Command::JumpPost { post_id } => {
                let post = self.api.post_by_id(&post_id).await?;
                self.users_of(std::slice::from_ref(&post)).await;
                self.out.emit(Event::JumpPost(post));
            }
            Command::LoadEmoji => {
                let cached = self.store.lock().unwrap().get_kv("emoji");
                if let Some(list) = cached.and_then(|j| serde_json::from_str(&j).ok()) {
                    self.out.emit(Event::CustomEmoji(list));
                }
                let list = self.api.custom_emoji().await?;
                if let Ok(j) = serde_json::to_string(&list) {
                    self.store.lock().unwrap().set_kv("emoji", &j);
                }
                self.out.emit(Event::CustomEmoji(list));
            }
            Command::OpenDm { user_id } => {
                let me = self.me_id();
                let channel = self.api.direct_channel(&me, &user_id).await?;
                self.ensure_users([user_id]).await;
                self.joined(&channel.id).await?;
            }
            Command::Login { .. } | Command::SsoStart { .. } | Command::Start { .. } | Command::Logout => {}
        }
        Ok(())
    }

    async fn load_categories(&self, team_id: &str) -> Result<()> {
        let categories = self.api.categories(team_id).await?;
        if let Ok(j) = serde_json::to_string(&categories) {
            self.store.lock().unwrap().set_kv(&format!("cats:{team_id}"), &j);
        }
        self.out.emit(Event::Categories { team_id: team_id.to_string(), categories });
        Ok(())
    }

    /// After joining or creating a channel: fetch it and our membership, save, announce.
    async fn joined(&self, channel_id: &str) -> Result<()> {
        let (channel, member) = tokio::join!(self.api.channel(channel_id), self.api.member(channel_id));
        let (channel, member) = (channel?, member?);
        {
            let store = self.store.lock().unwrap();
            store.save_channel(&channel);
            store.save_member(&member);
        }
        self.out.emit(Event::ChannelJoined { channel, member });
        Ok(())
    }

    async fn open_channel(&self, channel_id: &str) -> Result<()> {
        // 1. Whatever we have, this instant.
        let (cached, newest_cached, last_sync) = {
            let store = self.store.lock().unwrap();
            (
                store.recent_roots(channel_id, 60),
                store.newest_root_at(channel_id),
                store.get_kv(&format!("sync:{channel_id}")).and_then(|v| v.parse::<i64>().ok()),
            )
        };
        if !cached.is_empty() {
            self.out.emit(Event::ChannelPosts {
                channel_id: channel_id.to_string(),
                posts: cached,
                mode: PostsMode::Latest,
                reached_start: None,
            });
        }
        let started = api::now_ms();

        // 2. The newest page from the server.
        let mut latest = self.api.channel_posts(channel_id, "page=0&per_page=60").await?;
        latest.reverse();
        let full_page = latest.len() >= 60;
        let gap = full_page
            && match (latest.first(), newest_cached) {
                (Some(oldest_new), Some(newest_old)) => oldest_new.create_at > newest_old,
                _ => false,
            };
        {
            let mut store = self.store.lock().unwrap();
            if gap && let Some(first) = latest.first() {
                store.forget_older(channel_id, first.create_at);
            }
            store.save_posts(&latest);
        }
        self.users_of(&latest).await;
        let reached_start = if newest_cached.is_none() || gap { Some(!full_page) } else { None };
        self.out.emit(Event::ChannelPosts {
            channel_id: channel_id.to_string(),
            posts: latest,
            mode: if gap || newest_cached.is_none() { PostsMode::Latest } else { PostsMode::Merge },
            reached_start,
        });

        // 3. Edits and deletions to older cached posts since the last visit.
        if let Some(since) = last_sync
            && !gap
        {
            let changed = self.api.channel_posts(channel_id, &format!("since={since}")).await.unwrap_or_default();
            let (deleted, live): (Vec<Post>, Vec<Post>) = changed.into_iter().partition(|p| p.delete_at > 0);
            {
                let mut store = self.store.lock().unwrap();
                store.save_posts(&live);
                for p in &deleted {
                    store.delete_post(&p.id);
                }
            }
            for post in deleted {
                self.out.emit(Event::PostDeleted { post });
            }
            if !live.is_empty() {
                self.users_of(&live).await;
                self.out.emit(Event::ChannelPosts {
                    channel_id: channel_id.to_string(),
                    posts: live.into_iter().filter(|p| p.root_id.is_empty()).collect(),
                    mode: PostsMode::Merge,
                    reached_start: None,
                });
            }
        }
        self.store.lock().unwrap().set_kv(&format!("sync:{channel_id}"), &started.to_string());
        Ok(())
    }

    async fn load_image(&self, key: ImageKey) {
        let _permit = self.images.acquire().await;
        let path = self.files_dir.join(key.cache_name());
        let bytes = match tokio::fs::read(&path).await {
            Ok(b) => Some(b),
            Err(_) => match self.api.file_bytes(&key).await {
                Ok(b) => {
                    let _ = tokio::fs::write(&path, &b).await;
                    Some(b)
                }
                Err(err) => {
                    log::warn!("image {key:?}: {err:#}");
                    None
                }
            },
        };
        if let ImageKey::Animated(_) = key {
            let frames = match bytes {
                Some(b) => tokio::task::spawn_blocking(move || decode_gif(&b, 640)).await.ok().flatten(),
                None => None,
            };
            self.out.emit(Event::Animation { key, frames });
            return;
        }
        let max = match key {
            ImageKey::Avatar { .. } | ImageKey::TeamIcon { .. } | ImageKey::Emoji(_) => 96,
            ImageKey::Preview(_) => 1200,
            ImageKey::Full(_) | ImageKey::Animated(_) => 4096,
        };
        let image = match bytes {
            Some(b) => tokio::task::spawn_blocking(move || decode(&b, max)).await.ok().flatten(),
            None => None,
        };
        self.out.emit(Event::Image { key, image: image.map(Arc::new) });
    }

    async fn download(&self, file: &FileInfo) -> Result<PathBuf> {
        let bytes = self.api.download(&file.id).await?;
        let dir = directories::UserDirs::new()
            .and_then(|d| d.download_dir().map(Path::to_path_buf))
            .unwrap_or_else(std::env::temp_dir);
        let mut path = dir.join(&file.name);
        let (stem, ext) = match file.name.rsplit_once('.') {
            Some((s, e)) => (s.to_string(), format!(".{e}")),
            None => (file.name.clone(), String::new()),
        };
        let mut n = 1;
        while path.exists() {
            path = dir.join(format!("{stem} ({n}){ext}"));
            n += 1;
        }
        tokio::fs::write(&path, bytes).await?;
        Ok(path)
    }
}

/// Every frame of a GIF (up to 300), scaled to fit `max`, with its delay.
fn decode_gif(bytes: &[u8], max: u32) -> Option<Vec<(Arc<ColorImage>, f32)>> {
    use image::AnimationDecoder;
    let decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes)).ok()?;
    let mut out = Vec::new();
    for frame in decoder.into_frames().take(300) {
        let frame = frame.ok()?;
        let (num, den) = frame.delay().numer_denom_ms();
        // Browsers treat very short delays as 100 ms; so do we.
        let secs = if den == 0 { 0.1 } else { num as f32 / den as f32 / 1000.0 };
        let secs = if secs < 0.02 { 0.1 } else { secs };
        let img = image::DynamicImage::ImageRgba8(frame.into_buffer());
        let img = if img.width() > max || img.height() > max { img.thumbnail(max, max) } else { img };
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        out.push((Arc::new(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())), secs));
    }
    if out.is_empty() { None } else { Some(out) }
}

fn decode(bytes: &[u8], max: u32) -> Option<ColorImage> {
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > max || img.height() > max { img.thumbnail(max, max) } else { img };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}
