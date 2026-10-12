//! The file picker drawn in the window: Android gives a native activity no file dialog it can
//! wait on, so the host shows this one for every [`PickRequest`] (import, export folder, presets,
//! library…). It reads the shared storage directly (the activity asks for "All files access").
//!
//! Made for fingers: rows a thumb can hit, a selection that survives changing folders (pick photos
//! from several folders, then import them all at once), Select All, and — for photo imports —
//! "Import this whole folder", which hands the folder to the import review (searched with its
//! subfolders).

use egui::{Align2, Color32, FontId, Pos2, RichText, Sense, Stroke, Vec2, vec2};
use lightcraft_ui_egui::pick::{PickKind, PickRequest};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Row height (points) of the folder listing.
const ROW: f32 = 44.0;
/// Text size of the listing.
const TEXT: f32 = 17.0;

/// What happened to the picker this frame.
pub enum Outcome {
    /// Still open.
    Open,
    /// Chosen: the paths to answer with (never empty).
    Chosen(Vec<String>),
    /// Closed without a choice.
    Cancelled,
}

struct Entry {
    name: String,
    path: PathBuf,
    is_dir: bool,
    size: u64,
    /// A folder, or a file the request's filter admits.
    matches: bool,
}

/// The picker's state; one per app, reset by [`Browser::begin`] for each request.
pub struct Browser {
    /// The shared storage's root (`/storage/emulated/0`).
    home: PathBuf,
    /// The app's private folder (settings, models), offered as a place too.
    private: Option<PathBuf>,
    dir: PathBuf,
    /// The folder `entries` lists (relisted when `dir` changes).
    listed: Option<PathBuf>,
    entries: Vec<Entry>,
    error: Option<String>,
    /// [`PickKind::Files`]: what is ticked, in any folder.
    ticked: BTreeSet<PathBuf>,
    /// [`PickKind::File`]: the file tapped.
    single: Option<PathBuf>,
    /// [`PickKind::Save`]: the file name to save as.
    file_name: String,
    /// The name typed for New Folder.
    new_folder: String,
    /// The last folder each kind of request ended in.
    last_dir: [Option<PathBuf>; 4],
}

fn kind_index(kind: PickKind) -> usize {
    match kind {
        PickKind::Files => 0,
        PickKind::File => 1,
        PickKind::Folder => 2,
        PickKind::Save => 3,
    }
}

/// `name` has one of `exts` (no dots, any case).
fn has_extension(name: &str, exts: &[&str]) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| exts.iter().any(|x| x.eq_ignore_ascii_case(ext)))
}

fn human_size(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= 1024.0 * MB {
        format!("{:.1} GB", b / (1024.0 * MB))
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else {
        format!("{:.0} KB", (b / 1024.0).max(1.0))
    }
}

impl Browser {
    pub fn new(home: PathBuf, private: Option<PathBuf>) -> Self {
        Browser {
            dir: home.clone(),
            home,
            private,
            listed: None,
            entries: Vec::new(),
            error: None,
            ticked: BTreeSet::new(),
            single: None,
            file_name: String::new(),
            new_folder: String::new(),
            last_dir: [None, None, None, None],
        }
    }

    /// Start showing `req`: where the same kind of request ended last time, else a sensible
    /// place for it (the camera folder for photos, Pictures for folders and saves).
    pub fn begin(&mut self, req: &PickRequest) {
        self.ticked.clear();
        self.single = None;
        self.new_folder.clear();
        self.file_name = req.file_name.clone().unwrap_or_default();
        let photos = is_photo_request(req);
        let fallback = if photos {
            [self.home.join("DCIM").join("Camera"), self.home.join("DCIM"), self.home.join("Pictures")]
        } else if matches!(req.kind, PickKind::Folder | PickKind::Save) {
            [self.home.join("Pictures"), self.home.join("Download"), self.home.clone()]
        } else {
            [self.home.join("Download"), self.home.join("Documents"), self.home.clone()]
        };
        let last = self.last_dir[kind_index(req.kind)].clone().filter(|d| d.is_dir());
        self.dir = last.or_else(|| fallback.into_iter().find(|d| d.is_dir())).unwrap_or_else(|| self.home.clone());
        self.listed = None;
    }

    fn list(&mut self, filter: Option<&[&str]>) {
        self.entries.clear();
        self.error = None;
        match std::fs::read_dir(&self.dir) {
            Ok(read) => {
                for item in read.flatten() {
                    let name = item.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') {
                        continue;
                    }
                    let path = item.path();
                    // follows links: a link to a folder is a folder
                    let meta = std::fs::metadata(&path).ok();
                    let is_dir = meta.as_ref().is_some_and(std::fs::Metadata::is_dir);
                    let size = meta.as_ref().map_or(0, std::fs::Metadata::len);
                    let matches = is_dir || filter.is_none_or(|exts| has_extension(&name, exts));
                    self.entries.push(Entry { name, path, is_dir, size, matches });
                }
                self.entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
            }
            Err(e) => {
                self.error = Some(format!(
                    "Não foi possível abrir esta pasta ({e}). Se o LightCraft ainda não tem permissão, ative \"Acesso a todos os arquivos\" \
                     para ele em Configurações ▸ Apps ▸ LightCraft ▸ Permissões e volte para o app."
                ));
            }
        }
        self.listed = Some(self.dir.clone());
    }

    /// The places offered above the listing: the shared storage and its usual folders, memory
    /// cards and USB drives (`/storage/<id>`), and the app's private folder.
    fn places(&self) -> Vec<(String, PathBuf)> {
        let mut out = vec![("Armazenamento".to_string(), self.home.clone())];
        for (label, sub) in
            [("Câmera", "DCIM/Camera"), ("DCIM", "DCIM"), ("Imagens", "Pictures"), ("Downloads", "Download"), ("Documentos", "Documents")]
        {
            let p = self.home.join(sub);
            if p.is_dir() {
                out.push((label.to_string(), p));
            }
        }
        if let Ok(read) = std::fs::read_dir("/storage") {
            for item in read.flatten() {
                let name = item.file_name().to_string_lossy().into_owned();
                if name != "emulated" && name != "self" && item.path().is_dir() {
                    out.push((format!("Cartão {name}"), item.path()));
                }
            }
        }
        if let Some(p) = &self.private {
            out.push(("Pasta do app".to_string(), p.clone()));
        }
        out
    }

    /// Draw the picker for `req`; call every frame while it is open.
    pub fn show(&mut self, ctx: &egui::Context, req: &PickRequest) -> Outcome {
        let filter = req.filter.as_ref().map(|(_, exts)| *exts);
        if self.listed.as_deref() != Some(self.dir.as_path()) {
            self.list(filter);
        }
        let screen = ctx.content_rect();
        let size = vec2((screen.width() - 24.0).clamp(240.0, 1200.0), (screen.height() - 24.0).max(240.0));
        let mut outcome = Outcome::Open;
        let mut go: Option<PathBuf> = None;
        let modal = egui::Modal::new(egui::Id::new("lightcraft-android-picker")).show(ctx, |ui| {
            ui.set_width(size.x);
            ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
            ui.spacing_mut().interact_size.y = 36.0;
            let title = req.title.clone().unwrap_or_else(|| default_title(req.kind).to_string());
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).size(20.0).strong());
                if let Some((name, _)) = &req.filter {
                    ui.label(RichText::new(format!("({name})")).size(15.0).weak());
                }
            });
            // where we are, and up
            ui.horizontal(|ui| {
                let up = self.dir.parent().map(Path::to_path_buf).filter(|p| p.as_os_str().len() > 1);
                if ui.add_enabled(up.is_some(), egui::Button::new(RichText::new("↑ Acima").size(TEXT)).min_size(vec2(96.0, 38.0))).clicked() {
                    go = up;
                }
                ui.label(RichText::new(self.dir.to_string_lossy()).size(15.0).monospace());
            });
            // places
            egui::ScrollArea::horizontal().id_salt("lc-picker-places").show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (label, path) in self.places() {
                        let on = self.dir == path;
                        if ui.add(egui::Button::selectable(on, RichText::new(label).size(15.0)).min_size(vec2(0.0, 34.0))).clicked() {
                            go = Some(path);
                        }
                    }
                });
            });
            ui.separator();
            // reserve room for the bar at the bottom (a second row for Save and Folder)
            let footer = if matches!(req.kind, PickKind::Save | PickKind::Folder) { 2.0 * 50.0 } else { 54.0 };
            let used = ui.min_rect().height();
            let list_h = (size.y - used - footer - 16.0).max(ROW * 3.0);
            if let Some(err) = &self.error {
                ui.add_sized([size.x, list_h], egui::Label::new(RichText::new(err).size(TEXT).color(ui.visuals().warn_fg_color)).wrap());
            } else if self.entries.is_empty() {
                ui.add_sized([size.x, list_h], egui::Label::new(RichText::new("Pasta vazia").size(TEXT).weak()));
            } else {
                let width = size.x;
                egui::ScrollArea::vertical().id_salt(("lc-picker-list", self.dir.clone())).auto_shrink([false, false]).max_height(list_h).show_rows(
                    ui,
                    ROW,
                    self.entries.len(),
                    |ui, range| {
                        for i in range {
                            if let Some(clicked) = self.row(ui, i, width, req.kind) {
                                go = go.take().or(clicked);
                            }
                        }
                    },
                );
            }
            ui.separator();
            if let Some(done) = self.footer(ui, req) {
                outcome = done;
            }
        });
        if let Some(dir) = go {
            self.dir = dir;
        }
        if matches!(outcome, Outcome::Open) && modal.should_close() {
            outcome = Outcome::Cancelled;
        }
        if !matches!(outcome, Outcome::Open) {
            self.last_dir[kind_index(req.kind)] = Some(self.dir.clone());
        }
        outcome
    }

    /// One row of the listing. Returns `Some(folder)` when a folder was tapped (to open it).
    fn row(&mut self, ui: &mut egui::Ui, i: usize, width: f32, kind: PickKind) -> Option<Option<PathBuf>> {
        let entry = self.entries.get(i)?;
        let (rect, resp) = ui.allocate_exact_size(vec2(width, ROW), Sense::click());
        let visuals = ui.visuals().clone();
        let ticked = match kind {
            PickKind::Files => self.ticked.contains(&entry.path),
            PickKind::File => self.single.as_ref() == Some(&entry.path),
            PickKind::Save => !entry.is_dir && entry.name == self.file_name,
            PickKind::Folder => false,
        };
        let painter = ui.painter_at(rect);
        if ticked {
            painter.rect_filled(rect, 4.0, visuals.selection.bg_fill);
        } else if resp.hovered() || resp.is_pointer_button_down_on() {
            painter.rect_filled(rect, 4.0, visuals.widgets.hovered.weak_bg_fill);
        }
        let text_color = if entry.matches { visuals.text_color() } else { visuals.weak_text_color().gamma_multiply(0.6) };
        let left = rect.left_center() + vec2(12.0, 0.0);
        if entry.is_dir {
            painter.text(left, Align2::LEFT_CENTER, format!("{}/", entry.name), FontId::proportional(TEXT), text_color);
        } else {
            let mut x = left.x;
            if kind == PickKind::Files && entry.matches {
                checkbox(&painter, Pos2::new(x + 10.0, rect.center().y), ticked, &visuals);
                x += 32.0;
            }
            painter.text(Pos2::new(x, left.y), Align2::LEFT_CENTER, &entry.name, FontId::proportional(TEXT), text_color);
            painter.text(
                rect.right_center() - vec2(12.0, 0.0),
                Align2::RIGHT_CENTER,
                human_size(entry.size),
                FontId::proportional(13.0),
                visuals.weak_text_color(),
            );
        }
        if !resp.clicked() {
            return None;
        }
        if entry.is_dir {
            return Some(Some(entry.path.clone()));
        }
        if !entry.matches {
            return None;
        }
        let path = entry.path.clone();
        let name = entry.name.clone();
        match kind {
            PickKind::Files => {
                if !self.ticked.remove(&path) {
                    self.ticked.insert(path);
                }
            }
            PickKind::File => self.single = Some(path),
            PickKind::Save => self.file_name = name,
            PickKind::Folder => {}
        }
        Some(None)
    }

    fn footer(&mut self, ui: &mut egui::Ui, req: &PickRequest) -> Option<Outcome> {
        let big = |text: &str| egui::Button::new(RichText::new(text).size(TEXT)).min_size(vec2(110.0, 42.0));
        let mut outcome = None;
        if matches!(req.kind, PickKind::Save | PickKind::Folder) {
            ui.horizontal(|ui| {
                if req.kind == PickKind::Save {
                    ui.label(RichText::new("Nome:").size(TEXT));
                    ui.add(egui::TextEdit::singleline(&mut self.file_name).font(FontId::proportional(TEXT)).desired_width(260.0));
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_folder).hint_text("nova pasta").font(FontId::proportional(TEXT)).desired_width(180.0),
                );
                let name = self.new_folder.trim().to_string();
                let valid = !name.is_empty() && !name.contains('/') && name != "." && name != "..";
                if ui.add_enabled(valid, big("Criar pasta")).clicked() {
                    let dir = self.dir.join(&name);
                    match std::fs::create_dir_all(&dir) {
                        Ok(()) => {
                            self.dir = dir;
                            self.new_folder.clear();
                        }
                        Err(e) => self.error = Some(format!("Não foi possível criar a pasta: {e}")),
                    }
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.add(big("Cancelar")).clicked() {
                outcome = Some(Outcome::Cancelled);
            }
            match req.kind {
                PickKind::Files => {
                    let files: Vec<PathBuf> = self.entries.iter().filter(|e| !e.is_dir && e.matches).map(|e| e.path.clone()).collect();
                    let all_here = !files.is_empty() && files.iter().all(|f| self.ticked.contains(f));
                    let label = if all_here { format!("Desmarcar todos ({})", files.len()) } else { format!("Selecionar todos ({})", files.len()) };
                    if ui.add_enabled(!files.is_empty(), big(&label)).clicked() {
                        for f in files {
                            if all_here {
                                self.ticked.remove(&f);
                            } else {
                                self.ticked.insert(f);
                            }
                        }
                    }
                    if is_photo_request(req) && ui.add(big("Importar esta pasta inteira")).clicked() {
                        outcome = Some(Outcome::Chosen(vec![self.dir.to_string_lossy().into_owned()]));
                    }
                    let n = self.ticked.len();
                    if ui.add_enabled(n > 0, big(&format!("Abrir ({n})"))).clicked() {
                        outcome = Some(Outcome::Chosen(self.ticked.iter().map(|p| p.to_string_lossy().into_owned()).collect()));
                    }
                }
                PickKind::File => {
                    if ui.add_enabled(self.single.is_some(), big("Abrir")).clicked()
                        && let Some(p) = &self.single
                    {
                        outcome = Some(Outcome::Chosen(vec![p.to_string_lossy().into_owned()]));
                    }
                }
                PickKind::Folder => {
                    if ui.add(big("Usar esta pasta")).clicked() {
                        outcome = Some(Outcome::Chosen(vec![self.dir.to_string_lossy().into_owned()]));
                    }
                }
                PickKind::Save => {
                    let name = self.file_name.trim().to_string();
                    let valid = !name.is_empty() && !name.contains('/');
                    if ui.add_enabled(valid, big("Salvar aqui")).clicked() {
                        let name = match req.filter.as_ref().and_then(|(_, exts)| exts.first()) {
                            Some(&ext) if !has_extension(&name, &[ext]) => format!("{name}.{ext}"),
                            _ => name,
                        };
                        outcome = Some(Outcome::Chosen(vec![self.dir.join(name).to_string_lossy().into_owned()]));
                    }
                }
            }
        });
        outcome
    }
}

/// A request for photos (Import Photos…, Locate Missing File…): offers "Import this whole folder".
fn is_photo_request(req: &PickRequest) -> bool {
    req.kind == PickKind::Files && req.filter.as_ref().is_some_and(|(_, exts)| *exts == lightcraft_engine::import::EXTENSIONS)
}

fn default_title(kind: PickKind) -> &'static str {
    match kind {
        PickKind::Files => "Escolher arquivos",
        PickKind::File => "Escolher arquivo",
        PickKind::Folder => "Escolher pasta",
        PickKind::Save => "Salvar como",
    }
}

fn checkbox(painter: &egui::Painter, center: Pos2, on: bool, visuals: &egui::Visuals) {
    let rect = egui::Rect::from_center_size(center, Vec2::splat(20.0));
    let accent = visuals.selection.stroke.color;
    painter.rect_filled(rect, 3.0, if on { accent } else { visuals.widgets.inactive.bg_fill });
    if on {
        let ink = Color32::from_gray(if accent.r() as u32 + accent.g() as u32 + accent.b() as u32 > 380 { 20 } else { 245 });
        let a = rect.left_center() + vec2(4.0, 0.0);
        let b = rect.center_bottom() + vec2(-1.0, -5.0);
        let c = rect.right_top() + vec2(-4.0, 5.0);
        painter.line_segment([a, b], Stroke::new(2.5, ink));
        painter.line_segment([b, c], Stroke::new(2.5, ink));
    }
}
