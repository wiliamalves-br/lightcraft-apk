//! The Android host: opens the library, gives the UI its platform services (the in-window file
//! picker for every dialog), and adds what a touch screen lacks — a tool button with sticky
//! Ctrl / Shift for multi-select, interface zoom and the everyday commands; Back as Esc.

use crate::browser::{Browser, Outcome};
use lightcraft_engine::Session;
use lightcraft_ui_egui::i18n::{self, Locale};
use lightcraft_ui_egui::panels::library_problem::LibraryProblem;
use lightcraft_ui_egui::pick::{self, PickRequest};
use lightcraft_ui_egui::{LightcraftApp, PickFiles, PickFolder, SaveFile, Services, UiState};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use winit::platform::android::activity::AndroidApp;

/// Where things live on the device.
pub struct Places {
    /// The app's private storage (`/data/user/0/<package>/files`).
    pub private: PathBuf,
    /// Settings, logs, models (`LIGHTCRAFT_CONFIG_DIR`).
    pub config: PathBuf,
    /// Temporary files (`TMPDIR`).
    pub tmp: PathBuf,
    /// The shared storage (`/storage/emulated/0`): `HOME`, so exports default to its Pictures folder.
    pub home: PathBuf,
    /// The library used until another one is opened in Settings.
    pub library: PathBuf,
}

impl Places {
    pub fn new(internal: Option<PathBuf>, external: Option<PathBuf>) -> Places {
        let private = internal.or(external).unwrap_or_else(|| PathBuf::from("/data/local/tmp/lightcraft"));
        let shared = PathBuf::from("/storage/emulated/0");
        let home = if shared.is_dir() { shared } else { private.clone() };
        Places { config: private.join("config"), tmp: private.join("tmp"), library: private.join("LightCraft Library"), home, private }
    }
}

// ---------------------------------------------------------------------------------------------
// The file picker requests, shared between the UI's services and the window that draws them.

/// How a closed picker answers.
enum Reply {
    /// A command waits for it (`Services::picker`, see `lightcraft_ui_egui::pick`).
    Channel(Sender<Vec<String>>),
    /// A button inside a dialog asked through a synchronous service, which returned nothing at
    /// once. The answer is kept for `tag`, and the tap that asked (at `at`) is replayed, so the
    /// button asks again and gets it.
    Deferred { tag: &'static str, at: Option<egui::Pos2> },
}

struct Active {
    req: PickRequest,
    reply: Reply,
    /// Not shown yet (the picker starts in the place that fits the request).
    fresh: bool,
}

struct Ready {
    tag: &'static str,
    paths: Vec<String>,
    since: Instant,
}

#[derive(Default)]
struct Shared {
    active: Option<Active>,
    ready: Option<Ready>,
    /// Where the last primary press was (the tap a deferred answer replays).
    last_press: Option<egui::Pos2>,
}

type SharedRef = Arc<Mutex<Shared>>;

fn lock(shared: &SharedRef) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A synchronous service's answer: the paths chosen for `tag` if the picker just closed with
/// them, else nothing now — the picker opens, and the tap is replayed when it closes.
fn deferred(shared: &SharedRef, ctx: &egui::Context, tag: &'static str, req: PickRequest) -> Vec<String> {
    let mut s = lock(shared);
    if let Some(ready) = s.ready.take_if(|r| r.tag == tag)
        && ready.since.elapsed() < Duration::from_secs(10)
    {
        return ready.paths;
    }
    if s.active.is_none() {
        let at = s.last_press;
        s.active = Some(Active { req, reply: Reply::Deferred { tag, at }, fresh: true });
        ctx.request_repaint();
    }
    Vec::new()
}

fn sync_files(shared: &SharedRef, ctx: &egui::Context, tag: &'static str, req: PickRequest) -> PickFiles {
    let (shared, ctx) = (shared.clone(), ctx.clone());
    Box::new(move || deferred(&shared, &ctx, tag, req.clone()))
}

fn sync_one(shared: &SharedRef, ctx: &egui::Context, tag: &'static str, req: PickRequest) -> PickFolder {
    let (shared, ctx) = (shared.clone(), ctx.clone());
    Box::new(move || deferred(&shared, &ctx, tag, req.clone()).into_iter().next())
}

fn sync_save(shared: &SharedRef, ctx: &egui::Context, tag: &'static str, title: String, filter: String, exts: &'static [&'static str]) -> SaveFile {
    let (shared, ctx) = (shared.clone(), ctx.clone());
    Box::new(move |name: &str| deferred(&shared, &ctx, tag, PickRequest::save(title.clone(), filter.clone(), exts, name)).into_iter().next())
}

/// The platform services: every file dialog is the in-window picker.
fn services(ctx: &egui::Context, shared: &SharedRef, log_file: Option<&Path>) -> Services {
    let t = |s: &str| i18n::tr(s).to_string();
    let photos = lightcraft_engine::import::EXTENSIONS;
    let starter = {
        let (shared, ctx) = (shared.clone(), ctx.clone());
        move |req: PickRequest| {
            let (tx, rx) = std::sync::mpsc::channel();
            let mut s = lock(&shared);
            if s.active.is_some() {
                return Err("another file picker is open".to_string());
            }
            s.active = Some(Active { req, reply: Reply::Channel(tx), fresh: true });
            ctx.request_repaint();
            Ok(rx)
        }
    };
    Services {
        picker: Some(Box::new(starter)),
        pick_files: Some(sync_files(shared, ctx, "photos", PickRequest::files(None, t("Photos"), photos))),
        pick_folder: Some(sync_one(shared, ctx, "folder", PickRequest::folder(t("Choose Folder")))),
        pick_lightroom_catalog: Some(sync_one(
            shared,
            ctx,
            "lrcat",
            PickRequest::file(t("Import Lightroom Catalog"), t("Lightroom Classic Catalog"), &["lrcat"]),
        )),
        pick_denoise_model: Some(sync_files(shared, ctx, "denoise", PickRequest::file(t("ONNX model"), t("ONNX model"), &["onnx"]))),
        pick_model_file: Some(sync_files(
            shared,
            ctx,
            "faceModel",
            PickRequest::file(t("Add a Face Recognition Model"), t("Face models (ONNX)"), &["onnx"]),
        )),
        pick_preset_files: Some(sync_files(
            shared,
            ctx,
            "presets",
            PickRequest::files(Some(t("Import Presets")), t("Presets & Profiles"), pick::PRESET_EXTENSIONS),
        )),
        pick_tracklog: Some(sync_files(shared, ctx, "gpx", PickRequest::file(t("Auto-Tag from Tracklog"), t("GPS Track Log"), &["gpx"]))),
        pick_display_profile: Some(sync_files(
            shared,
            ctx,
            "icc",
            PickRequest::file(t("Choose Monitor Profile"), t("ICC Profiles"), &["icc", "icm"]),
        )),
        save_preset_file: Some(sync_save(shared, ctx, "savePreset", t("Export Presets"), t("LightCraft Preset"), &["lcpreset"])),
        pick_keyword_list: Some(sync_files(shared, ctx, "keywords", PickRequest::file(t("Import Keywords"), t("Keyword Lists"), &["txt", "utf8"]))),
        save_keyword_list: Some(sync_save(shared, ctx, "saveKeywords", t("Export Keywords"), t("Keyword Lists"), &["txt"])),
        pick_curve_preset_files: Some(sync_files(
            shared,
            ctx,
            "curves",
            PickRequest::files(Some(t("Import Point Curve Presets")), t("Point Curve Presets"), pick::CURVE_PRESET_EXTENSIONS),
        )),
        save_curve_preset_file: Some(sync_save(shared, ctx, "saveCurves", t("Export Point Curve Presets"), t("Point Curve Presets"), &["lccurve"])),
        // atomic (temp file + sync + rename): a failed write never leaves a truncated file
        write_shared: Some(Arc::new(lightcraft_engine::export::write_file)),
        write: Some(Box::new(lightcraft_engine::export::write_file)),
        png: Some(Box::new(|img: &lightcraft_raster::Rgba8| {
            lightcraft_codecs::encode_png(&lightcraft_codecs::EncodeImage::rgba8(img), &lightcraft_codecs::EncodeMeta::default()).unwrap_or_default()
        })),
        log_file: log_file.map(|p| p.display().to_string()),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------------------------
// Settings files.

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, path)
}

/// `ui.json`, the UI state and app settings, and a warning when it exists but can't be used (it is
/// kept as `ui.json.corrupt-<time>`).
fn load_prefs(path: &Path) -> (Option<UiState>, Option<String>) {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (None, None),
        Err(e) => return (None, Some(format!("As configurações ({}) não puderam ser lidas: {e}.", path.display()))),
    };
    match serde_json::from_slice::<UiState>(&bytes) {
        Ok(ui) => (Some(ui.sanitized()), None),
        Err(e) => {
            let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let _ = std::fs::rename(path, path.with_file_name(format!("ui.json.corrupt-{secs}")));
            (None, Some(format!("As configurações estavam danificadas ({e}); foram guardadas à parte e os padrões estão em uso.")))
        }
    }
}

/// Saves `ui.json` when it changed: every few seconds, when the app leaves the screen, at exit.
#[derive(Default)]
struct PrefsWriter {
    path: Option<PathBuf>,
    written: Vec<u8>,
    checked: f64,
    failing: bool,
}

impl PrefsWriter {
    /// `temporary`: the library couldn't be opened and the session promises to save nothing.
    fn save(&mut self, ui: &UiState, temporary: bool) -> Result<(), String> {
        if temporary {
            return Ok(());
        }
        let Some(path) = self.path.clone() else { return Ok(()) };
        if let Some(workspace) = &ui.docking {
            workspace.validate()?;
        }
        let bytes = serde_json::to_vec_pretty(ui).map_err(|e| e.to_string())?;
        if bytes == self.written {
            return Ok(());
        }
        write_atomic(&path, &bytes).map_err(|e| format!("saving the app settings failed: {e}"))?;
        self.written = bytes;
        Ok(())
    }

    fn tick(&mut self, app: &mut LightcraftApp, now: f64) {
        if now - self.checked < 3.0 {
            return;
        }
        self.checked = now;
        match self.save(&app.ui, app.library_problem.is_some()) {
            Ok(()) => self.failing = false,
            Err(e) => {
                if !self.failing {
                    log::warn!("{e}");
                    app.notices.push(format!("{e}. LightCraft keeps trying."));
                }
                self.failing = true;
            }
        }
    }
}

/// The Android host's own settings (`android.json`).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
struct AndroidPrefs {
    /// Interface zoom; `None` = fit the screen.
    #[serde(default)]
    zoom: Option<f32>,
    /// The tool button sits at the right edge instead of the left.
    #[serde(default)]
    tools_right: bool,
}

// ---------------------------------------------------------------------------------------------
// The library session.

fn new_session() -> Session {
    Session::new().with_fs().with_default_denoise_models().with_system_clock()
}

/// The library at `dir`; if it can't be opened, an empty in-memory session and the problem (the
/// window then asks what to do: try again, choose another library, continue without saving).
fn open_session(dir: &Path) -> (Session, Option<LibraryProblem>) {
    let mut s = new_session();
    let opened = s.open_library(dir, false).map(|_| ()).map_err(|e| e.to_string());
    let (mut s, problem) = match opened {
        Ok(()) => {
            log::info!("library {}: {} photos", dir.display(), s.catalog.len());
            (s, None)
        }
        Err(e) => {
            log::error!("can't open library {}: {e}", dir.display());
            (new_session(), Some(LibraryProblem::new(dir.to_string_lossy(), e)))
        }
    };
    s.face_models_dir = lightcraft_engine::config::default_face_models_dir();
    s.apple_photos_background = true;
    (s.with_default_sam3_model(), problem)
}

// ---------------------------------------------------------------------------------------------
// The window.

const EXIT_WAIT: Duration = Duration::from_secs(2);

struct Host {
    app: LightcraftApp,
    shared: SharedRef,
    browser: Browser,
    prefs: PrefsWriter,
    android: AndroidPrefs,
    android_path: PathBuf,
    android_written: AndroidPrefs,
    tools_open: bool,
    sticky_ctrl: bool,
    sticky_shift: bool,
    /// Tell egui once that the sticky modifiers were released.
    modifiers_reset: bool,
    /// A tap to replay at this point: frames to wait first, then press (stage 1) and release (2).
    replay: Option<(egui::Pos2, u8)>,
    zoom_applied: bool,
    focused: bool,
}

impl Host {
    fn save_all(&mut self) {
        if let Err(e) = self.prefs.save(&self.app.ui, self.app.library_problem.is_some()) {
            log::warn!("{e}");
        }
        if let Err(e) = self.app.session.persist() {
            log::warn!("saving the library failed: {e}");
        }
        self.save_android();
    }

    fn save_android(&mut self) {
        if self.android == self.android_written {
            return;
        }
        match serde_json::to_vec_pretty(&self.android)
            .map_err(|e| e.to_string())
            .and_then(|b| write_atomic(&self.android_path, &b).map_err(|e| e.to_string()))
        {
            Ok(()) => self.android_written = self.android.clone(),
            Err(e) => log::warn!("saving android.json failed: {e}"),
        }
    }

    fn run(&mut self, id: &str) {
        if let Err(e) = self.app.run(id, serde_json::json!({})) {
            self.app.notices.push(e);
        }
    }

    /// The picker closed: answer whoever asked.
    fn finish(&mut self, paths: Vec<String>, ctx: &egui::Context) {
        let Some(active) = lock(&self.shared).active.take() else { return };
        match active.reply {
            Reply::Channel(tx) => {
                // the command may have gone away meanwhile: nobody to tell
                let _ = tx.send(paths);
            }
            Reply::Deferred { tag, at } => {
                if !paths.is_empty() {
                    lock(&self.shared).ready = Some(Ready { tag, paths, since: Instant::now() });
                    match at {
                        Some(pos) => self.replay = Some((pos, 0)),
                        None => self.app.notices.push("Escolhido. Toque no botão de novo para usar.".to_string()),
                    }
                }
            }
        }
        ctx.request_repaint();
    }

    fn picker(&mut self, ctx: &egui::Context) -> bool {
        let shown = {
            let mut s = lock(&self.shared);
            s.active.as_mut().map(|a| (a.req.clone(), std::mem::replace(&mut a.fresh, false)))
        };
        let Some((req, fresh)) = shown else { return false };
        if fresh {
            self.browser.begin(&req);
        }
        match self.browser.show(ctx, &req) {
            Outcome::Open => {}
            Outcome::Chosen(paths) => self.finish(paths, ctx),
            Outcome::Cancelled => self.finish(Vec::new(), ctx),
        }
        true
    }

    fn set_zoom(&mut self, ctx: &egui::Context, zoom: f32) {
        let zoom = (zoom * 20.0).round() / 20.0;
        let zoom = zoom.clamp(0.4, 2.0);
        ctx.set_zoom_factor(zoom);
        self.android.zoom = Some(zoom);
    }

    /// The tool button and, when open, its panel.
    fn tools(&mut self, ctx: &egui::Context) {
        let (align, offset) = if self.android.tools_right {
            (egui::Align2::RIGHT_BOTTOM, egui::vec2(-10.0, -10.0))
        } else {
            (egui::Align2::LEFT_BOTTOM, egui::vec2(10.0, -10.0))
        };
        let mut action: Option<&'static str> = None;
        egui::Area::new(egui::Id::new("lightcraft-android-tools")).order(egui::Order::Foreground).anchor(align, offset).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                let big = |text: &str| egui::Button::new(egui::RichText::new(text).size(16.0)).min_size(egui::vec2(170.0, 40.0));
                if self.tools_open {
                    ui.vertical(|ui| {
                        for (label, id) in [
                            ("Importar fotos…", "file.addPhotos"),
                            ("Importar pasta…", "file.addFolder"),
                            ("Exportar…", "dialog.export"),
                            ("Importar presets…", "file.importPresets"),
                            ("Colar ajustes…", "dialog.pasteSettings"),
                            ("Selecionar tudo", "library.selectAll"),
                            ("Desfazer", "edit.undo"),
                            ("Refazer", "edit.redo"),
                            ("Configurações", "app.settings"),
                        ] {
                            if ui.add(big(label)).clicked() {
                                action = Some(id);
                            }
                        }
                        ui.separator();
                        if ui
                            .add(
                                egui::Button::selectable(self.sticky_ctrl, egui::RichText::new("Ctrl: tocar soma à seleção").size(15.0))
                                    .min_size(egui::vec2(170.0, 38.0)),
                            )
                            .clicked()
                        {
                            self.sticky_ctrl = !self.sticky_ctrl;
                            self.modifiers_reset = true;
                        }
                        if ui
                            .add(
                                egui::Button::selectable(self.sticky_shift, egui::RichText::new("Shift: selecionar intervalo").size(15.0))
                                    .min_size(egui::vec2(170.0, 38.0)),
                            )
                            .clicked()
                        {
                            self.sticky_shift = !self.sticky_shift;
                            self.modifiers_reset = true;
                        }
                        ui.horizontal(|ui| {
                            let zoom = ctx.zoom_factor();
                            if ui.add(egui::Button::new(egui::RichText::new("A−").size(18.0)).min_size(egui::vec2(48.0, 40.0))).clicked() {
                                self.set_zoom(ctx, zoom - 0.05);
                            }
                            ui.label(egui::RichText::new(format!("{:.0}%", zoom * 100.0)).size(15.0));
                            if ui.add(egui::Button::new(egui::RichText::new("A+").size(18.0)).min_size(egui::vec2(48.0, 40.0))).clicked() {
                                self.set_zoom(ctx, zoom + 0.05);
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.add(egui::Button::new(egui::RichText::new("Teclado").size(15.0)).min_size(egui::vec2(82.0, 38.0))).clicked() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::IMEAllowed(true));
                            }
                            let side = if self.android.tools_right { "← Lado" } else { "Lado →" };
                            if ui.add(egui::Button::new(egui::RichText::new(side).size(15.0)).min_size(egui::vec2(82.0, 38.0))).clicked() {
                                self.android.tools_right = !self.android.tools_right;
                            }
                        });
                    });
                }
                let mut label = if self.tools_open { "×".to_string() } else { "•••".to_string() };
                if !self.tools_open && (self.sticky_ctrl || self.sticky_shift) {
                    label = format!("••• {}{}", if self.sticky_ctrl { "Ctrl " } else { "" }, if self.sticky_shift { "Shift" } else { "" });
                }
                if ui.add(egui::Button::new(egui::RichText::new(label).size(20.0).strong()).min_size(egui::vec2(56.0, 44.0))).clicked() {
                    self.tools_open = !self.tools_open;
                }
            });
        });
        if let Some(id) = action {
            self.tools_open = false;
            self.run(id);
        }
    }
}

impl eframe::App for Host {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.zoom_applied {
            let rect = ctx.viewport_rect();
            if rect.width() > 100.0 && rect.height() > 100.0 {
                // fit the desktop layout (made for ≥ 1280 × 680 points) on the screen
                let fit = (rect.width() / 1280.0).min(rect.height() / 680.0).clamp(0.5, 1.0);
                let zoom = self.android.zoom.unwrap_or(fit);
                ctx.set_zoom_factor(zoom.clamp(0.4, 2.0));
                self.zoom_applied = true;
            }
        }
        self.app.logic(ctx);
        let (now, focused) = ctx.input(|i| (i.time, i.focused));
        self.prefs.tick(&mut self.app, now);
        if self.focused && !focused {
            // leaving the screen: Android may end the process without another word
            self.save_all();
        }
        self.focused = focused;
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        let picking = lock(&self.shared).active.is_some();
        let mut back = false;
        for ev in raw.events.iter_mut() {
            match ev {
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, .. } => {
                    lock(&self.shared).last_press = Some(*pos);
                }
                egui::Event::Key { key, pressed, .. } if *key == egui::Key::BrowserBack => {
                    if *pressed {
                        back = true;
                    }
                    // Back is Esc: it closes dialogs and menus, leaves full screen…
                    *key = egui::Key::Escape;
                }
                _ => {}
            }
        }
        if back && (picking || self.tools_open) {
            // Back closes the picker or the tool panel, and nothing else
            raw.events.retain(|e| !matches!(e, egui::Event::Key { key: egui::Key::Escape, .. }));
            if picking {
                if let Some(active) = lock(&self.shared).active.take()
                    && let Reply::Channel(tx) = active.reply
                {
                    let _ = tx.send(Vec::new());
                }
            } else {
                self.tools_open = false;
            }
        }
        // sticky modifiers
        if self.sticky_ctrl || self.sticky_shift || self.modifiers_reset {
            self.modifiers_reset = false;
            let add = |m: &mut egui::Modifiers, ctrl: bool, shift: bool| {
                m.ctrl |= ctrl;
                m.command |= ctrl;
                m.shift |= shift;
            };
            let (ctrl, shift) = (self.sticky_ctrl, self.sticky_shift);
            for ev in raw.events.iter_mut() {
                match ev {
                    egui::Event::PointerButton { modifiers, .. } | egui::Event::Key { modifiers, .. } | egui::Event::ModifiersChanged(modifiers) => {
                        add(modifiers, ctrl, shift);
                    }
                    _ => {}
                }
            }
            let mut m = egui::Modifiers::default();
            add(&mut m, ctrl, shift);
            raw.events.insert(0, egui::Event::ModifiersChanged(m));
        }
        // a deferred picker's tap, replayed once the picker is gone
        if let Some((pos, stage)) = self.replay {
            let modifiers = egui::Modifiers::default();
            match stage {
                0 | 1 => self.replay = Some((pos, stage + 1)),
                2 => {
                    raw.events.push(egui::Event::PointerMoved(pos));
                    raw.events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: true, modifiers });
                    self.replay = Some((pos, 3));
                }
                _ => {
                    raw.events.push(egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed: false, modifiers });
                    self.replay = None;
                }
            }
        }
        self.app.raw_input_hook(raw);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.app.ui(ui);
        let ctx = ui.ctx().clone();
        let picking = self.picker(&ctx);
        if !picking {
            self.tools(&ctx);
        }
        if self.replay.is_some() {
            ctx.request_repaint();
        }
        self.save_android();
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_all();
        if let Err(e) = self.app.session.close_library() {
            log::error!("saving the library failed: {e}");
        }
        if !self.app.shutdown(EXIT_WAIT) {
            log::warn!("quit: background work was still running after {EXIT_WAIT:?}");
        }
    }
}

/// Run the app in the activity's window until Android closes it.
pub fn run(places: Places, locale: Option<String>, android_app: AndroidApp) -> eframe::Result {
    let log_file = crate::logger::install(&places.config.join("logs"));
    lightcraft_engine::guard::install_hook(places.config.join("lightcraft-panics.log"));
    log::info!("LightCraft {} for Android, settings in {}", env!("CARGO_PKG_VERSION"), places.config.display());

    let prefs_path = places.config.join("ui.json");
    let (prefs, prefs_warning) = load_prefs(&prefs_path);
    // the language first: the services' dialog titles are translated when they are made
    let language = match &prefs {
        Some(ui) => Some(ui.language),
        None => locale.as_deref().and_then(Locale::parse_tag),
    };
    if let Some(language) = language {
        i18n::set_language(language);
    }
    let library_dir = prefs
        .as_ref()
        .map(|u| u.settings.library_path.clone())
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| places.library.clone());

    // GPU compute: off if the preference says so, or if the last launch died creating the device
    use lightcraft_engine::gpu::backend;
    let marker = places.config.join("gpu-init.marker");
    let gpu_crash = backend::take_init_marker(&marker);
    backend::set_init_marker(Some(marker));
    let gpu_on = prefs.as_ref().is_none_or(|u| u.settings.gpu) && gpu_crash.is_none();
    lightcraft_engine::gpu::set_enabled(gpu_on);

    let android_path = places.config.join("android.json");
    let android: AndroidPrefs = std::fs::read(&android_path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let (home, private) = (places.home.clone(), places.private.clone());

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        android_app: Some(android_app),
        viewport: egui::ViewportBuilder::default().with_title("LightCraft"),
        ..Default::default()
    };
    eframe::run_native(
        "LightCraft",
        options,
        Box::new(move |cc| {
            let (session, problem) = open_session(&library_dir);
            let shared: SharedRef = Arc::new(Mutex::new(Shared::default()));
            let mut app = LightcraftApp::new(session, services(&cc.egui_ctx, &shared, log_file.as_deref()));
            if let Some(ui) = prefs {
                app.ui = ui;
            }
            if let Some(language) = language {
                app.ui.language = language;
            }
            i18n::set_language(app.ui.language);
            app.integrated_titlebar = false;
            app.notices.extend(prefs_warning);
            if let Some(what) = gpu_crash {
                app.ui.settings.gpu = false;
                app.notices.push(format!(
                    "O LightCraft fechou ao iniciar a GPU da última vez ({what}); as fotos agora são processadas na CPU. \
                     Para tentar a GPU de novo, ative Configurações ▸ Desempenho ▸ Usar a GPU."
                ));
            }
            app.library_problem = problem;
            let android_written = android.clone();
            Ok(Box::new(Host {
                app,
                shared,
                browser: Browser::new(home, Some(private)),
                // nothing written yet: the first check saves the settings
                prefs: PrefsWriter { path: Some(prefs_path), ..Default::default() },
                android,
                android_path,
                android_written,
                tools_open: false,
                sticky_ctrl: false,
                sticky_shift: false,
                modifiers_reset: false,
                replay: None,
                zoom_applied: false,
                focused: true,
            }))
        }),
    )
}
