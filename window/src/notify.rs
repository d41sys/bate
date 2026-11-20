//! The system's notification for a bot that needs the person, and what a click on it does.
//! The page asks (`notify_waiting`) when a question comes in that it is not showing; macOS asks
//! the user's leave itself, the first time one is due, not before.

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{NSBundle, NSError, NSObject, NSObjectProtocol, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
    UNNotificationResponse, UNUserNotificationCenter, UNUserNotificationCenterDelegate,
};
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};

/// A notification's identifier is the bot and the question: every open window asks for the same
/// question, and the system keeps one notification per identifier, so there is one banner.
fn identifier(bot: &str, question: &str) -> String {
    format!("{bot}\n{question}")
}

/// A bot's id, as the page writes it into a script when a click opens the bot: nothing else
/// may get in there.
pub fn is_bot_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// The bot a notification's identifier names.
fn target(identifier: &str) -> Option<&str> {
    let bot = identifier.split('\n').next()?;
    is_bot_id(bot).then_some(bot)
}

/// A click: an open window comes forward and opens the bot.
fn clicked(app: &AppHandle, identifier: &str) {
    let Some(bot) = target(identifier) else {
        return;
    };
    let Some(window) = app.webview_windows().into_values().next() else {
        // No window is open: Bate opens one; the person finds the bot in its list.
        let _ = crate::open_window(app);
        return;
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    // `bot` is checked by `target`: nothing from outside goes into the script.
    let _ = window.eval(format!(
        "dispatchEvent(new CustomEvent('bate-open-bot', {{ detail: '{bot}' }}))"
    ));
}

struct Ivars {
    app: AppHandle,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[ivars = Ivars]
    #[name = "BateNotificationDelegate"]
    struct Delegate;

    unsafe impl NSObjectProtocol for Delegate {}

    unsafe impl UNUserNotificationCenterDelegate for Delegate {
        #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
        fn did_receive(
            &self,
            _center: &UNUserNotificationCenter,
            response: &UNNotificationResponse,
            done: &DynBlock<dyn Fn()>,
        ) {
            let id = response.notification().request().identifier().to_string();
            clicked(&self.ivars().app, &id);
            done.call(());
        }
    }
);

// SAFETY: the delegate holds an `AppHandle` (which is `Send` and `Sync`) and nothing else; the
// system calls it from its own queue.
unsafe impl Send for Delegate {}
unsafe impl Sync for Delegate {}

/// The notification centre keeps its delegate weakly.
static DELEGATE: OnceLock<Retained<Delegate>> = OnceLock::new();

/// Only an app in a bundle has a notification centre; a bare `bate` from a build folder has
/// none, and asking for it is an exception.
fn bundled() -> bool {
    NSBundle::mainBundle().bundleIdentifier().is_some()
}

/// Lets a click on a notification reach the app. Call once, at start.
pub fn listen(app: &AppHandle) {
    if !bundled() {
        return;
    }
    let delegate = DELEGATE.get_or_init(|| {
        let this = Delegate::alloc().set_ivars(Ivars { app: app.clone() });
        unsafe { msg_send![super(this), init] }
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .setDelegate(Some(ProtocolObject::from_ref(&**delegate)));
}

/// Shows "<title>" and "<body>" for `bot`'s open `question`. The first time, macOS asks the
/// user's leave; without it nothing is shown. While Bate is the frontmost app macOS shows no
/// banner: the row's badge is the notice then.
pub fn waiting(bot: &str, question: &str, title: &str, body: &str) -> Result<(), String> {
    if !is_bot_id(bot) {
        return Err("not a bot's id".into());
    }
    if !bundled() {
        return Err("notifications need Bate.app".into());
    }
    let (id, title, body) = (
        identifier(bot, question),
        title.to_owned(),
        body.to_owned(),
    );
    let granted = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        if !granted.as_bool() {
            return;
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&title));
        content.setBody(&NSString::from_str(&body));
        let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
            &NSString::from_str(&id),
            &content,
            None,
        );
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, None);
    });
    UNUserNotificationCenter::currentNotificationCenter()
        .requestAuthorizationWithOptions_completionHandler(UNAuthorizationOptions::Alert, &granted);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_click_finds_the_bot_in_the_identifier() {
        assert_eq!(target(&identifier("b_1-x", "q7")), Some("b_1-x"));
    }

    #[test]
    fn only_a_plain_bot_id_may_get_into_the_script_a_click_runs() {
        assert!(is_bot_id("3f9c-A_1"));
        for refused in ["", "a'b", "a b", "a\nb", "a);alert(1", &"a".repeat(65)] {
            assert!(!is_bot_id(refused), "{refused:?}");
        }
        assert_eq!(target("a'b\nq"), None);
        assert_eq!(target(""), None);
    }
}
