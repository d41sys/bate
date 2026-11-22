//! The system's notification for a bot that needs the person, and what a click on it does.
//! The page asks (`notify_waiting`) when a question comes in that it is not showing; macOS asks
//! the user's leave itself, the first time one is due, not before.
//!
//! A notification the page marks `actions` has two buttons, "Allow once" and "Deny". They need no
//! entitlement, only the bundle every notification needs. A button does not bring the app forward,
//! so it is answered from here, on a connection of its own: it works with no window open
//! (Bate stays in the menu bar) and never leaves the Mac's own socket.

use block2::{DynBlock, RcBlock};
use objc2::rc::Retained;
use objc2::runtime::{Bool, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, MainThreadMarker, define_class, msg_send};
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSArray, NSBundle, NSError, NSObject, NSObjectProtocol, NSSet, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationAction,
    UNNotificationActionOptions, UNNotificationCategory, UNNotificationCategoryOptions,
    UNNotificationRequest, UNNotificationResponse, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};
use serde_json::{Value, json};
use std::sync::OnceLock;
use tauri::AppHandle;

/// The category of a notification with buttons, and the buttons' own identifiers.
const CATEGORY: &str = "bate.question";
const ALLOW: &str = "allow";
const DENY: &str = "deny";

/// A notification's identifier is the bot, the conversation and the question: every open window
/// asks for the same question, and the system keeps one notification per identifier, so there is
/// one banner. A button answers exactly this question of this conversation.
fn identifier(bot: &str, conversation: &str, question: &str) -> String {
    format!("{bot}\n{conversation}\n{question}")
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

/// The `answer` a button gives, as the service takes it: only "Approve" and "Deny", never
/// "Always" or "Conversation", and none for any other action (the body of the notification is a
/// click, not an answer).
fn answer_for(identifier: &str, action: &str) -> Option<Value> {
    let choice = match action {
        ALLOW => "Approve",
        DENY => "Deny",
        _ => return None,
    };
    let mut parts = identifier.splitn(3, '\n');
    let (bot, conversation, question) = (parts.next()?, parts.next()?, parts.next()?);
    (is_bot_id(bot) && is_bot_id(conversation) && !question.is_empty()).then(|| {
        json!({"type": "answer", "bot": bot, "conversation": conversation, "question": question, "choice": choice})
    })
}

/// The Dock's count of the conversations that need the person, from a `list_attention` reply:
/// the same rule as the Activity screen's "Needs you" (an open question).
fn needing(attention: &Value) -> usize {
    let items = attention["data"]["items"].as_array();
    items.map_or(0, |items| {
        items
            .iter()
            .filter(|i| i["questions"].as_array().is_some_and(|q| !q.is_empty()))
            .count()
    })
}

/// A button was pressed: answers on a connection of its own, off the thread macOS called on. An
/// answer that is too late (the question is closed or answered) does nothing; one the service
/// could not take opens the bot, so the person sees what is waiting.
fn answered(app: &AppHandle, identifier: &str, action: &str) {
    let Some(answer) = answer_for(identifier, action) else {
        return;
    };
    let (app, identifier) = (app.clone(), identifier.to_owned());
    std::thread::spawn(move || {
        let sent = crate::bridge::ask(
            &proto::paths::socket(),
            &[answer, json!({"type": "list_attention"})],
        );
        match sent {
            Ok(replies) if replies[0]["success"] == true => count(&app, needing(&replies[1])),
            Ok(replies) if replies[0]["code"] == "no_such_question" => {}
            _ => clicked(&app, &identifier),
        }
    });
}

/// The Dock's badge with no window open; a window's page sets it itself.
fn count(app: &AppHandle, n: usize) {
    if !crate::windows(app).is_empty() {
        return;
    }
    let _ = app.run_on_main_thread(move || {
        if let Some(main) = MainThreadMarker::new() {
            let label = (n > 0).then(|| NSString::from_str(&n.to_string()));
            NSApplication::sharedApplication(main)
                .dockTile()
                .setBadgeLabel(label.as_deref());
        }
    });
}

/// A click: an open window comes forward and opens the bot; with none open, one opens on it.
fn clicked(app: &AppHandle, identifier: &str) {
    if let Some(bot) = target(identifier) {
        crate::open_bot(app, bot);
    }
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
            match response.actionIdentifier().to_string().as_str() {
                action @ (ALLOW | DENY) => answered(&self.ivars().app, &id, action),
                // The body: today's click.
                _ => clicked(&self.ivars().app, &id),
            }
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
    let center = UNUserNotificationCenter::currentNotificationCenter();
    center.setDelegate(Some(ProtocolObject::from_ref(&**delegate)));
    let button = |id: &str, title: &str, options| {
        UNNotificationAction::actionWithIdentifier_title_options(
            &NSString::from_str(id),
            &NSString::from_str(title),
            options,
        )
    };
    let actions = NSArray::from_retained_slice(&[
        button(ALLOW, "Allow once", UNNotificationActionOptions::empty()),
        button(DENY, "Deny", UNNotificationActionOptions::Destructive),
    ]);
    let category = UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
        &NSString::from_str(CATEGORY),
        &actions,
        &NSArray::new(),
        UNNotificationCategoryOptions::empty(),
    );
    center.setNotificationCategories(&NSSet::from_retained_slice(&[category]));
}

/// Shows "<title>" and "<body>" for `bot`'s open `question` of `conversation`, with the two
/// buttons when `actions`. The first time, macOS asks the user's leave; without it nothing is
/// shown. While Bate is the frontmost app macOS shows no banner: the row's badge is the notice
/// then.
pub fn waiting(
    bot: &str,
    conversation: &str,
    question: &str,
    title: &str,
    body: &str,
    actions: bool,
) -> Result<(), String> {
    if !is_bot_id(bot) || !is_bot_id(conversation) {
        return Err("not a bot's or conversation's id".into());
    }
    if !bundled() {
        return Err("notifications need Bate.app".into());
    }
    let id = identifier(bot, conversation, question);
    let (title, body) = (title.to_owned(), body.to_owned());
    let granted = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
        if !granted.as_bool() {
            return;
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&title));
        content.setBody(&NSString::from_str(&body));
        if actions {
            content.setCategoryIdentifier(&NSString::from_str(CATEGORY));
        }
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
        assert_eq!(target(&identifier("b_1-x", "c1", "q7")), Some("b_1-x"));
    }

    #[test]
    fn a_button_answers_that_question_with_allow_once_or_deny_and_nothing_else() {
        let id = identifier("b1", "c9", "q\nwith a line break");
        assert_eq!(
            answer_for(&id, ALLOW),
            Some(
                json!({"type": "answer", "bot": "b1", "conversation": "c9", "question": "q\nwith a line break", "choice": "Approve"})
            )
        );
        assert_eq!(answer_for(&id, DENY).unwrap()["choice"], "Deny");
        // The notification's body (a click), a button of the system's, and any other word.
        for action in [
            "com.apple.UNNotificationDefaultActionIdentifier",
            "Always",
            "Conversation",
            "",
        ] {
            assert_eq!(answer_for(&id, action), None, "{action:?}");
        }
        // An identifier that does not name a bot, a conversation and a question answers nothing.
        for bad in ["b1\nc9", "b1\nc9\n", "a'b\nc9\nq1", "b1\nc 9\nq1", ""] {
            assert_eq!(answer_for(bad, ALLOW), None, "{bad:?}");
        }
    }

    #[test]
    fn the_dock_counts_the_conversations_with_an_open_question() {
        let reply = json!({"data": {"items": [
            {"questions": [{"question": "q1"}]},
            {"questions": []},
            {"questions": [{"question": "q2"}, {"question": "q3"}]},
        ]}});
        assert_eq!(needing(&reply), 2);
        assert_eq!(needing(&json!({"success": false})), 0);
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
