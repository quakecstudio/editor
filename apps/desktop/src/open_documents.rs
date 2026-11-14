// SPDX-License-Identifier: GPL-3.0-or-later

//! The files macOS asks the app to open: a double click or Open With in Finder, or files
//! dropped on its Dock icon. macOS sends them to the app delegate's `application:openURLs:`,
//! not on the command line, and winit 0.30's delegate has no such method, so the files would
//! be dropped and Finder would say the app cannot open them. [`install`] adds the method to
//! winit's delegate class, as `bevy_macos_open_files` does for Bevy; the files wait in a queue
//! that [`take`] empties each frame. winit 0.31 lets an app set its own delegate instead.
//!
//! This module is the app's one use of `unsafe` (Justin, 2026-11-14): adding a method to a
//! class is the Objective-C runtime's C API, and no crate does it for an eframe app.

use std::ffi::CStr;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock, PoisonError};

use eframe::egui;
use objc2::ffi::class_addMethod;
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{MainThreadMarker, sel};
use objc2_app_kit::NSApplication;
use objc2_foundation::{NSArray, NSURL};

/// The class winit 0.30 registers as the app delegate (`platform_impl/macos/app_state.rs`).
const WINIT_DELEGATE: &CStr = c"WinitApplicationDelegate";

/// The Objective-C type encoding of `-application:openURLs:`: returns nothing, and takes the
/// receiver, the selector and two objects.
const OPEN_URLS_TYPES: &CStr = c"v@:@@";

/// The files macOS has asked the app to open since [`take`] last ran. A static, since the
/// Objective-C runtime calls the delegate method with no Rust state.
static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// The window's context, to draw a frame when files arrive while the app is idle.
static CONTEXT: OnceLock<egui::Context> = OnceLock::new();

/// Why the app does not receive the files macOS opens with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenDocumentsError {
    /// [`install`] ran off the main thread.
    NotMainThread,
    /// winit's delegate class is not registered: the event loop is not built yet, or winit
    /// renamed it.
    NoDelegateClass,
    /// The delegate already has `application:openURLs:`, which is left as it is.
    AlreadyHandled,
}

impl std::fmt::Display for OpenDocumentsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NotMainThread => "not on the main thread",
            Self::NoDelegateClass => "winit's app delegate class is not registered",
            Self::AlreadyHandled => "the app delegate already handles application:openURLs:",
        })
    }
}

impl std::error::Error for OpenDocumentsError {}

/// Adds `application:openURLs:` to winit's app delegate. Call it after eframe's event loop is
/// built, which registers the class and sets the delegate, and before it runs, when macOS sends
/// the files the app was launched to open.
///
/// # Errors
///
/// [`OpenDocumentsError`] when the method could not be added; the app then runs without it.
pub fn install() -> Result<(), OpenDocumentsError> {
    let main = MainThreadMarker::new().ok_or(OpenDocumentsError::NotMainThread)?;
    let class = AnyClass::get(WINIT_DELEGATE).ok_or(OpenDocumentsError::NoDelegateClass)?;
    if !add_open_urls(class) {
        return Err(OpenDocumentsError::AlreadyHandled);
    }
    // NSApplication notes which methods its delegate has when the delegate is set, so winit's
    // is set again.
    let app = NSApplication::sharedApplication(main);
    let delegate = app.delegate();
    app.setDelegate(delegate.as_deref());
    Ok(())
}

/// Keeps the window's context, so files that arrive while the app is idle draw a frame.
pub fn set_context(ctx: &egui::Context) {
    // A second call keeps the first context: the app has one window.
    CONTEXT.get_or_init(|| ctx.clone());
}

/// The files macOS has asked the app to open since the last call, in the order it sent them.
#[must_use]
pub fn take() -> Vec<PathBuf> {
    std::mem::take(&mut *OPENED.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Adds `application:openURLs:` to `class`; false when it has the method already.
#[expect(
    unsafe_code,
    reason = "class_addMethod is the Objective-C runtime's C API; the function added takes \
              the arguments OPEN_URLS_TYPES describes"
)]
fn add_open_urls(class: &AnyClass) -> bool {
    let method: extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSArray<NSURL>) =
        application_open_urls;
    // SAFETY: `Imp` is an untyped method pointer; the runtime calls it with the receiver, the
    // selector and the two objects OPEN_URLS_TYPES names, which is `method`'s signature.
    // `class` is a registered class.
    unsafe {
        let method = std::mem::transmute::<
            extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSArray<NSURL>),
            Imp,
        >(method);
        class_addMethod(
            std::ptr::from_ref(class).cast_mut(),
            sel!(application:openURLs:),
            method,
            OPEN_URLS_TYPES.as_ptr(),
        )
        .as_bool()
    }
}

/// The files among `urls`; a URL that is not a file is not opened.
fn files(urls: &NSArray<NSURL>) -> Vec<PathBuf> {
    urls.iter()
        .filter(|url| url.isFileURL())
        .filter_map(|url| url.path())
        .map(|path| PathBuf::from(path.to_string()))
        .collect()
}

/// `-[WinitApplicationDelegate application:openURLs:]`, which AppKit calls with the files to
/// open.
#[expect(
    unsafe_code,
    reason = "AppKit passes the URLs as a raw pointer to an NSArray of NSURL"
)]
extern "C-unwind" fn application_open_urls(
    _delegate: *mut AnyObject,
    _selector: Sel,
    _application: *mut AnyObject,
    urls: *mut NSArray<NSURL>,
) {
    // SAFETY: AppKit passes an NSArray of NSURL, or nil, valid for the length of the call.
    let Some(urls) = (unsafe { urls.as_ref() }) else {
        return;
    };
    let files = files(urls);
    if files.is_empty() {
        return;
    }
    OPENED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .extend(files);
    if let Some(ctx) = CONTEXT.get() {
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    #![expect(
        clippy::unwrap_used,
        unsafe_code,
        reason = "test code: unwraps state the expected value, and the tests send the message \
                  AppKit sends"
    )]

    use std::sync::atomic::{AtomicUsize, Ordering};

    use objc2::rc::Retained;
    use objc2::runtime::{ClassBuilder, NSObject};
    use objc2::{ClassType, msg_send};
    use objc2_foundation::NSString;

    use super::*;

    /// The tests that read the queue, one at a time.
    static QUEUE: Mutex<()> = Mutex::new(());

    /// A class like winit's delegate, without the method; a name registers once, so each test
    /// has its own.
    fn delegate_class() -> &'static AnyClass {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let name = format!("QsTestDelegate{}", NEXT.fetch_add(1, Ordering::SeqCst));
        let name = std::ffi::CString::new(name).unwrap();
        ClassBuilder::new(&name, NSObject::class())
            .unwrap()
            .register()
    }

    fn instance(class: &AnyClass) -> Retained<AnyObject> {
        unsafe { msg_send![class, new] }
    }

    fn file(path: &str) -> Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(path))
    }

    fn link(url: &str) -> Retained<NSURL> {
        NSURL::URLWithString(&NSString::from_str(url)).unwrap()
    }

    /// Sends `application:openURLs:` to `delegate`, as AppKit does.
    fn open(delegate: &AnyObject, urls: &NSArray<NSURL>) {
        let application: *const AnyObject = std::ptr::null();
        let () = unsafe { msg_send![delegate, application: application, openURLs: urls] };
    }

    // Finder hands files as file URLs, which may be percent-encoded; a web link is not a file
    #[test]
    fn only_file_urls_are_files() {
        let urls = NSArray::from_retained_slice(&[
            file("/tmp/mod/world.qc"),
            link("https://example.com/a.qc"),
            link("file:///tmp/my%20maps/start.map"),
        ]);
        assert_eq!(
            files(&urls),
            [
                PathBuf::from("/tmp/mod/world.qc"),
                PathBuf::from("/tmp/my maps/start.map")
            ]
        );
    }

    // The method is added once; a delegate that has it keeps its own
    #[test]
    fn the_method_is_added_once() {
        let class = delegate_class();
        let delegate = instance(class);
        let responds = |delegate: &AnyObject| -> bool {
            unsafe { msg_send![delegate, respondsToSelector: sel!(application:openURLs:)] }
        };
        assert!(!responds(&delegate));
        assert!(add_open_urls(class));
        assert!(responds(&delegate));
        assert!(!add_open_urls(class));
    }

    // What AppKit sends waits for the next frame, in order; nothing queues for no files
    #[test]
    fn opened_files_wait_for_the_next_frame() {
        let _queue = QUEUE.lock().unwrap_or_else(PoisonError::into_inner);
        drop(take());
        let class = delegate_class();
        assert!(add_open_urls(class));
        let delegate = instance(class);
        open(&delegate, &NSArray::from_retained_slice(&[]));
        open(
            &delegate,
            &NSArray::from_retained_slice(&[link("https://x.y")]),
        );
        assert_eq!(take(), Vec::<PathBuf>::new());
        open(
            &delegate,
            &NSArray::from_retained_slice(&[file("/a/b.qc"), file("/a/c.map")]),
        );
        open(&delegate, &NSArray::from_retained_slice(&[file("/d")]));
        assert_eq!(
            take(),
            [
                PathBuf::from("/a/b.qc"),
                PathBuf::from("/a/c.map"),
                PathBuf::from("/d")
            ]
        );
        assert_eq!(take(), Vec::<PathBuf>::new());
    }
}
