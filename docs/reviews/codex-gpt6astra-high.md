The architecture is compact and understandable, with sensible separation between rendering, networking, and storage. However, several security boundaries are incomplete, and asynchronous responses can overwrite newer state or cross session boundaries. History reconciliation and unread accounting also have defects that can hide messages or misrepresent what has been read. I read all requested files and made no changes; this was a source review, without running the application or tests. No Critical finding was verified.

**Findings, ordered by severity**

1. **High: A short math expression can exhaust memory**
   
   [src/markup.rs:464](/Users/aritra/Projects/mattermost-fast/src/markup.rs:464), [src/math.rs:276](/Users/aritra/Projects/mattermost-fast/src/math.rs:276)
   
   Each nested `\bar{…}` doubles the converted character count because it adds an overline even to existing combining overlines. Thirty nested bars around `x` produce over a billion characters from a small message, blocking or exhausting the UI process. Both math parsers also recurse without a depth limit. **Fix:** enforce nesting, node-count, and output-size budgets; fall back to literal text when exceeded.

2. **High: Cached private data is shared across accounts**
   
   [src/backend.rs:253](/Users/aritra/Projects/mattermost-fast/src/backend.rs:253), [src/store.rs:208](/Users/aritra/Projects/mattermost-fast/src/store.rs:208)
   
   Cache paths identify only the server, and the cached snapshot is emitted before validating the new account. Signing out of account A and into account B on the same server can display A’s cached channels; A’s posts also remain in the unrestricted local search index after B’s snapshot replaces channel metadata. **Fix:** isolate databases and attachments by canonical server identity and authenticated user ID, and validate identity before exposing cached content.

3. **High: Logout leaves websocket and request tasks running**
   
   [src/backend.rs:286](/Users/aritra/Projects/mattermost-fast/src/backend.rs:286), [src/backend.rs:391](/Users/aritra/Projects/mattermost-fast/src/backend.rs:391)
   
   `stop()` aborts the task awaiting two separately spawned tasks. Dropping their `JoinHandle`s detaches them rather than cancelling them; ordinary request tasks and initial synchronization are also untracked. After switching accounts, old tasks can still emit snapshots, posts, or `Unauthorized` into the new session. **Fix:** cancel and join all session-owned tasks, and attach a session generation to commands/events so obsolete results are rejected.

4. **High: SSO callbacks permit login CSRF**
   
   [src/app.rs:1415](/Users/aritra/Projects/mattermost-fast/src/app.rs:1415)
   
   Acceptance requires only an outstanding sign-in and the `mmauth` scheme. There is no unpredictable attempt identifier, callback host/path validation, or mandatory server parameter. While sign-in is pending, a malicious page can supply an attacker’s valid token for the same server and sign the client into that account. **Fix:** bind callbacks to a single-use random state and immutable chosen server, and validate the exact callback endpoint.

5. **High: Download names can escape the Downloads directory**
   
   [src/backend.rs:843](/Users/aritra/Projects/mattermost-fast/src/backend.rs:843)
   
   Server-controlled `file.name` is joined directly onto the destination. An absolute path replaces the destination prefix; `../…` traverses out of it. A malicious server response can therefore cause a requested download to create a file elsewhere under the user’s permissions. The separate existence check also allows concurrent downloads to overwrite one another. **Fix:** accept only a single safe filename component and reserve the destination atomically with `create_new`.

6. **Medium: Late responses overwrite newer edits and live messages**
   
   [src/app.rs:46](/Users/aritra/Projects/mattermost-fast/src/app.rs:46), [src/app.rs:650](/Users/aritra/Projects/mattermost-fast/src/app.rs:650), [src/app.rs:682](/Users/aritra/Projects/mattermost-fast/src/app.rs:682)
   
   Upserts replace posts without revision checks, while channel/thread snapshots discard confirmed posts absent from their response. An old fetch arriving after an edit restores old text; a thread response captured before a websocket reply removes that reply. Concurrent mutation requests can also finish in reverse order. **Fix:** serialize mutations per entity, preserve pending revisions, and merge snapshots using request generations, server revisions, and deletion tombstones.

7. **Medium: Sparse history is treated as a continuous page**
   
   [src/backend.rs:790](/Users/aritra/Projects/mattermost-fast/src/backend.rs:790), [src/app.rs:1114](/Users/aritra/Projects/mattermost-fast/src/app.rs:1114)
   
   Catch-up inserts every changed root into the displayed timeline, but pagination uses its earliest displayed post as the cursor. If posts 941–1000 are loaded and catch-up returns an edited post 100, the next request fetches before 100, skipping 101–940. Search-populated cache rows create the same continuity problem. **Fix:** maintain explicit contiguous page boundaries and update out-of-window posts without moving the history cursor.

8. **Medium: Failed catch-up is recorded as successful synchronization**
   
   [src/backend.rs:776](/Users/aritra/Projects/mattermost-fast/src/backend.rs:776), [src/backend.rs:798](/Users/aritra/Projects/mattermost-fast/src/backend.rs:798)
   
   A failed `since` request becomes an empty result, after which the sync timestamp advances. An older cached post edited or deleted during that interval may remain stale indefinitely because subsequent requests start after the missed change. **Fix:** preserve the previous checkpoint on failure and advance it only after complete reconciliation.

9. **Medium: Failed optimistic mutations leave false local state**
   
   [src/app.rs:1067](/Users/aritra/Projects/mattermost-fast/src/app.rs:1067), [src/backend.rs:242](/Users/aritra/Projects/mattermost-fast/src/backend.rs:242)
   
   Edits, deletions, reactions, pins, and headers change locally before the request, but generic errors only produce a notification. For example, a rejected deletion removes the message locally while everyone else still sees it. **Fix:** return operation-specific failures and roll back or refetch the affected entity, preserving unsuccessful edit text.

10. **Medium: Initial connection failure prevents automatic recovery**
    
    [src/backend.rs:275](/Users/aritra/Projects/mattermost-fast/src/backend.rs:275), [src/backend.rs:410](/Users/aritra/Projects/mattermost-fast/src/backend.rs:410)
    
    If initial synchronization fails, the startup task exits before starting websocket reconnection. Restoring connectivity does not restart it. Additionally, the first successful websocket connection skips catch-up, leaving a gap between initial REST reads and websocket establishment. **Fix:** retry bootstrap independently and reconcile after every connection, including the first.

11. **Medium: A failed history request permanently disables pagination**
    
    [src/app.rs:1116](/Users/aritra/Projects/mattermost-fast/src/app.rs:1116), [src/backend.rs:493](/Users/aritra/Projects/mattermost-fast/src/backend.rs:493)
    
    `loading_older` becomes true before the request and is cleared only by a successful `Older` event. A timeout or HTTP error leaves subsequent requests blocked. The loading row can also continue animating near the viewport. **Fix:** emit a completion result on both success and failure, reset the flag, and show a retryable error instead of indefinite loading.

12. **Medium: Background synchronization clears unread indicators**
    
    [src/app.rs:639](/Users/aritra/Projects/mattermost-fast/src/app.rs:639)
    
    Every fresh snapshot marks the current channel locally read without checking focus, window visibility, or whether the Threads view is active. Reconnecting while hidden can therefore erase unread and mention indicators for messages the user never saw. **Fix:** acknowledge reading only when the relevant conversation is actually visible and focused.

13. **Medium: Snapshot and websocket overlap double-counts messages**
    
    [src/app.rs:875](/Users/aritra/Projects/mattermost-fast/src/app.rs:875), [src/app.rs:897](/Users/aritra/Projects/mattermost-fast/src/app.rs:897)
    
    Every live event increments totals and reply counts without checking whether that post is already represented. During reconnect synchronization, a snapshot can include a post whose websocket event is queued behind the fetch; processing both counts it twice. **Fix:** make event application idempotent and reconcile counters against a defined snapshot/event boundary.

14. **Medium: Read acknowledgements are dropped rather than deferred**
    
    [src/app.rs:942](/Users/aritra/Projects/mattermost-fast/src/app.rs:942)
    
    View requests within one second are simply discarded. If two messages arrive while reading, the first acknowledgement can complete before the second arrives; the second is marked read locally but never acknowledged remotely. **Fix:** coalesce suppressed acknowledgements into a trailing request.

15. **Medium: Multi-channel read events update only one channel**
    
    [src/ws.rs:65](/Users/aritra/Projects/mattermost-fast/src/ws.rs:65)
    
    `multiple_channels_viewed` takes only the first key from `channel_times`. Marking several channels read from another client leaves the remaining channels unread here. **Fix:** represent all affected channels in the event and apply each update.

16. **Medium: Followed-thread badges are stale outside the Threads view**
    
    [src/app.rs:761](/Users/aritra/Projects/mattermost-fast/src/app.rs:761), [src/ui/sidebar.rs:101](/Users/aritra/Projects/mattermost-fast/src/ui/sidebar.rs:101)
    
    Thread updates trigger a reload only while the Threads view is open, but the sidebar always uses that cached response for its badges. Before the first visit, unread threads appear as zero; afterward, new replies leave stale counts while reading channels. **Fix:** load and maintain thread totals independently of the selected view.

17. **Medium: Followed threads use the wrong page-size parameter and never paginate**
    
    [src/api.rs:399](/Users/aritra/Projects/mattermost-fast/src/api.rs:399)
    
    The request sends `pageSize=50`, whereas Mattermost’s client sends `per_page` with `before`/`after` cursors. Only one response is loaded, so accounts exceeding the first page cannot browse older followed threads. **Fix:** use `per_page` and implement cursor pagination. See the [official client implementation](https://github.com/mattermost/mattermost/blob/master/webapp/platform/client/src/client4.ts#L2514-L2531).

18. **Medium: Search displays results belonging to a previous query**
    
    [src/app.rs:1143](/Users/aritra/Projects/mattermost-fast/src/app.rs:1143), [src/ui/rhs.rs:154](/Users/aritra/Projects/mattermost-fast/src/ui/rhs.rs:154)
    
    Editing the query increments its generation but does not clear existing server results, which take precedence over local results. Searching “alpha” and then typing “beta” therefore displays alpha’s matches under beta’s heading. Editing during an outstanding server search can also leave `loading` permanently true. **Fix:** reset query-specific results and loading state whenever the query changes.

19. **Medium: Message links can invoke arbitrary OS URL handlers**
    
    [src/app.rs:1186](/Users/aritra/Projects/mattermost-fast/src/app.rs:1186)
    
    Unhandled markdown destinations are passed directly to `open`. Clicking a disguised `file:` link can open a local file or application; custom schemes invoke their registered handlers. **Fix:** explicitly allow supported external schemes and reject filesystem paths and unexpected protocols. This is handler invocation, not a verified shell-injection bug.

20. **Medium: GIF loading has no practical memory budget**
    
    [src/backend.rs:859](/Users/aritra/Projects/mattermost-fast/src/backend.rs:859), [src/images.rs:74](/Users/aritra/Projects/mattermost-fast/src/images.rs:74)
    
    Up to 300 decoded frames are retained and then uploaded as separate textures in one UI call. A 640×640 animation at that limit contains about 469 MiB of pixel data before additional copies or GPU storage; several animations can exhaust memory or stall rendering. **Fix:** enforce decoded-pixel and resident-byte budgets, and upload only a small working set of frames.

21. **Medium: Retained history grows indefinitely and is rescanned every frame**
    
    [src/app.rs:864](/Users/aritra/Projects/mattermost-fast/src/app.rs:864), [src/ui/post.rs:175](/Users/aritra/Projects/mattermost-fast/src/ui/post.rs:175), [src/ui/list.rs:107](/Users/aritra/Projects/mattermost-fast/src/ui/list.rs:107)
    
    Previously visited channel views keep accumulating live posts, while height caches retain obsolete revision keys. Rendering rebuilds row descriptions and estimates across the entire loaded history, including scanning message text. Long sessions therefore increase both memory use and per-frame work despite viewport virtualization. **Fix:** bound inactive views and obsolete heights, and cache row metadata until its inputs change.

22. **Medium: Reopening the window changes the clock used by persistent state**
    
    [src/app.rs:1450](/Users/aritra/Projects/mattermost-fast/src/app.rs:1450), [src/app.rs:1545](/Users/aritra/Projects/mattermost-fast/src/app.rs:1545)
    
    Window frames use egui’s context time; headless ticks use process elapsed time. The shell recreates the window/context, but deadlines and throttle timestamps survive. After reopening, old timestamps can lie far in the future, suppressing typing/read updates and prolonging temporary UI state. **Fix:** use one process-wide monotonic clock for all persistent deadlines and throttles.

23. **Medium: Linux offers SSO without a callback implementation**
    
    [src/main.rs:56](/Users/aritra/Projects/mattermost-fast/src/main.rs:56), [src/app.rs:1446](/Users/aritra/Projects/mattermost-fast/src/app.rs:1446)
    
    The sign-in UI opens an SSO flow returning to `mmauth://`, but callback installation and delivery exist only on macOS. There is no Linux URL argument/IPC path to deliver the token to the running client. **Fix:** implement scheme registration and callback forwarding on Linux, or disable this sign-in option there until supported.

24. **Medium: Virtualized rows lack stable widget identities**
    
    [src/ui/list.rs:102](/Users/aritra/Projects/mattermost-fast/src/ui/list.rs:102)
    
    Each visible row uses an unnamed child UI, making automatic widget IDs depend on its position among rendered children. When scrolling changes the first rendered row, the same ID can refer to another post, invalidating or transferring selection/focus state. **Fix:** assign each row an explicit persistent UI identity derived from the list and post ID, separately from its height-cache revision.

25. **Medium: Credential writes and deletion block the UI thread**
    
    [src/app.rs:590](/Users/aritra/Projects/mattermost-fast/src/app.rs:590), [src/config.rs:94](/Users/aritra/Projects/mattermost-fast/src/config.rs:94)
    
    Login completion calls synchronous keychain storage directly; logout and unauthorized handling similarly delete credentials synchronously. A slow or locked credential service, including an authorization prompt, blocks rendering and event processing. **Fix:** perform these operations through the worker mechanism already used for credential loading and report completion asynchronously.

26. **Medium: Script logging exposes message text and callback tokens**
    
    [src/script.rs:75](/Users/aritra/Projects/mattermost-fast/src/script.rs:75)
    
    Every complete script step is printed to stderr. `type:private text`, search terms, and `link:mmauth://…MMAUTHTOKEN=…` therefore appear in logs. Script execution is not restricted to `--dev`. **Fix:** log only command names and explicitly safe metadata, never raw arguments.

27. **Low: Global Escape handling prevents edit cancellation**
    
    [src/ui/overlays.rs:26](/Users/aritra/Projects/mattermost-fast/src/ui/overlays.rs:26), [src/ui/post.rs:489](/Users/aritra/Projects/mattermost-fast/src/ui/post.rs:489)
    
    Global shortcuts consume Escape before the post editor checks it, even when no global overlay handles the key. Escape consequently fails to cancel an edit, or closes the thread panel instead. **Fix:** give active editors/modals priority and consume Escape only when an action is performed.

28. **Low: Jumping to the same message twice can stop working**
    
    [src/ui/list.rs:74](/Users/aritra/Projects/mattermost-fast/src/ui/list.rs:74)
    
    `reveal_done` remembers the last revealed row indefinitely. Jump to a message, scroll away, then select that same search result again: the second reveal is ignored because its key matches. **Fix:** identify reveal requests individually or clear the remembered key once the request is consumed.

29. **Low: Packaging reports success after ad-hoc signing fails**
    
    [dev/bundle-macos.sh:47](/Users/aritra/Projects/mattermost-fast/dev/bundle-macos.sh:47)
    
    The fallback signing command suppresses diagnostics and ignores failure, then prints `built`. A failed signature therefore produces an apparently successful bundle that may fail to launch correctly. **Fix:** propagate signing failures and verify the completed bundle’s signature before reporting success.

**Things done well**

- Clear `Action`/`Command`/`Event` boundaries make state transitions traceable.
- Network requests and image decoding generally stay off the UI thread.
- Tokens use the OS credential store rather than ordinary configuration files.
- SQLite uses parameterized queries, WAL, and FTS5.
- Markdown parsing is cached, and the list limits actual row layout to the viewport neighborhood.
- The self-signed identity script uses temporary working files with cleanup and scopes trust to code signing.

**Recommended next improvements**

1. Fix account isolation, session cancellation, callback validation, and download containment first.
2. Add deterministic tests that reorder REST responses, websocket events, logout, and optimistic mutations.
3. Define explicit history-window and synchronization-checkpoint invariants, then test gaps and failed catch-up.
4. Fuzz math/markdown input under strict resource limits; add image decoding budgets.
5. Add lifecycle checks covering offline startup, hidden-window reconnect, window recreation, Linux SSO, and packaged signature verification.
---

## Resolution (2026-10-07)

Review by Codex (`gpt-6-astra`, reasoning high, read-only) of commit `695633f`.
Every finding was checked against the code; all were confirmed. Status after the
fix commit:

| # | Finding | Status |
|---|---|---|
| 1 | Nested `\bar` exhausts memory | Fixed: one bar per base letter (growth is linear); nesting and size budgets in both math parsers and in the markdown renderer; tests with hostile input |
| 2 | Cache shared across accounts | Fixed: cache per server *and* account; cached snapshot shown only for the expected account; the pre-0.1.1 shared cache is deleted; sign-out deletes the account's local copy |
| 3 | Logout leaves tasks running | Fixed: the session owns and aborts every task; a stopped session's late results are dropped |
| 4 | SSO login CSRF | Fixed: a random one-time `state` in the return address; the callback must be `mmauth://callback` with that state, the chosen server and a token (unit-tested) |
| 5 | Download path traversal | Fixed: one sanitized file-name component, claimed with `create_new` (unit-tested) |
| 6 | Late responses overwrite newer data | Mostly fixed: upserts keep the newer `update_at`; thread answers merge instead of replacing. Not done: serializing concurrent mutations per post |
| 7 | Sparse history breaks paging | Fixed: catch-up only touches the loaded window; server search results no longer enter the channel cache |
| 8 | Failed catch-up advances the checkpoint | Fixed |
| 9 | Failed optimistic changes stay | Fixed: the post (or channel header) is re-fetched when the server refuses |
| 10 | No recovery from a failed first sync | Fixed: the first sync retries with backoff; an unknown account is looked up again every 10 s; the first websocket connection also catches up |
| 11 | Failed paging blocks paging | Fixed: failure is reported, retried after a pause |
| 12 | Background sync clears unread | Fixed: only while the channel is on screen and focused |
| 13 | Double-counted live events | Mitigated: posts older than the channel's last known post are not counted |
| 14 | Read receipts dropped | Fixed: coalesced into a trailing receipt |
| 15 | Multi-channel read events | Fixed |
| 16 | Stale thread badges | Fixed: totals load with every server snapshot and thread update |
| 17 | Threads page size | Fixed `per_page`. Not done: paging past the first 50 followed threads |
| 18 | Stale search results | Fixed |
| 19 | Arbitrary URL schemes | Fixed: only http, https and mailto leave the app |
| 20 | GIF memory | Fixed: decoding stops at a 48 MB pixel budget |
| 21 | Unbounded history and heights | Fixed: channels off screen keep their newest 150 posts; the height cache is pruned |
| 22 | Clock changes with the window | Fixed: one process-wide monotonic clock |
| 23 | Linux SSO without a handler | Fixed by hiding the button on Linux (tokens and passwords work); a Linux `mmauth://` handler remains to be written |
| 24 | Unstable row identities | Fixed: rows take their post id as identity |
| 25 | Keychain writes on the UI thread | Fixed |
| 26 | Script logs arguments | Fixed: only step names are logged |
| 27 | Escape while editing | Fixed |
| 28 | Repeat jump ignored | Fixed |
| 29 | Signing failure ignored | Fixed: signing errors stop the build and the bundle is verified |

Tests: 19 unit tests (6 new for these fixes). Checked again end to end on macOS
against the local test server: sign-in, per-account cache, live delivery,
threads, search, jump to an old message, sign-out removing local data.
