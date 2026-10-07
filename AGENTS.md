# mmfast agent guide

mmfast is a small native Mattermost client in Rust and egui, built the way
Spotifast and ZapFast (github.com/crmne) are built. Read PLAN.md for the why.

## Product boundaries

- Chat, threads, images, attachments, the channel header, and search.
  No calls, boards, playbooks, plugins, integrations, or admin console.
- No browser engine, no telemetry, no hosted backend.
- Do not broaden a task into adjacent features or a general refactor.

## Architecture

- `src/ui/` draws views and pushes `app::Action`s; `src/app.rs` applies them
  after the frame. Views only mutate their own widget state (`UiState`).
- `src/backend.rs` is the UI's handle to a tokio runtime (2 workers) on its
  own thread. The two sides talk only through `Command` and `Event`; every
  event calls `request_repaint`, so nothing polls and an idle window costs no
  CPU. Network, disk, and image decoding never run on the UI thread.
- `src/api.rs` is the REST v4 client, `src/ws.rs` the websocket (reconnects
  with backoff; after a reconnect the worker resyncs with REST, so a missed
  event is never a missing message).
- `src/store.rs` is the SQLite cache (WAL) with an FTS5 index. Opening a
  channel shows cached posts at once, then fetches the newest page, detects
  a gap (drops older cached rows if one opened), and applies edits/deletions
  `since` the last visit.
- Optimistic UI: a sent post appears at once with a `pending_post_id`; the
  server's echo replaces it in place. Edits, reactions and pins update
  locally first. A late answer must never undo what the user just did.
- `src/ui/list.rs` is the virtualized list: only rows near the viewport are
  laid out, heights are cached by a key that includes everything that
  changes a row's height (see `post::row_key`), and the first visible row is
  held in place across frames.
- `src/images.rs` keeps textures; attachments not drawn recently are dropped
  beyond 48 resident. Avatars stay.
- `src/markup.rs` parses Mattermost markdown once per post (cached by id and
  edit time) into blocks; each paragraph is one galley with link ranges for
  hit-testing. LaTeX becomes Unicode with real sub/superscripts.
- Collapsed reply threads: channel views hold root posts only; replies live
  in the thread panel. Unread counts use the `_root` counters for channels and
  the plain ones for DMs.
- Fonts, colour emoji, icons, scrolling, the window-less shell and the tray
  come from fastframe (tag v0.4.1); move all fastframe crates together.
- `App` is made in `main` before any window and handed to
  `fastframe_shell::Shell`; each window `attach`es (fonts, menus, theme,
  textures) and `App::tick` runs for window frames and headless ticks alike.
  Closing the window hides to the menu bar when the tray is shown and
  `keep_running` is on; Cmd+Q and the tray's Quit really quit.
- `src/macos.rs`: the menu bar (its Edit items feed Cmd+C/X/V/A/Z to egui,
  with a `pbpaste` fallback; tray menu ids go to `fastframe_tray::claim_menu_event`
  first, and muda must stay at tray-icon's version), the `mmauth://` Apple
  Event handler (installed in `main`, before the event loop), the Dock badge.
- Single sign-on: `Api::sso_url` picks the server's provider and sends
  `redirect_to=mmauth://callback?state=<random>`; the browser returns that
  link with `MMAUTHTOKEN` and `srv`. `check_sso_link` accepts it once, only
  with our state, our callback and the chosen server.
- Sessions: the local cache is per server *and* account
  (`servers/<host>/<user id>/`); `Session::stop` aborts every task the session
  owns and mutes its late results; sign-out deletes the account's cache.
- Untrusted input has budgets: math nesting and size (both parsers), markdown
  block depth, GIF decode size; download names are reduced to one component;
  only http/https/mailto links leave the app.
- `src/math.rs` lays out display math (parse → boxes → paint) in STIX Two
  Math; `src/highlight.rs` colours code blocks; `src/icon.rs` draws the app
  icon and the menu-bar glyph.
- Sidebar categories come from `/users/me/teams/{id}/channels/categories`
  (cached per team in the store); muted = `notify_props.mark_unread == "mention"`;
  notifications follow the channel's then the account's `desktop` setting.

egui pitfalls already hit: `add_space` panics inside a `Grid`; egui-winit
eats Cmd+V when the clipboard has no text; `TextEdit::frame` takes a `Frame`
in 0.36; `ScrollArea::stick_to_bottom` overrides explicit offsets (the list
sticks itself); a blinking caret repaints forever (blink is off); a
`ScrollArea` inside a `Modal` needs `min_scrolled_height`.

## Testing

- `python3 dev/seed.py` seeds the local server (see README). Never use a
  real server's data for screenshots.
- `cargo run -- --dev --script "channel:ttbar-analysis;wait:1500;shot:x.png;quit"`
  drives the app and saves screenshots; steps are listed in `src/script.rs`.
- `python3 dev/post_as.py alice ttbar-analysis "hi @aritra"` posts as another
  user, to check live delivery.
- `cargo test` covers markup (mentions, emoji, LaTeX).

Never log tokens or message contents.
