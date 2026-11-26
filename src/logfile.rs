//! Порт LogFile.cs: журналы в %APPDATA%\RigControl, хранятся только записи текущего дня.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

const DEFAULT_MAX_BYTES: u64 = 512 * 1024;
const DAY_MARKER: &str = ".logs-day";
static LAST_CLEANUP: Mutex<Option<String>> = Mutex::new(None);

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

pub fn cleanup_on_startup() {
    let dir = crate::settings::app_dir();
    let mut last = LAST_CLEANUP.lock().unwrap_or_else(|e| e.into_inner());
    cleanup_locked(&dir, &mut last);
}

pub fn append(file_name: &str, text: &str) {
    append_ex(file_name, text, DEFAULT_MAX_BYTES, false)
}

/// Ошибки журнала молча игнорируются: журнал не должен мешать работе программы.
pub fn append_ex(file_name: &str, text: &str, max_bytes: u64, reset: bool) {
    let dir = crate::settings::app_dir();
    let mut last = LAST_CLEANUP.lock().unwrap_or_else(|e| e.into_inner());
    if fs::create_dir_all(&dir).is_err() { return; }
    cleanup_locked(&dir, &mut last);
    let path = dir.join(file_name);
    let too_big = fs::metadata(&path).map(|m| m.len() > max_bytes).unwrap_or(false);
    if reset || too_big {
        let _ = fs::write(&path, text);
    } else if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = f.write_all(text.as_bytes());
    }
}

fn cleanup_locked(dir: &Path, last: &mut Option<String>) {
    let day = today();
    if last.as_deref() == Some(day.as_str()) { return; }
    if dir.is_dir() {
        let marker = dir.join(DAY_MARKER);
        let is_current = fs::read_to_string(&marker).map(|t| t.trim() == day).unwrap_or(false);
        if let Ok(entries) = fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().and_then(|x| x.to_str()) != Some("log") { continue; }
                let old = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d").to_string() < day)
                    .unwrap_or(false);
                if !is_current || old {
                    let _ = fs::remove_file(&p);
                }
            }
        }
        let _ = fs::write(&marker, &day);
    }
    *last = Some(day);
}
