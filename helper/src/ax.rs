//! What the helper does in other apps: lists their windows, reads a window's accessibility
//! tree, acts on its elements, posts keys to one app and takes a picture of one window.
//! Everything but `find` and `apps_text` is a macOS call that needs the user's permission,
//! so only those two are tested.

use core_foundation::array::CFArray;
use core_foundation::base::{CFType, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::number::CFNumber;
use core_foundation::string::CFString;
use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetTypeID, CFArrayGetValueAtIndex};
use core_foundation_sys::base::{CFGetTypeID, CFRelease};
use core_foundation_sys::data::{
    CFDataCreateMutable, CFDataGetBytePtr, CFDataGetLength, CFMutableDataRef,
};
use core_foundation_sys::dictionary::CFDictionaryGetValue;
use std::ffi::c_void;
use std::ptr::null;
use std::sync::Once;
use std::time::{Duration, Instant};

/// Most elements one `look` lists; the rest of a big window is cut.
const MAX_ELEMENTS: usize = 500;
const MAX_DEPTH: usize = 30;
/// A window of an app that does not answer is read for this long at most.
const READ_TIME: Duration = Duration::from_secs(10);
/// Longest label or value shown for one element.
const MAX_LABEL: usize = 200;

type Raw = *const c_void;

#[repr(C)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
struct CGSize {
    width: f64,
    height: f64,
}

#[repr(C)]
struct CGRect {
    origin: CGPoint,
    size: CGSize,
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: Raw) -> bool;
    fn AXUIElementCreateApplication(pid: i32) -> Raw;
    fn AXUIElementCopyAttributeValue(element: Raw, attribute: Raw, value: *mut Raw) -> i32;
    fn AXUIElementSetAttributeValue(element: Raw, attribute: Raw, value: Raw) -> i32;
    fn AXUIElementPerformAction(element: Raw, action: Raw) -> i32;
    fn AXUIElementSetMessagingTimeout(element: Raw, seconds: f32) -> i32;
    // Not documented, but what every tool that matches an AX window to a window id uses.
    fn _AXUIElementGetWindow(element: Raw, window: *mut u32) -> i32;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> Raw;
    fn CGWindowListCreateImage(bounds: CGRect, option: u32, window: u32, image: u32) -> Raw;
    fn CGWindowListCreateImageFromArray(bounds: CGRect, windows: Raw, option: u32) -> Raw;
    fn CGMainDisplayID() -> u32;
    fn CGDisplayBounds(display: u32) -> CGRect;
    fn CGEventCreateKeyboardEvent(source: Raw, key: u16, down: bool) -> Raw;
    fn CGEventKeyboardSetUnicodeString(event: Raw, length: usize, text: *const u16);
    fn CGEventSetFlags(event: Raw, flags: u64);
    fn CGEventPostToPid(pid: i32, event: Raw);
}

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageDestinationCreateWithData(
        data: CFMutableDataRef,
        kind: Raw,
        count: usize,
        options: Raw,
    ) -> Raw;
    fn CGImageDestinationAddImage(destination: Raw, image: Raw, properties: Raw);
    fn CGImageDestinationFinalize(destination: Raw) -> bool;
}

/// Whether the helper may use the accessibility API: the permission to read and control apps.
pub fn trusted() -> bool {
    // SAFETY: no arguments.
    unsafe { AXIsProcessTrusted() }
}

/// Whether the helper may take pictures of other apps' windows.
pub fn can_capture() -> bool {
    // SAFETY: no arguments.
    unsafe { CGPreflightScreenCaptureAccess() }
}

/// Makes macOS list the helper under Accessibility and asks the user, where it lacks it. Once in a
/// run of the helper: macOS asks once, and a refusal is not asked again at every call.
pub fn ask_accessibility() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if !trusted() {
            let key = CFString::new("AXTrustedCheckOptionPrompt");
            let options = CFDictionary::from_CFType_pairs(&[(key, CFBoolean::true_value())]);
            // SAFETY: a valid dictionary for the call.
            unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as Raw) };
        }
    });
}

/// The same for Screen Recording.
pub fn ask_screen_recording() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        if !can_capture() {
            // SAFETY: no arguments.
            unsafe { CGRequestScreenCaptureAccess() };
        }
    });
}

/// A window on screen.
pub struct Win {
    pub id: u32,
    pub pid: i32,
    pub owner: String,
    pub title: String,
    pub width: i64,
    pub height: i64,
}

fn get(dict: Raw, key: &str) -> Option<CFType> {
    let key = CFString::new(key);
    // SAFETY: `dict` is a CFDictionary that outlives the call; the key is a CFString.
    let value = unsafe { CFDictionaryGetValue(dict.cast(), key.as_concrete_TypeRef().cast()) };
    // SAFETY: a value of a dictionary is valid while the dictionary is; this takes its own hold.
    (!value.is_null()).then(|| unsafe { CFType::wrap_under_get_rule(value) })
}

fn int(value: Option<CFType>) -> Option<i64> {
    value?.downcast::<CFNumber>()?.to_i64()
}

/// A size or position: macOS gives fractional ones on scaled displays.
fn float(value: Option<CFType>) -> Option<f64> {
    value?.downcast::<CFNumber>()?.to_f64()
}

fn string(value: Option<CFType>) -> Option<String> {
    Some(value?.downcast::<CFString>()?.to_string())
}

/// The windows on screen, front to back. shortcut: a minimised window or one on another Space
/// is not listed, nor is an app that has none; add them when a bot needs to reach those.
pub fn windows() -> Vec<Win> {
    // On screen only (1), without the desktop's own elements (16).
    // SAFETY: plain call; the list is ours to release, which the wrapper does.
    let list = unsafe { CGWindowListCopyWindowInfo(1 | 16, 0) };
    if list.is_null() {
        return Vec::new();
    }
    // SAFETY: the copy is ours (create rule).
    let list = unsafe { CFType::wrap_under_create_rule(list as CFTypeRef) };
    // SAFETY: `list` is a CFArray, which that call returns.
    let count = unsafe { CFArrayGetCount(list.as_CFTypeRef().cast()) };
    let mut wins = Vec::new();
    for i in 0..count {
        // SAFETY: `i` is in range; the entries are dictionaries.
        let entry = unsafe { CFArrayGetValueAtIndex(list.as_CFTypeRef().cast(), i) };
        // Layer 0 is where an app's own windows are: menus, the Dock and overlays are not.
        if int(get(entry, "kCGWindowLayer")) != Some(0) {
            continue;
        }
        let Some(bounds) = get(entry, "kCGWindowBounds") else {
            continue;
        };
        let size = |key| {
            float(get(bounds.as_CFTypeRef(), key))
                .unwrap_or(0.0)
                .round() as i64
        };
        let (width, height) = (size("Width"), size("Height"));
        let (Some(id), Some(pid), Some(owner)) = (
            int(get(entry, "kCGWindowNumber")),
            int(get(entry, "kCGWindowOwnerPID")),
            string(get(entry, "kCGWindowOwnerName")),
        ) else {
            continue;
        };
        if width < 2 || height < 2 {
            continue;
        }
        wins.push(Win {
            id: id as u32,
            pid: pid as i32,
            owner,
            // Without Screen Recording macOS leaves the title out.
            title: string(get(entry, "kCGWindowName")).unwrap_or_default(),
            width,
            height,
        });
    }
    wins
}

/// The apps and their windows, as the bot reads them.
pub fn apps_text(wins: &[Win]) -> String {
    if wins.is_empty() {
        return "No app has a window on screen.".into();
    }
    let mut owners: Vec<&str> = Vec::new();
    for w in wins {
        if !owners.contains(&w.owner.as_str()) {
            owners.push(&w.owner);
        }
    }
    let mut out = String::new();
    for owner in owners {
        out.push_str(&format!("{}\n", clip(owner)));
        for w in wins.iter().filter(|w| w.owner == owner) {
            out.push_str(&format!(
                "  window {} \"{}\" {}x{}\n",
                w.id,
                clip(&w.title),
                w.width,
                w.height
            ));
        }
    }
    out
}

/// The window the bot means: the app's `window`, or its front one.
pub fn find<'a>(wins: &'a [Win], app: &str, window: Option<u32>) -> Result<&'a Win, String> {
    let app = app.trim();
    let of_app: Vec<&Win> = wins
        .iter()
        .filter(|w| w.owner.to_lowercase() == app.to_lowercase())
        .collect();
    let Some(front) = of_app.first() else {
        let mut owners: Vec<&str> = wins.iter().map(|w| w.owner.as_str()).collect();
        owners.dedup();
        return Err(format!(
            "No app named {app} has a window on screen. Apps that have: {}.",
            owners.join(", ")
        ));
    };
    match window {
        None => Ok(front),
        Some(id) => of_app.iter().find(|w| w.id == id).copied().ok_or_else(|| {
            let ids: Vec<String> = of_app.iter().map(|w| w.id.to_string()).collect();
            format!("{app} has no window {id}; its windows: {}.", ids.join(", "))
        }),
    }
}

/// One line of text: no line breaks, and cut at `MAX_LABEL`.
fn clip(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_LABEL)
        .collect()
}

fn attr(element: &CFType, name: &str) -> Option<CFType> {
    let name = CFString::new(name);
    let mut value: Raw = null();
    // SAFETY: valid element and name; `value` is set only on success.
    let error = unsafe {
        AXUIElementCopyAttributeValue(
            element.as_CFTypeRef(),
            name.as_concrete_TypeRef().cast(),
            &mut value,
        )
    };
    // SAFETY: a copied attribute is ours (create rule).
    (error == 0 && !value.is_null()).then(|| unsafe { CFType::wrap_under_create_rule(value) })
}

/// What an attribute holds, as text: a string, a number or a yes/no.
fn text(element: &CFType, name: &str) -> Option<String> {
    let value = attr(element, name)?;
    if let Some(s) = value.downcast::<CFString>() {
        return Some(s.to_string());
    }
    if let Some(n) = value.downcast::<CFNumber>() {
        return n
            .to_i64()
            .map(|n| n.to_string())
            .or(n.to_f64().map(|f| f.to_string()));
    }
    value
        .downcast::<CFBoolean>()
        .map(|b| bool::from(b).to_string())
}

fn children(element: &CFType, name: &str) -> Vec<CFType> {
    let Some(list) = attr(element, name) else {
        return Vec::new();
    };
    let list = list.as_CFTypeRef();
    // SAFETY: valid object; the type is checked before it is read as an array.
    if unsafe { CFGetTypeID(list) != CFArrayGetTypeID() } {
        return Vec::new();
    }
    // SAFETY: `list` is a CFArray; the indexes are in range; each entry gets its own hold.
    unsafe {
        (0..CFArrayGetCount(list.cast()))
            .map(|i| CFType::wrap_under_get_rule(CFArrayGetValueAtIndex(list.cast(), i)))
            .collect()
    }
}

fn why(error: i32) -> String {
    match error {
        -25211 => "Accessibility is not allowed for Bate Helper.".into(),
        -25202 | -25201 => "The element is gone: look again.".into(),
        -25204 => "The app did not answer.".into(),
        -25205 => "The element has no such attribute.".into(),
        -25206 => "The element cannot do that.".into(),
        n => format!("macOS refused (accessibility error {n})."),
    }
}

/// A window's elements as the last `look` numbered them, until the next one.
pub struct Tree {
    pub pid: i32,
    pub window: u32,
    elements: Vec<CFType>,
}

impl Tree {
    fn element(&self, number: u32) -> Result<&CFType, String> {
        self.elements
            .get(number as usize - 1)
            .ok_or_else(|| format!("There is no element {number} in the window: look again."))
    }
}

fn window_element(win: &Win) -> Result<CFType, String> {
    // SAFETY: plain call; the application element is ours (create rule).
    let app = unsafe { CFType::wrap_under_create_rule(AXUIElementCreateApplication(win.pid)) };
    // An app that hangs must not hang the helper with it.
    // SAFETY: valid element.
    unsafe { AXUIElementSetMessagingTimeout(app.as_CFTypeRef(), 5.0) };
    for window in children(&app, "AXWindows") {
        let mut id = 0u32;
        // SAFETY: valid element and out-pointer.
        let error = unsafe { _AXUIElementGetWindow(window.as_CFTypeRef(), &mut id) };
        if error == 0 && id == win.id {
            return Ok(window);
        }
    }
    Err(format!(
        "{} did not give window {} to read; it may have closed.",
        win.owner, win.id
    ))
}

fn walk(element: &CFType, depth: usize, tree: &mut Tree, out: &mut String, until: Instant) {
    if tree.elements.len() >= MAX_ELEMENTS || depth > MAX_DEPTH || Instant::now() > until {
        return;
    }
    tree.elements.push(element.clone());
    let role = text(element, "AXRole").unwrap_or_default();
    let secret = text(element, "AXSubrole").as_deref() == Some("AXSecureTextField");
    let role = if secret {
        "password field"
    } else {
        role.strip_prefix("AX").unwrap_or(&role)
    };
    let label = ["AXTitle", "AXDescription", "AXPlaceholderValue"]
        .iter()
        .find_map(|name| text(element, name).filter(|t| !t.trim().is_empty()))
        .map(|t| clip(&t))
        .unwrap_or_default();
    // What is in a password field is not shown.
    let value = if secret {
        String::new()
    } else {
        text(element, "AXValue")
            .map(|t| clip(&t))
            .unwrap_or_default()
    };
    out.push_str(&format!(
        "{}[{}] {role}",
        "  ".repeat(depth),
        tree.elements.len()
    ));
    if !label.is_empty() {
        out.push_str(&format!(" \"{label}\""));
    }
    if !value.is_empty() && value != label {
        out.push_str(&format!(" = \"{value}\""));
    }
    out.push('\n');
    for child in children(element, "AXChildren") {
        walk(&child, depth + 1, tree, out, until);
    }
}

/// Reads the window's tree: the text for the bot, and the elements it numbered.
pub fn look(win: &Win) -> Result<(String, Tree), String> {
    let root = window_element(win)?;
    // SAFETY: valid element.
    unsafe { AXUIElementSetMessagingTimeout(root.as_CFTypeRef(), 5.0) };
    let mut tree = Tree {
        pid: win.pid,
        window: win.id,
        elements: Vec::new(),
    };
    let mut out = format!(
        "{}: window {} \"{}\"\n",
        win.owner,
        win.id,
        clip(&win.title)
    );
    walk(&root, 0, &mut tree, &mut out, Instant::now() + READ_TIME);
    if tree.elements.len() >= MAX_ELEMENTS {
        out.push_str(&format!("[cut at {MAX_ELEMENTS} elements]\n"));
    }
    Ok((out, tree))
}

pub fn press(tree: &Tree, number: u32) -> Result<String, String> {
    let element = tree.element(number)?;
    let action = CFString::new("AXPress");
    // SAFETY: valid element and action name.
    let error = unsafe {
        AXUIElementPerformAction(element.as_CFTypeRef(), action.as_concrete_TypeRef().cast())
    };
    match error {
        0 => Ok(format!("Pressed element {number}.")),
        n => Err(why(n)),
    }
}

fn set(element: &CFType, name: &str, value: &CFType) -> Result<(), String> {
    let name = CFString::new(name);
    // SAFETY: valid element, name and value.
    let error = unsafe {
        AXUIElementSetAttributeValue(
            element.as_CFTypeRef(),
            name.as_concrete_TypeRef().cast(),
            value.as_CFTypeRef(),
        )
    };
    match error {
        0 => Ok(()),
        n => Err(why(n)),
    }
}

pub fn set_value(tree: &Tree, number: u32, value: &str) -> Result<String, String> {
    let element = tree.element(number)?;
    set(element, "AXValue", &CFString::new(value).as_CFType())?;
    Ok(format!("Set element {number}."))
}

/// Types `text` into the element (focused first) or into what has the focus, by posting key
/// events to the app's process: the user's front app and pointer stay where they are.
/// shortcut: one event pair per character with no pause, which an app that is slow to take
/// input may drop from; add a pause when one does.
pub fn type_text(pid: i32, tree: Option<(&Tree, u32)>, text: &str) -> Result<String, String> {
    if let Some((tree, number)) = tree {
        set(
            tree.element(number)?,
            "AXFocused",
            &CFBoolean::true_value().as_CFType(),
        )?;
    }
    for c in text.chars() {
        let mut units = [0u16; 2];
        match c {
            '\n' => key(pid, 36, &[]),
            '\t' => key(pid, 48, &[]),
            c => key(pid, 0, c.encode_utf16(&mut units)),
        }
    }
    Ok(format!("Typed {} characters.", text.chars().count()))
}

/// A key press and release for the app `pid`; `units` is the text the key types, if any.
fn key(pid: i32, code: u16, units: &[u16]) {
    for down in [true, false] {
        // SAFETY: a null source is the combined state; the event is ours and released below.
        unsafe {
            let event = CGEventCreateKeyboardEvent(null(), code, down);
            if event.is_null() {
                return;
            }
            // No modifier the user holds goes with the key.
            CGEventSetFlags(event, 0);
            if !units.is_empty() {
                CGEventKeyboardSetUnicodeString(event, units.len(), units.as_ptr());
            }
            CGEventPostToPid(pid, event);
            CFRelease(event);
        }
    }
}

/// A PNG of the window, even where other windows cover it.
pub fn screenshot(win: &Win) -> Result<Vec<u8>, String> {
    // Only this window (8), without its shadow (1).
    // SAFETY: plain call; the image is ours and `encode` releases it.
    let image = unsafe { CGWindowListCreateImage(null_rect(), 8, win.id, 1) };
    if image.is_null() {
        return Err(format!("Could not take a picture of window {}.", win.id));
    }
    encode(image)
}

/// The rectangle that means "as big as what is listed".
fn null_rect() -> CGRect {
    CGRect {
        origin: CGPoint {
            x: f64::INFINITY,
            y: f64::INFINITY,
        },
        size: CGSize {
            width: 0.0,
            height: 0.0,
        },
    }
}

/// A PNG of the main display, drawn from every window on screen but Bate's own: Quick Ask's
/// panel, which is up while it asks, is not in the picture.
pub fn screen() -> Result<Vec<u8>, String> {
    // On screen only (1): the desktop and the menu bar are listed too.
    // SAFETY: plain call; the list is ours (create rule).
    let list = unsafe { CGWindowListCopyWindowInfo(1, 0) };
    if list.is_null() {
        return Err("Could not list the windows.".into());
    }
    // SAFETY: the copy is ours (create rule).
    let list = unsafe { CFType::wrap_under_create_rule(list as CFTypeRef) };
    // SAFETY: `list` is a CFArray, which that call returns.
    let count = unsafe { CFArrayGetCount(list.as_CFTypeRef().cast()) };
    let mut ids = Vec::new();
    for i in 0..count {
        // SAFETY: `i` is in range; the entries are dictionaries.
        let entry = unsafe { CFArrayGetValueAtIndex(list.as_CFTypeRef().cast(), i) };
        if string(get(entry, "kCGWindowOwnerName")).is_some_and(|owner| is_bate(&owner)) {
            continue;
        }
        if let Some(id) = int(get(entry, "kCGWindowNumber")) {
            ids.push(CFNumber::from(id as i32));
        }
    }
    let ids = CFArray::from_CFTypes(&ids);
    // SAFETY: plain calls; the array outlives the call, the image is ours and `encode` releases it.
    let image = unsafe {
        CGWindowListCreateImageFromArray(
            CGDisplayBounds(CGMainDisplayID()),
            ids.as_concrete_TypeRef().cast(),
            0,
        )
    };
    if image.is_null() {
        return Err("Could not take a picture of the screen.".into());
    }
    encode(image)
}

/// Bate's own windows: the app, its Quick Ask panel.
fn is_bate(owner: &str) -> bool {
    owner.eq_ignore_ascii_case("bate")
}

/// The windows on screen that are not Bate's, one to a line: `id`, app and title, with tabs between.
pub fn windows_list(wins: &[Win]) -> String {
    wins.iter()
        .filter(|w| !is_bate(&w.owner))
        .map(|w| format!("{}\t{}\t{}\n", w.id, clip(&w.owner), clip(&w.title)))
        .collect()
}

/// The text selected in the app in front of Bate's windows, or none (empty). Never the selection
/// of a password field.
pub fn selection(wins: &[Win]) -> String {
    let Some(front) = wins.iter().find(|w| !is_bate(&w.owner)) else {
        return String::new();
    };
    // SAFETY: plain call; the application element is ours (create rule).
    let app = unsafe { CFType::wrap_under_create_rule(AXUIElementCreateApplication(front.pid)) };
    // An app that hangs must not hang the helper with it.
    // SAFETY: valid element.
    unsafe { AXUIElementSetMessagingTimeout(app.as_CFTypeRef(), 2.0) };
    let Some(focus) = attr(&app, "AXFocusedUIElement") else {
        return String::new();
    };
    if text(&focus, "AXSubrole").as_deref() == Some("AXSecureTextField") {
        return String::new();
    }
    text(&focus, "AXSelectedText")
        .unwrap_or_default()
        .chars()
        .take(proto::desktop::MAX_TEXT)
        .collect()
}

/// Takes `image` (which it releases) as a PNG.
fn encode(image: Raw) -> Result<Vec<u8>, String> {
    let png = CFString::new("public.png");
    // SAFETY: the data, type, image and destination are valid for each call; all are released.
    let bytes = unsafe {
        let data = CFDataCreateMutable(null(), 0);
        let target =
            CGImageDestinationCreateWithData(data, png.as_concrete_TypeRef().cast(), 1, null());
        let done = !target.is_null() && {
            CGImageDestinationAddImage(target, image, null());
            CGImageDestinationFinalize(target)
        };
        let bytes = done.then(|| {
            std::slice::from_raw_parts(CFDataGetBytePtr(data), CFDataGetLength(data) as usize)
                .to_vec()
        });
        if !target.is_null() {
            CFRelease(target);
        }
        CFRelease(data.cast());
        CFRelease(image);
        bytes
    };
    bytes.ok_or_else(|| "Could not encode the picture.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(id: u32, owner: &str, title: &str) -> Win {
        Win {
            id,
            pid: 1,
            owner: owner.into(),
            title: title.into(),
            width: 500,
            height: 400,
        }
    }

    #[test]
    fn the_bot_names_an_app_in_any_case_and_gets_its_front_window_unless_it_names_one() {
        let wins = [
            win(5, "Safari", "A"),
            win(7, "TextEdit", "Untitled"),
            win(9, "TextEdit", "B"),
        ];
        assert_eq!(find(&wins, "textedit ", None).unwrap().id, 7);
        assert_eq!(find(&wins, "TextEdit", Some(9)).unwrap().id, 9);
        assert_eq!(
            find(&wins, "TextEdit", Some(5)).err().unwrap(),
            "TextEdit has no window 5; its windows: 7, 9."
        );
        assert_eq!(
            find(&wins, "Notes", None).err().unwrap(),
            "No app named Notes has a window on screen. Apps that have: Safari, TextEdit."
        );
    }

    #[test]
    fn windows_are_listed_under_their_app_without_line_breaks_in_titles() {
        let wins = [
            win(7, "TextEdit", "Un\ntitled"),
            win(5, "Safari", "A"),
            win(9, "TextEdit", "B"),
        ];
        assert_eq!(
            apps_text(&wins),
            "TextEdit\n  window 7 \"Un titled\" 500x400\n  window 9 \"B\" 500x400\nSafari\n  window 5 \"A\" 500x400\n"
        );
    }

    #[test]
    fn the_pickers_list_has_one_line_per_window_and_none_of_bates_own() {
        let wins = [
            win(3, "Bate", "Quick Ask"),
            win(7, "TextEdit", "Un\ttitled"),
            win(5, "bate", "Bate"),
            win(9, "Safari", ""),
        ];
        assert_eq!(windows_list(&wins), "7\tTextEdit\tUn titled\n9\tSafari\t\n");
        assert_eq!(selection(&[win(3, "Bate", "x")]), "");
    }
}
