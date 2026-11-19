//! Notifications under Sokki's own name and icon (UserNotifications).
//!
//! `osascript -e 'display notification …'` shows up as "Script Editor", so
//! people can't tell the banner came from Sokki, nor turn it off for Sokki in
//! System Settings. Apple's notification centre needs a real app bundle and
//! the person's permission (asked once, by macOS). Outside a bundle (dev
//! runs, tests) or when macOS gives an error, the caller falls back to
//! osascript. If the person said no, nothing is shown: their choice holds.

use std::sync::OnceLock;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{define_class, msg_send, AnyThread};
use objc2_foundation::{NSBundle, NSError, NSString};
use objc2_user_notifications::{
    UNAuthorizationOptions, UNMutableNotificationContent, UNNotification,
    UNNotificationPresentationOptions, UNNotificationRequest, UNUserNotificationCenter,
    UNUserNotificationCenterDelegate,
};

define_class!(
    // SAFETY: NSObject has no subclassing requirements; no ivars.
    #[unsafe(super(NSObject))]
    #[name = "SokkiNotificationDelegate"]
    struct ForegroundDelegate;

    unsafe impl NSObjectProtocol for ForegroundDelegate {}

    unsafe impl UNUserNotificationCenterDelegate for ForegroundDelegate {
        // Without this, macOS hides banners while Sokki is the active app,
        // e.g. "Meeting note ready" while its window is open.
        #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
        fn will_present(
            &self,
            _center: &UNUserNotificationCenter,
            _notification: &UNNotification,
            completion_handler: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
        ) {
            completion_handler.call((UNNotificationPresentationOptions(
                UNNotificationPresentationOptions::Banner.0
                    | UNNotificationPresentationOptions::List.0,
            ),));
        }
    }
);

/// The notification centre keeps its delegate weakly; this keeps it alive.
struct Kept(#[allow(dead_code)] Retained<ForegroundDelegate>);
// SAFETY: the delegate has no state and is only ever handed to the centre.
unsafe impl Send for Kept {}
unsafe impl Sync for Kept {}
static DELEGATE: OnceLock<Kept> = OnceLock::new();

/// Running from an app bundle with an identifier: the centre crashes otherwise.
fn in_app_bundle() -> bool {
    let bundle = NSBundle::mainBundle();
    bundle.bundleIdentifier().is_some() && bundle.bundlePath().to_string().ends_with(".app")
}

fn center() -> Retained<UNUserNotificationCenter> {
    let center = UNUserNotificationCenter::currentNotificationCenter();
    let delegate = DELEGATE.get_or_init(|| {
        let d: Retained<ForegroundDelegate> =
            unsafe { msg_send![ForegroundDelegate::alloc(), init] };
        Kept(d)
    });
    center.setDelegate(Some(ProtocolObject::from_ref(&*delegate.0)));
    center
}

/// Shows a banner as Sokki. Returns false when it can't (no app bundle, or
/// macOS reports an error); the caller then uses osascript. The first call
/// makes macOS ask for permission; a "no" is respected silently.
pub fn notify(title: &str, body: &str, fallback: fn(&str, &str)) -> bool {
    if !in_app_bundle() {
        return false;
    }
    let center = center();
    let (title, body) = (title.to_string(), body.to_string());
    let deliver = RcBlock::new(move |granted: Bool, error: *mut NSError| {
        if !error.is_null() {
            log::warn!("Notifications: permission check failed, using osascript");
            fallback(&title, &body);
            return;
        }
        if !granted.as_bool() {
            log::info!("Notifications: turned off for Sokki, not shown");
            return;
        }
        let content = UNMutableNotificationContent::new();
        content.setTitle(&NSString::from_str(&title));
        content.setBody(&NSString::from_str(&body));
        let id = NSString::from_str(&format!(
            "sokki-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let request =
            UNNotificationRequest::requestWithIdentifier_content_trigger(&id, &content, None);
        let (t, b) = (title.clone(), body.clone());
        let added = RcBlock::new(move |error: *mut NSError| {
            if !error.is_null() {
                log::warn!("Notifications: not delivered, using osascript");
                fallback(&t, &b);
            }
        });
        UNUserNotificationCenter::currentNotificationCenter()
            .addNotificationRequest_withCompletionHandler(&request, Some(&added));
    });
    // Asks only the first time; later calls answer at once.
    center.requestAuthorizationWithOptions_completionHandler(
        UNAuthorizationOptions(UNAuthorizationOptions::Alert.0 | UNAuthorizationOptions::Sound.0),
        &deliver,
    );
    true
}
