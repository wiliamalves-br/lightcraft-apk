//! LightCraft for Android.
//!
//! The same egui UI (`lightcraft-ui-egui`) and engine as the desktop app, run in an Android
//! `NativeActivity` (`android/`, the Gradle project that packages the APK). What differs from the
//! desktop host (`apps/lightcraft`) is only what the platform gives:
//!
//! - **Files.** Android has no native file dialog a native activity can wait on, so the host draws
//!   its own picker in the window ([`browser`]): any number of photos or whole folders for import,
//!   preset files, export and library folders. It reads shared storage directly, which the Java
//!   activity asks the user to allow once ("All files access").
//! - **Touch.** Long-press is the secondary click (context menus); a small tool button
//!   ([`host`]) adds what a keyboard would: sticky Ctrl / Shift for multi-select, interface zoom,
//!   Undo, Select All, and shortcuts to Import, Export and Presets. Back is Esc.
//! - **Places.** Settings and the library live in the app's private storage; exports default to
//!   `Pictures/LightCraft Exports` on the shared storage, where the gallery sees them.
#![cfg(target_os = "android")]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod browser;
pub mod host;
mod logger;

use winit::platform::android::activity::AndroidApp;

/// The NativeActivity entry point (android-activity calls it on its own thread).
#[allow(unsafe_code)]
// SAFETY: `android_main` is the symbol android-activity's glue looks up and calls exactly once; it
// has the signature the glue expects and is defined nowhere else in the library.
#[unsafe(no_mangle)]
fn android_main(app: AndroidApp) {
    let places = host::Places::new(app.internal_data_path(), app.external_data_path());
    // before any thread starts: the engine reads these to find its folders
    set_environment(&places);
    let locale = app.config().language().map(|language| match app.config().country() {
        Some(country) => format!("{language}-{country}"),
        None => language,
    });
    if let Err(e) = host::run(places, locale, app) {
        log::error!("LightCraft stopped: {e}");
    }
}

/// Point the engine at Android's folders. It finds them through the environment, as on the
/// desktop: settings and models in `LIGHTCRAFT_CONFIG_DIR`, temporary files in `TMPDIR` (Rust's
/// default on Android, `/data/local/tmp`, is not writable by apps) and the default export folder
/// under `HOME/Pictures`.
#[allow(unsafe_code)]
fn set_environment(places: &host::Places) {
    let vars = [("LIGHTCRAFT_CONFIG_DIR", &places.config), ("TMPDIR", &places.tmp), ("HOME", &places.home)];
    for (name, value) in vars {
        let _ = std::fs::create_dir_all(value);
        // SAFETY: called first thing in `android_main`, before this process starts any thread of
        // its own (logging, renderer, workers), so nothing reads the environment concurrently.
        unsafe { std::env::set_var(name, value) };
    }
}
