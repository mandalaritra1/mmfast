# mattermost-fast: plan

## Status (2026-10-07)

Phases 0 to 7 are built and checked against a local test server
(Mattermost 11.11.1 Team Edition in Docker, seeded by `dev/seed.py`).
The app is `mmfast`; see README.md and AGENTS.md.

Verified by scripted runs (`--script`, screenshots) and API checks:
sign-in by password and by token (keychain saved, cleared on sign-out);
sidebar with unread and mention badges across two teams; cached start;
live posts, thread replies and unread counts over the websocket; sending
(optimistic, replaced by the server's echo); edit, reaction and file upload
confirmed on the server; threads panel and Followed threads; channel header
popup and pinned list; cached and server search with term highlighting and
jump-to-message (pages back through history); lightbox; Cmd+K switcher;
dark theme. 9 unit tests (markup, cache, optimistic updates, completion).

Head to head on the CERN server, same account, warm caches, three fresh
launches each (medians; `bench/bench.py`, raw numbers in `bench/`):

| | Mattermost Desktop 6.3.0 | mmfast | |
|---|---:|---:|---|
| Main window created | 517 ms | 533 ms | same |
| Startup finished (CPU settles) | 4.64 s | 0.82 s | 5.7x faster |
| CPU spent starting | 4.38 s | 0.59 s | 7.4x less |
| Memory footprint, settled | 669 MB | 212 MB | 3.2x less |
| Processes / threads | 10 / 191 | 1 / 9 | |
| CPU while idle, 30 s | 1.25 s | 0.08 s | 16x less |
| App size | 241 MB | 15 MB | 16x smaller |

"Window created" is the window server's first record of the main window,
not first paint (no screen-recording permission). "Startup finished" is
when all of the app's processes together drop below 5% of a core for 2 s.

Added the same day, on the follow-up list (verified on the test server,
CERN for the first): browser single sign-on via `mmauth://`; muted channels,
server sidebar categories and favorites, browse/join/leave, new DMs; custom
emoji, profile cards, in-app permalinks, Dock badge; background mode in the
menu bar; TeX-style display math, code highlighting, animated GIFs. Idle CPU
is still zero and memory 158 MB on the test server.

Not yet done: the CERN SSO round trip on a real account (the flow is tested
locally), Linux/Windows builds (code is cross-platform, untested),
self-update, several servers at once.

A native Mattermost client built the way Spotifast and ZapFast are built.
Scope is deliberately small: chat, threads, images, attachments, the channel
header, and search. No calls, boards, playbooks, plugins, integrations
marketplace, or apps framework.

---

## 1. What Spotifast is, and why it is fast

Spotifast (github.com/crmne/spotifast) is a Spotify client in Rust,
~88k lines. It belongs to a family of apps by the same author:

| Repo | What it is | Relevance here |
|---|---|---|
| `crmne/spotifast` | Spotify client (egui + librespot) | the app you pointed at |
| `crmne/zapfast` | WhatsApp client (egui + whatsapp-rust), ~106k lines | **the real template**: a chat app with a message list, media, attachments, search, SQLite archive |
| `crmne/fastframe` | shared crates both apps use | we depend on it directly |

Their measured numbers: ZapFast 150-200 MB idle RAM vs 1.13 GB for WhatsApp
Web, 152 ms to first window vs 528 ms. Spotifast: 100-250 MB vs 600 MB-1 GB.
Mattermost Desktop is Electron, so it is in the WhatsApp Web class.

The speed comes from a handful of architectural choices, not tricks:

1. **No browser engine.** The UI is egui (immediate-mode, GPU-drawn) on the
   `glow` OpenGL backend. No Chromium, no JS heap, no DOM, no React/Redux
   store of the whole server state. This alone is most of the RAM gap.
2. **Event-driven repaint, idle at 0% CPU.** egui only redraws when
   something happens. The network side never polls the UI: it sends an
   `Event` and calls a `Waker` (`fastframe_shell::Waker`, wraps
   `request_repaint`). No timers spinning.
3. **Two threads with a hard wall between them.** A tokio runtime (2 worker
   threads) on its own OS thread owns all networking, disk and decoding.
   The UI talks to it only through `Command` (UI → backend, tokio mpsc) and
   `Event` (backend → UI, std mpsc). Nothing blocking ever runs on the UI
   thread.
4. **Views never mutate state.** `ui/*` draws and pushes `Action`s;
   `app.rs` applies them after the frame. This keeps frames cheap and
   avoids borrow fights.
5. **Local store first, network second.** ZapFast keeps a SQLite archive
   (WAL mode, paged `LIMIT ?` queries); Spotifast keeps an on-disk playlist
   cache. On launch the window shows cached content immediately, then the
   network catches up.
6. **Optimistic UI.** A control shows its result the instant it is used
   (a sent message is already in the list); the backend makes it true and a
   stale server answer is not allowed to undo it.
7. **Virtualized lists.** `ScrollArea::show_rows` / `show_viewport`: only
   rows on screen are laid out.
8. **Memory discipline for images.** egui caches bytes, pixels and GPU
   texture for every image forever. ZapFast's `image_cache.rs` tracks which
   URIs were drawn on which frame and forgets anything beyond 96 resident
   images once it scrolls away. Decoding happens off the UI thread.
9. **Small binary, one TLS stack.** rustls only (no OpenSSL), `glow` not
   `wgpu`, release profile `lto = "thin"`, `codegen-units = 1`,
   `strip = true`, `panic = "abort"`.
10. **Shared, boring infrastructure (fastframe).** Fonts, colour emoji drawn
    off-thread, themes, tray, single-instance, platform-correct scrolling,
    logging, self-update. All already solved; we reuse it.

How it was built, process-wise: trunk-based on `main`, one topic per
commit, an `AGENTS.md` that encodes the architecture rules above for coding
agents, a `demo` cargo feature with fake data for deterministic UI work and
screenshots, and a "fix upstream, pin a fork" rule for egui/winit.

---

## 2. Target product

### In scope

| Area | What it means |
|---|---|
| **Sign-in** | One server, one account to start. Token stored in the OS keychain. |
| **Sidebar** | Teams; channels grouped Favorites / Channels / Direct Messages; unread bold, mention count badge; Ctrl+K channel switcher. |
| **Channel view** | Virtualized post list, consecutive-post grouping, date separators, "new messages" line, load older on scroll-up, Mattermost markdown (incl. code blocks, tables, @mentions, ~channel links, :emoji:, links), edited/deleted markers, reactions shown (add/remove is cheap, include it). |
| **Channel header** | Top bar: name, member count, header text rendered as markdown, purpose on hover/expand. Edit header (if permitted). Pinned posts list if it is cheap (one endpoint). |
| **Composer** | Enter sends, Shift+Enter newline, edit last with Up, @-mention and ~channel autocomplete, emoji autocomplete, paste/drag-drop files. |
| **Threads** | Reply count under a root post; right panel showing the thread with its own composer; collapsed-reply-threads "Threads" view (followed threads, unread). |
| **Images** | Inline thumbnails sized from file metadata (no layout jump), click → lightbox with zoom/pan, GIF animation, link-preview images (OpenGraph) optional. |
| **Attachments** | Upload (picker, drag-drop, clipboard paste) with progress; file cards with name/size/type; download to Downloads folder, open with system app. |
| **Search** | Server message search with Mattermost syntax (`from:`, `in:`, `before:`, `after:`, `on:`, quotes, `-exclude`), file search, results with context and jump-to-post; instant local results from the cache while the server answers. |
| **Basics** | Desktop notifications for mentions/DMs, tray, light/dark themes, keyboard shortcuts. |

### Out of scope

Calls, voice/video, Boards, Playbooks, plugins and their UI, slash-command
app forms, interactive message buttons (render as plain text), custom
statuses editing, admin/system console, team management, user settings
beyond what this app needs, multiple servers (later, ZapFast's multi-account
pattern shows how).

### One thing worth adding for you

CERN's Mattermost renders LaTeX (`$...$` and ```` ```latex ```` blocks). v1
shows the source in a code style; v2 can render it natively (a small math
layout engine to egui shapes, or rasterize via a bundled renderer). Flagged
as optional.

---

## 3. Mattermost API surface we need

All REST v4 with `Authorization: Bearer <token>`, plus one WebSocket.

| Need | Endpoint |
|---|---|
| Login (local accounts) | `POST /api/v4/users/login` (`login_id`, `password`, `token` for MFA); session token in `Token` header |
| Me / teams / channels | `GET /users/me`, `GET /users/me/teams`, `GET /users/me/teams/{tid}/channels`, `GET /users/me/teams/{tid}/channels/members` (unread + mention counts) |
| Posts | `GET /channels/{cid}/posts?per_page&before&after&since` (`since` = delta catch-up after reconnect) |
| Send / edit / delete | `POST /posts` (with `root_id`, `file_ids`, `pending_post_id` for optimistic dedupe), `PUT /posts/{id}/patch`, `DELETE /posts/{id}` |
| Read state | `POST /channels/members/me/view` |
| Users | `POST /users/ids` (batch), `GET /users/{id}/image` (avatars, disk-cached), `GET /users/autocomplete` |
| Threads | `GET /posts/{id}/thread`, `GET /users/{uid}/teams/{tid}/threads`, `PUT .../threads/{id}/read/{ts}`, follow/unfollow |
| Files | `POST /files` (multipart, `channel_id`), `GET /files/{id}/thumbnail`, `/preview`, `/files/{id}`; file infos come inside `post.metadata.files` |
| Channel header | channel object `header`, `purpose`; `GET /channels/{cid}/stats` (member count); `PUT /channels/{cid}/patch`; `GET /channels/{cid}/pinned` |
| Search | `POST /teams/{tid}/posts/search` (`terms`, `is_or_search`, `page`, `per_page`), `POST /teams/{tid}/files/search` |
| Reactions / emoji | `POST /reactions`, `DELETE /users/me/posts/{pid}/reactions/{name}`, `GET /emoji` (custom emoji, cached) |
| Live events | `wss://<server>/api/v4/websocket`, `authentication_challenge`, then `posted`, `post_edited`, `post_deleted`, `reaction_added/removed`, `typing`, `channel_viewed`, `channel_updated` (header changes), `user_updated`, `thread_updated`, `hello`; track `seq`, reconnect with backoff, then `since`-catch-up per open channel |

**Auth is the riskiest unknown.** If the target server is CERN's
(`mattermost.web.cern.ch`), login goes through CERN SSO, not a password.
Options, in order of preference, to be settled in Phase 0:

1. Personal access token, if the server allows users to create them.
2. The desktop-app SSO handoff: open the system browser on the server's
   desktop login page, receive the token back via a custom URL scheme or a
   loopback redirect. Must be checked against the target server version
   before relying on it.
3. Fallback that always works: paste the `MMAUTHTOKEN` cookie from a
   browser session (documented in-app, stored in the keychain).

---

## 4. Architecture (mirrors ZapFast)

```
mattermost-fast/
  Cargo.toml             # eframe(glow) + egui + fastframe-*, tokio, reqwest(rustls),
                         # tokio-websockets, rusqlite(bundled, FTS5), pulldown-cmark,
                         # image, rfd, arboard, keyring-core, notify-rust
  AGENTS.md              # the rules in section 1, adapted
  src/
    main.rs              # fastframe_shell::Shell, single instance, logging
    app.rs               # App state, applies Actions, drains Events each frame
    model.rs             # our own types: Team, Channel, Post, User, FileInfo, Thread
    backend.rs           # UI handle: Command/Event channels, Waker, spawns runtime
    backend/
      worker.rs          # owns API client, WS, store, downloads; translates JSON -> model
      api.rs             # typed REST v4 client (reqwest, rustls, gzip, http2)
      ws.rs              # websocket, seq tracking, reconnect + catch-up
      files.rs           # uploads w/ progress, thumbnail/preview/avatar disk cache
    store.rs             # SQLite (WAL): teams, channels, members, posts, users,
                         # file_infos, reactions; FTS5 table over post text
    markup.rs            # Mattermost markdown -> egui LayoutJob (+ mentions, emoji)
    image_cache.rs       # resident-window eviction (copy ZapFast's approach)
    auth.rs              # login flows, keychain
    notify.rs            # desktop notifications
    demo.rs              # fake server state for --demo (UI work + screenshots)
    ui/
      sidebar.rs  switcher.rs  channel.rs  header.rs  post.rs
      composer.rs thread.rs    threads.rs  search.rs  lightbox.rs
      login.rs    widgets.rs   keys.rs
```

Rules carried over verbatim:

- UI thread never blocks; all I/O on the backend runtime.
- Views emit `Action`s; `app.rs` mutates state after drawing.
- Every backend event wakes the window; nothing polls.
- Open a channel = render from SQLite this frame, then fetch `since` the
  newest cached post. Switching channels never waits on the network.
- Send = append a pending post with a client `pending_post_id` immediately;
  the WS `posted` echo or the REST answer replaces it in place.
- Lists are virtualized; post heights are cached per (post, width) so
  `show_viewport` can skip layout of off-screen rows.
- Images: thumbnails from disk cache, decoded on the runtime, textures
  evicted when off-screen beyond a resident budget.
- Memory: keep only open channels' posts in RAM (a window around the
  viewport); SQLite holds the rest.

Search design: as you type, query the local FTS5 index over cached posts
(instant, partial), and fire the server search on Enter / debounce; merge
results, server results win, label which came from where.

---

## 5. Phases

Each phase ends with something runnable. Effort sizes are rough, assuming
agent-assisted work in the evenings.

| # | Phase | Deliverable | Size |
|---|---|---|---|
| 0 | **Spike** | Confirm target server, version, and an auth method that works. A ~200-line CLI that logs in, lists channels, prints a channel's last 50 posts, and tails the websocket. | 1-2 days |
| 1 | **Skeleton** | Cargo project on fastframe + egui fork pins, Shell/tray/single-instance, backend thread with Command/Event/Waker, `--demo` mode with fake teams/channels/posts, theme. | 2-3 days |
| 2 | **Read path** | Login screen, sidebar with unread/mentions, virtualized channel view, markdown, avatars, SQLite cache + instant reopen, websocket live updates and reconnect catch-up, mark-as-read. | 1-2 weeks |
| 3 | **Write path** | Composer, optimistic send, edit/delete own, reactions, @/~/: autocomplete, Ctrl+K switcher. | 1 week |
| 4 | **Threads** | Reply counts, thread side panel with composer, Threads view (CRT), unread threads. | 3-5 days |
| 5 | **Images + attachments** | Inline thumbnails with no layout jump, lightbox, GIFs, uploads (picker/drag-drop/paste) with progress, download/open, image cache eviction. | 1 week |
| 6 | **Header + search** | Channel header bar (markdown header, purpose, member count, edit, pinned), message + file search with syntax, local FTS instant results, jump-to-post with highlight. | 1 week |
| 7 | **Polish + measure** | Notifications, keyboard shortcuts, unread jump, packaging (macOS .app first), a benchmark page vs Mattermost Desktop on the same account (idle RSS, first window, channel switch). | 3-5 days |

**Performance targets** (checked in Phase 7, watched from Phase 2):
idle RSS < 200 MB; first window < 200 ms; cached channel visible < 300 ms
from launch; channel switch from cache within one frame; 0% CPU when idle.

---

## 6. Risks

- **SSO login** (see section 3). Phase 0 exists to kill this risk first.
- **Markdown fidelity.** Mattermost markdown has extensions (tables, inline
  images, LaTeX, mention highlighting). Start with a strict subset through
  pulldown-cmark and fall back to plain text for anything unknown.
- **Text selection and IME in egui.** ZapFast already solved selection
  across messages; copy its approach rather than rediscover it.
- **Rate limits** on the server. Batch user lookups, cache avatars on disk
  with ETag, never refetch what SQLite has.
- **Server-side admin settings** may disable personal tokens, file uploads,
  or public links. Detect from `GET /config/client?format=old` and hide what
  is off.
- **Naming/branding.** Do not ship with Mattermost's name or logo as the
  product identity; call it something of our own and say "for Mattermost".

---

## 7. Decisions needed before Phase 0

1. Which server: CERN's `mattermost.web.cern.ch` or another?
2. Which platform first: macOS only to start, or all three from day one?
3. Do you want LaTeX rendering in scope for v1, or v2?
