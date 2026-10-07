//! The macOS menu bar. Its Edit menu owns Cmd+C/X/V/A/Z and hands them to
//! egui as input events: macOS routes those key equivalents to the menu (or
//! the accessibility view) before the window sees them, so without this,
//! paste and copy silently do nothing (ZapFast's src/macos.rs does the same).

use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

thread_local! {
    static MENU: RefCell<Option<Menu>> = const { RefCell::new(None) };
}

static EDIT_EVENTS: LazyLock<Arc<Mutex<Vec<egui::Event>>>> = LazyLock::new(|| Arc::new(Mutex::new(Vec::new())));
/// Set when Paste found an image (and no text) on the clipboard.
static IMAGE_PASTE: AtomicBool = AtomicBool::new(false);

/// Menu edits reach the input before egui processes focus and selection.
struct MenuInput(Arc<Mutex<Vec<egui::Event>>>);

impl egui::plugin::Plugin for MenuInput {
    fn debug_name(&self) -> &'static str {
        "mmfast-macos-menu"
    }

    fn input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        input.events.extend(self.0.lock().unwrap_or_else(|p| p.into_inner()).drain(..));
    }
}

fn item(id: &str, text: &str, shift: bool, code: Code) -> MenuItem {
    let mods = if shift { Modifiers::META | Modifiers::SHIFT } else { Modifiers::META };
    MenuItem::with_id(id, text, true, Some(Accelerator::new(mods, code)))
}

pub fn install(ctx: &egui::Context) {
    *REPAINT_CTX.lock().unwrap_or_else(|p| p.into_inner()) = Some(ctx.clone());
    ctx.add_plugin(MenuInput(EDIT_EVENTS.clone()));
    // The menu bar and its handler outlive windows: make them once.
    if MENU.with(|m| m.borrow().is_some()) {
        return;
    }
    let menu = Menu::new();
    let app = Submenu::new("mmfast", true);
    let edit = Submenu::new("Edit", true);
    let window = Submenu::new("Window", true);
    let built = app
        .append_items(&[
            &PredefinedMenuItem::about(Some("About mmfast"), None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(Some("Hide mmfast")),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::quit(Some("Quit mmfast")),
        ])
        .and_then(|_| {
            edit.append_items(&[
                &item("undo", "Undo", false, Code::KeyZ),
                &item("redo", "Redo", true, Code::KeyZ),
                &PredefinedMenuItem::separator(),
                &item("cut", "Cut", false, Code::KeyX),
                &item("copy", "Copy", false, Code::KeyC),
                &item("paste", "Paste", false, Code::KeyV),
                &item("select-all", "Select All", false, Code::KeyA),
            ])
        })
        .and_then(|_| {
            window.append_items(&[
                &PredefinedMenuItem::minimize(None),
                &PredefinedMenuItem::maximize(None),
                &PredefinedMenuItem::fullscreen(None),
            ])
        })
        .and_then(|_| menu.append_items(&[&app, &edit, &window]));
    if let Err(err) = built {
        log::warn!("could not build the menu bar: {err}");
        return;
    }
    menu.init_for_nsapp();
    MENU.with(|m| *m.borrow_mut() = Some(menu));

    let events = EDIT_EVENTS.clone();
    // muda keeps the first handler it gets, process-wide; the tray's menu
    // clicks come through it too, so hand those over first.
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if fastframe_tray::claim_menu_event(&event.id().0) {
            return;
        }
        if let Some(ev) = edit_event(event.id().0.as_str()) {
            events.lock().unwrap_or_else(|p| p.into_inner()).push(ev);
        }
        if let Some(ctx) = REPAINT_CTX.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            ctx.request_repaint();
        }
    }));
}

static REPAINT_CTX: Mutex<Option<egui::Context>> = Mutex::new(None);

/// Clipboard text. arboard reads only the plain-string type; `pbpaste`
/// also converts the rich types some apps put text on the clipboard as.
fn clipboard_text() -> Option<String> {
    if let Ok(text) = arboard::Clipboard::new().ok()?.get_text()
        && !text.is_empty()
    {
        return Some(text);
    }
    let out = std::process::Command::new("/usr/bin/pbpaste").args(["-Prefer", "txt"]).output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    if text.is_empty() { None } else { Some(text) }
}

/// Paste found an image rather than text since the last call.
pub fn take_image_paste() -> bool {
    IMAGE_PASTE.swap(false, Ordering::SeqCst)
}

fn edit_event(id: &str) -> Option<egui::Event> {
    match id {
        "copy" => return Some(egui::Event::Copy),
        "cut" => return Some(egui::Event::Cut),
        "paste" => {
            if let Some(text) = clipboard_text() {
                return Some(egui::Event::Paste(text.replace("\r\n", "\n")));
            }
            if arboard::Clipboard::new().ok()?.get_image().is_ok() {
                IMAGE_PASTE.store(true, Ordering::SeqCst);
            } else {
                log::warn!("paste: the clipboard holds neither text nor an image");
            }
            return None;
        }
        _ => {}
    }
    let (key, shift) = match id {
        "undo" => (egui::Key::Z, false),
        "redo" => (egui::Key::Z, true),
        "select-all" => (egui::Key::A, false),
        _ => return None,
    };
    Some(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers { command: true, mac_cmd: true, shift, ..Default::default() },
    })
}

/// Links macOS hands the app (`mmauth://` from single sign-on). They come as
/// an Apple Event, never on the command line, so the handler is installed
/// in `main` before the event loop starts; otherwise the link the app was
/// launched for is lost. Adapted from Spotifast's src/mac_links.rs (MIT).
pub mod links {
    use objc2::rc::Retained;
    use objc2::{MainThreadOnly, define_class, msg_send, sel};
    use objc2_foundation::{MainThreadMarker, NSAppleEventDescriptor, NSAppleEventManager, NSObject};
    use std::sync::Mutex;

    /// `kInternetEventClass` and `kAEGetURL` are both `GURL`.
    const GET_URL: u32 = u32::from_be_bytes(*b"GURL");
    /// `keyDirectObject`, the parameter carrying the URL.
    const DIRECT_OBJECT: u32 = u32::from_be_bytes(*b"----");

    static LINKS: Mutex<Vec<String>> = Mutex::new(Vec::new());
    static REPAINT: Mutex<Option<egui::Context>> = Mutex::new(None);

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "MmfastLinkHandler"]
        pub struct MmfastLinkHandler;

        impl MmfastLinkHandler {
            #[unsafe(method(handleGetURLEvent:withReplyEvent:))]
            fn handle_get_url(&self, event: &NSAppleEventDescriptor, _reply: &NSAppleEventDescriptor) {
                let url: Option<Retained<NSAppleEventDescriptor>> =
                    unsafe { msg_send![event, paramDescriptorForKeyword: DIRECT_OBJECT] };
                if let Some(text) = url.and_then(|u| u.stringValue()) {
                    LINKS.lock().unwrap_or_else(|p| p.into_inner()).push(text.to_string());
                    if let Some(ctx) = REPAINT.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
                        ctx.request_repaint();
                    }
                }
            }
        }
    );

    pub fn install() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let handler: Retained<MmfastLinkHandler> = unsafe { msg_send![mtm.alloc::<MmfastLinkHandler>(), init] };
        let target: &NSObject = &handler;
        let manager = NSAppleEventManager::sharedAppleEventManager();
        unsafe {
            let _: () = msg_send![
                &*manager,
                setEventHandler: target,
                andSelector: sel!(handleGetURLEvent:withReplyEvent:),
                forEventClass: GET_URL,
                andEventID: GET_URL,
            ];
        }
        // The manager does not retain its handler; this one lives for the process.
        std::mem::forget(handler);
    }

    pub fn set_repaint(ctx: &egui::Context) {
        *REPAINT.lock().unwrap_or_else(|p| p.into_inner()) = Some(ctx.clone());
    }

    pub fn take() -> Vec<String> {
        std::mem::take(&mut *LINKS.lock().unwrap_or_else(|p| p.into_inner()))
    }
}

/// The number on the Dock icon; zero clears it.
pub fn set_badge(count: i64) {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;
    let label = (count > 0).then(|| NSString::from_str(&if count > 99 { "99+".to_string() } else { count.to_string() }));
    unsafe {
        let app: Option<Retained<AnyObject>> = msg_send![class!(NSApplication), sharedApplication];
        let Some(app) = app else { return };
        let tile: Option<Retained<AnyObject>> = msg_send![&*app, dockTile];
        if let Some(tile) = tile {
            let _: () = msg_send![&*tile, setBadgeLabel: label.as_deref()];
        }
    }
}
