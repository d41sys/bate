//! Quick Ask's panel and the global shortcut that opens it (PROTOCOL.md, "Quick Ask").
//!
//! The shortcut is a Carbon hot key: the system keeps it and calls this process for it with no
//! permission asked, whichever app is in front. The panel is a window of the page, made an
//! `NSPanel` that does not take the app forward: it shows over the front app (a full-screen one
//! too, in its own Space) and takes the keys while the front app stays the active one.
//! What the page does in it is in `QuickAsk.tsx`; this file only shows, places and hides it.

use objc2::runtime::{AnyClass, AnyObject};
use objc2::{ClassType, MainThreadOnly, define_class};
use objc2_app_kit::{NSPanel, NSWindow, NSWindowCollectionBehavior, NSWindowStyleMask};
use proto::quick::Shortcut;
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};
use tauri::{
    AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

/// The panel's label. It is not one of the windows of the app: `crate::windows` leaves it out.
pub const LABEL: &str = "bate-quick";

/// The panel's size in points; the page fills it and does not ask for another.
const WIDTH: f64 = 560.0;
const HEIGHT: f64 = 340.0;
/// `NSStatusWindowLevel`: over the windows of every app, a full-screen one's too.
const STATUS_LEVEL: isize = 25;

#[repr(C)]
struct EventTypeSpec {
    class: u32,
    kind: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct HotKeyId {
    signature: u32,
    id: u32,
}

type Handler = extern "C" fn(call: *mut c_void, event: *mut c_void, data: *mut c_void) -> i32;

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> *mut c_void;
    fn InstallEventHandler(
        target: *mut c_void,
        handler: Handler,
        count: u32,
        types: *const EventTypeSpec,
        data: *mut c_void,
        out: *mut *mut c_void,
    ) -> i32;
    fn RegisterEventHotKey(
        code: u32,
        modifiers: u32,
        id: HotKeyId,
        target: *mut c_void,
        options: u32,
        out: *mut *mut c_void,
    ) -> i32;
    fn UnregisterEventHotKey(hot_key: *mut c_void) -> i32;
}

/// `kEventClassKeyboard` ("keyb") and `kEventHotKeyPressed`.
const KEYBOARD: u32 = 0x6b65_7962;
const PRESSED: u32 = 5;
/// `eventHotKeyExistsErr`: another app holds the key.
const EXISTS: i32 = -9878;

static APP: OnceLock<AppHandle> = OnceLock::new();
/// The hot key in force: its text and the system's reference to it (a pointer kept as a number).
static HOT_KEY: Mutex<Option<(String, usize)>> = Mutex::new(None);

extern "C" fn pressed(_call: *mut c_void, _event: *mut c_void, _data: *mut c_void) -> i32 {
    if let Some(app) = APP.get() {
        let on_main = app.clone();
        let _ = app.run_on_main_thread(move || toggle(&on_main));
    }
    0
}

/// Makes `shortcut` (the service's text, `Ctrl+Alt+Space`) the one that opens the panel, or none.
/// The new key is taken before the old one is let go, so a refused key leaves the old one working.
pub fn set_shortcut(app: &AppHandle, shortcut: Option<&str>) -> Result<(), String> {
    let wanted = shortcut.map(Shortcut::parse).transpose()?;
    let mut held = HOT_KEY.lock().map_err(|_| "the shortcut is busy")?;
    let wanted_text = wanted.map(|s| s.to_string());
    if held.as_ref().map(|(text, _)| text) == wanted_text.as_ref() {
        return Ok(());
    }
    APP.get_or_init(|| app.clone());
    let fresh = match wanted {
        Some(shortcut) => Some((shortcut.to_string(), register(shortcut)?)),
        None => None,
    };
    if let Some((_, old)) = held.take() {
        // SAFETY: the reference came from `RegisterEventHotKey` and is let go once.
        unsafe { UnregisterEventHotKey(old as *mut c_void) };
    }
    *held = fresh;
    Ok(())
}

fn register(shortcut: Shortcut) -> Result<usize, String> {
    static HANDLER: OnceLock<i32> = OnceLock::new();
    let status = *HANDLER.get_or_init(|| {
        let spec = EventTypeSpec {
            class: KEYBOARD,
            kind: PRESSED,
        };
        let mut out = std::ptr::null_mut();
        // SAFETY: `spec` lives through the call, which copies it; `pressed` is a plain function.
        unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                pressed,
                1,
                &spec,
                std::ptr::null_mut(),
                &mut out,
            )
        }
    });
    if status != 0 {
        return Err(format!("the system would not take the shortcut ({status})"));
    }
    let (code, modifiers) = shortcut.carbon();
    let id = HotKeyId {
        signature: u32::from_be_bytes(*b"BATE"),
        id: 1,
    };
    let mut out = std::ptr::null_mut();
    // SAFETY: `out` is written by the call; options 1 keeps the key from any other app.
    let status = unsafe {
        RegisterEventHotKey(
            code,
            modifiers,
            id,
            GetApplicationEventTarget(),
            1,
            &mut out,
        )
    };
    match status {
        0 => Ok(out as usize),
        EXISTS => Err(format!(
            "{shortcut} is used by another app or by macOS: choose another"
        )),
        _ => Err(format!("the system would not take {shortcut} ({status})")),
    }
}

define_class!(
    /// What the panel's window is turned into: an `NSPanel` that may take the keys.
    #[unsafe(super(NSPanel))]
    #[thread_kind = MainThreadOnly]
    #[name = "BateQuickPanel"]
    struct Panel;

    impl Panel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key(&self) -> bool {
            true
        }
    }
);

/// The window of `window` as the system's own, for the calls of AppKit.
fn native(window: &WebviewWindow) -> Result<&NSWindow, String> {
    let ptr = window.ns_window().map_err(|e| e.to_string())?;
    // SAFETY: the pointer is the window's own `NSWindow`, alive as long as `window` is, and
    // only used on the main thread, where these calls run.
    Ok(unsafe { &*(ptr as *const NSWindow) })
}

/// Builds the panel, hidden. Run on the main thread.
fn build(app: &AppHandle) -> Result<WebviewWindow, String> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("index.html".into()))
        .title("Quick Ask")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .visible(false)
        // A first click on the panel is a click on what is under it, not only a focus.
        .accept_first_mouse(true)
        .disable_drag_drop_handler()
        .on_navigation(crate::stays)
        .on_new_window(crate::deny_new_window)
        .initialization_script("window.__BATE_QUICK__ = true;")
        .build()
        .map_err(|e| e.to_string())?;
    let ns = native(&window)?;
    let ptr = ns as *const NSWindow as *mut AnyObject;
    // SAFETY: the class is swapped for a subclass of `NSPanel` that adds no ivar, and only when the
    // window's own class is at least as large, so that no ivar of the window lies outside the
    // instance (`NSPanel` and `NSWindow` are the same size, and the window's class adds its own).
    // A window whose class is smaller stays a plain window: it works, but takes Bate forward.
    unsafe {
        let own: &AnyClass = (*ptr).class();
        let panel = Panel::class();
        if panel.instance_size() <= own.instance_size() {
            objc2::ffi::object_setClass(ptr, panel as *const AnyClass as *const _);
            let panel: &NSPanel = &*(ptr as *const NSPanel);
            panel.setFloatingPanel(true);
            panel.setBecomesKeyOnlyIfNeeded(false);
        }
    }
    ns.setStyleMask(ns.styleMask() | NSWindowStyleMask::NonactivatingPanel);
    ns.setLevel(STATUS_LEVEL);
    ns.setHidesOnDeactivate(false);
    ns.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
    Ok(window)
}

/// Puts the panel on the screen the pointer is on: centred, in the upper part. Physical pixels.
fn place(app: &AppHandle, window: &WebviewWindow) {
    let Ok(at) = app.cursor_position() else {
        return;
    };
    let Ok(Some(screen)) = app.monitor_from_point(at.x, at.y) else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    let (origin, area) = (screen.position(), screen.size());
    let x = origin.x + (area.width as i32 - size.width as i32) / 2;
    let y = origin.y + area.height as i32 / 6;
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

/// Shows the panel, or hides it when it is up. Run on the main thread.
pub fn toggle(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL)
        && window.is_visible().unwrap_or(false)
    {
        let _ = window.hide();
        return;
    }
    if let Err(e) = show(app) {
        eprintln!("Quick Ask: the panel could not open: {e}");
    }
}

fn show(app: &AppHandle) -> Result<(), String> {
    let window = match app.get_webview_window(LABEL) {
        Some(window) => window,
        None => build(app)?,
    };
    place(app, &window);
    let ns = native(&window)?;
    // Not `show` and `set_focus` of the window: those bring Bate forward, and with it its own Space.
    ns.orderFrontRegardless();
    ns.makeKeyWindow();
    // The page starts again, empty: nothing from the last time is there.
    window
        .eval("dispatchEvent(new CustomEvent('bate-quick'))")
        .map_err(|e| e.to_string())
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
}

/// Takes the shortcut the service has kept, once the service is there: at the start of the app it
/// is not yet. A refused key (another app took it) is said on stderr, and in Settings when it opens.
pub fn restore(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        for _ in 0..30 {
            let kept = crate::bridge::ask(
                &proto::paths::socket(),
                &[serde_json::json!({"type": "get_quick_ask"})],
            );
            let Ok(replies) = kept else {
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            };
            let Some(text) = replies[0]["data"]["shortcut"].as_str().map(str::to_owned) else {
                return;
            };
            let on_main = app.clone();
            let _ = app.run_on_main_thread(move || {
                if let Err(e) = set_shortcut(&on_main, Some(&text)) {
                    eprintln!("Quick Ask: {e}");
                }
            });
            return;
        }
    });
}
