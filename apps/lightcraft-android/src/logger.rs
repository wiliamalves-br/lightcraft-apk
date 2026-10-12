//! A small log for the Android app: `<settings>/logs/lightcraft.log` (the previous run is kept as
//! `lightcraft.1.log`). Android drops standard error, so this file is where a problem report's
//! details come from (Help ▸ System Info names the settings folder).

use std::io::Write;
use std::sync::Mutex;

struct FileLog {
    file: Mutex<Option<std::fs::File>>,
}

impl log::Log for FileLog {
    fn enabled(&self, meta: &log::Metadata) -> bool {
        meta.level() <= log::Level::Warn || (meta.level() <= log::Level::Info && meta.target().starts_with("lightcraft"))
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let mut file = self.file.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(f) = file.as_mut() {
            let _ = writeln!(f, "{secs} {:<5} {}: {}", record.level(), record.target(), record.args());
        }
    }

    fn flush(&self) {
        let mut file = self.file.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(f) = file.as_mut() {
            let _ = f.flush();
        }
    }
}

/// Start logging into `dir`; returns the log file's path when it could be opened.
pub fn install(dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let _ = std::fs::create_dir_all(dir);
    let path = dir.join("lightcraft.log");
    let _ = std::fs::rename(&path, dir.join("lightcraft.1.log"));
    let file = std::fs::File::create(&path).ok();
    let opened = file.is_some();
    let logger: &'static FileLog = Box::leak(Box::new(FileLog { file: Mutex::new(file) }));
    if log::set_logger(logger).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    opened.then_some(path)
}
