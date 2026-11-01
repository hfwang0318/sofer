//! OCR history persisted to `<app-data>/history.json`.
//!
//! The WebView used to keep history in `localStorage`, which is lost with the
//! cache and capped by the browser quota. Inside Tauri the store now lives next
//! to the settings and models, is written atomically, and survives a corrupted
//! file instead of wiping it. The browser preview keeps using `localStorage`
//! (see `src/lib/history.ts`).
//!
//! Rules owned by this module:
//! - at most [`HISTORY_LIMIT`] entries; pinned entries are evicted last and the
//!   newest-first order of the caller is preserved;
//! - a thumbnail larger than [`THUMBNAIL_MAX_BYTES`] is dropped so the entry
//!   itself survives;
//! - a broken file is copied aside (never deleted) and the store loads empty;
//! - the legacy `localStorage` copy is imported exactly once.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// Hard ceiling for the persisted store. The UI may keep fewer through
/// `settings.historyLimit`; that knob is applied by the frontend.
pub const HISTORY_LIMIT: usize = 50;
/// A single thumbnail above this size is dropped instead of the whole entry.
pub const THUMBNAIL_MAX_BYTES: usize = 200 * 1024;
const STORE_VERSION: u32 = 1;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Mirrors `HistoryEntry` in `src/lib/types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub text: String,
    pub engine: String,
    pub ms: u64,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub language: Option<String>,
    pub format: String,
    pub chars: usize,
    pub lines: usize,
    #[serde(default)]
    pub thumbnail: Option<String>,
    pub captured_at: i64,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoreFile {
    #[serde(default = "store_version")]
    version: u32,
    /// Set once the legacy `localStorage` history has been imported (or found
    /// empty) so a later launch never re-imports it.
    #[serde(default)]
    migrated: bool,
    #[serde(default)]
    entries: Vec<HistoryEntry>,
}

fn store_version() -> u32 {
    STORE_VERSION
}

impl Default for StoreFile {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            migrated: false,
            entries: Vec::new(),
        }
    }
}

pub fn store_path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("history.json")
}

/// Enforce the size rules regardless of who wrote the file.
fn normalize(entries: Vec<HistoryEntry>) -> Vec<HistoryEntry> {
    let entries: Vec<HistoryEntry> = entries
        .into_iter()
        .map(|mut entry| {
            if entry
                .thumbnail
                .as_ref()
                .is_some_and(|thumb| thumb.len() > THUMBNAIL_MAX_BYTES)
            {
                // Keep the text; a preview image is not worth losing the entry.
                entry.thumbnail = None;
            }
            entry
        })
        .collect();
    trim(entries)
}

/// Keep at most [`HISTORY_LIMIT`] entries. Pinned entries claim a slot first;
/// the remaining slots go to the caller's order (newest first).
fn trim(entries: Vec<HistoryEntry>) -> Vec<HistoryEntry> {
    if entries.len() <= HISTORY_LIMIT {
        return entries;
    }
    let mut keep = vec![false; entries.len()];
    let mut kept = 0;
    for (index, entry) in entries.iter().enumerate() {
        if entry.pinned && kept < HISTORY_LIMIT {
            keep[index] = true;
            kept += 1;
        }
    }
    for (index, entry) in entries.iter().enumerate() {
        if kept >= HISTORY_LIMIT {
            break;
        }
        if !entry.pinned {
            keep[index] = true;
            kept += 1;
        }
    }
    entries
        .into_iter()
        .zip(keep)
        .filter_map(|(entry, keep)| keep.then_some(entry))
        .collect()
}

fn read_store(path: &Path) -> StoreFile {
    let Ok(raw) = fs::read_to_string(path) else {
        // A missing file is a fresh install, not an error.
        return StoreFile::default();
    };
    match serde_json::from_str::<StoreFile>(&raw) {
        Ok(mut store) => {
            store.entries = normalize(store.entries);
            store
        }
        Err(err) => {
            let kept = quarantine(path);
            match &kept {
                Some(bak) => eprintln!(
                    "history: {} is invalid ({err}); kept as {} and starting empty",
                    path.display(),
                    bak.display()
                ),
                None => eprintln!(
                    "history: {} is invalid ({err}); could not keep a copy, starting empty",
                    path.display()
                ),
            }
            StoreFile::default()
        }
    }
}

/// Move a broken file aside so nobody silently deletes the user's data.
fn quarantine(path: &Path) -> Option<PathBuf> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let backup = path.with_extension(format!("json.corrupt-{}-{stamp}", std::process::id()));
    if fs::rename(path, &backup).is_ok() {
        return Some(backup);
    }
    // Rename can fail across odd mounts; a copy still preserves the evidence.
    fs::copy(path, &backup).ok().map(|_| backup)
}

fn temp_path(path: &Path) -> PathBuf {
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "history.json".into());
    path.with_file_name(format!("{name}.tmp-{}-{unique}", std::process::id()))
}

/// Write through a sibling temp file and rename, so a crash mid-write leaves the
/// previous history intact instead of a half-written JSON file.
fn write_store(path: &Path, store: &StoreFile) -> Result<(), String> {
    let Some(dir) = path.parent() else {
        return Err(format!("历史目录无效：{}", path.display()));
    };
    fs::create_dir_all(dir).map_err(|e| format!("创建历史目录失败：{e}"))?;
    let body = serde_json::to_string_pretty(store).map_err(|e| format!("序列化历史失败：{e}"))?;
    let temp = temp_path(path);
    let result = (|| -> std::io::Result<()> {
        {
            let mut file = fs::File::create(&temp)?;
            file.write_all(body.as_bytes())?;
            file.sync_all()?;
        }
        fs::rename(&temp, path)
    })();
    if let Err(err) = result {
        let _ = fs::remove_file(&temp);
        return Err(format!("写入历史失败：{err}"));
    }
    Ok(())
}

pub fn load_entries(path: &Path) -> Vec<HistoryEntry> {
    read_store(path).entries
}

pub fn save_entries(path: &Path, entries: Vec<HistoryEntry>) -> Result<(), String> {
    let existing = read_store(path);
    let store = StoreFile {
        version: STORE_VERSION,
        // A save must not reopen the legacy import if it already ran.
        migrated: existing.migrated,
        entries: normalize(entries),
    };
    write_store(path, &store)
}

/// Import the pre-disk `localStorage` history once. The store's `migrated`
/// flag (persisted in the file) makes later calls no-ops, including after the
/// user clears their history.
pub fn migrate_entries(
    path: &Path,
    legacy: Vec<HistoryEntry>,
) -> Result<Vec<HistoryEntry>, String> {
    let existing = read_store(path);
    if existing.migrated || !existing.entries.is_empty() {
        return Ok(existing.entries);
    }
    let entries = normalize(legacy);
    let store = StoreFile {
        version: STORE_VERSION,
        migrated: true,
        entries: entries.clone(),
    };
    write_store(path, &store)?;
    Ok(entries)
}

#[tauri::command]
pub fn load_history(app: AppHandle) -> Vec<HistoryEntry> {
    load_entries(&store_path(&app))
}

#[tauri::command]
pub fn save_history(app: AppHandle, entries: Vec<HistoryEntry>) -> Result<(), String> {
    save_entries(&store_path(&app), entries)
}

#[tauri::command]
pub fn migrate_history(
    app: AppHandle,
    legacy: Vec<HistoryEntry>,
) -> Result<Vec<HistoryEntry>, String> {
    migrate_entries(&store_path(&app), legacy)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let unique = format!(
            "sofer-history-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(unique);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(id: &str, captured_at: i64) -> HistoryEntry {
        HistoryEntry {
            id: id.into(),
            text: format!("fn {id}() {{}}"),
            engine: "mock".into(),
            ms: 12,
            width: 800,
            height: 600,
            language: None,
            format: "text".into(),
            chars: 12,
            lines: 1,
            thumbnail: None,
            captured_at,
            pinned: false,
        }
    }

    fn ids(entries: &[HistoryEntry]) -> Vec<&str> {
        entries.iter().map(|entry| entry.id.as_str()).collect()
    }

    #[test]
    fn save_writes_camel_case_and_reads_back() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("history.json");
        let mut first = entry("a", 1_700_000_000_000);
        first.language = Some("rust".into());
        save_entries(&path, vec![first.clone()]).unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        assert!(raw.contains("\"capturedAt\""));
        assert!(raw.contains("\"pinned\""));
        assert_eq!(load_entries(&path), vec![first]);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn save_replaces_atomically_and_leaves_no_temp_files() {
        let dir = temp_dir("atomic");
        let path = dir.join("history.json");
        save_entries(&path, vec![entry("first", 1)]).unwrap();
        save_entries(&path, vec![entry("second", 2)]).unwrap();

        assert_eq!(ids(&load_entries(&path)), vec!["second"]);
        let leftovers: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|item| item.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp files should be renamed away: {leftovers:?}"
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn corrupted_store_is_kept_aside_and_loads_empty() {
        let dir = temp_dir("corrupt");
        let path = dir.join("history.json");
        fs::write(&path, "{ not json").unwrap();

        assert!(load_entries(&path).is_empty());

        let files: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .map(|item| item.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!path.exists(), "the broken file is moved, not overwritten");
        let backup = files
            .iter()
            .find(|name| name.contains(".corrupt-"))
            .expect("a copy of the broken file must remain");
        assert_eq!(fs::read_to_string(dir.join(backup)).unwrap(), "{ not json");
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn missing_file_is_an_empty_store() {
        let dir = temp_dir("missing");
        assert!(load_entries(&dir.join("history.json")).is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn entries_are_capped_and_pinned_ones_survive_eviction() {
        let dir = temp_dir("cap");
        let path = dir.join("history.json");
        let mut entries: Vec<HistoryEntry> = (0..HISTORY_LIMIT + 10)
            .map(|index| entry(&format!("e{index}"), (HISTORY_LIMIT + 10 - index) as i64))
            .collect();
        // The oldest entry is pinned and must keep its slot.
        let oldest = entries.last_mut().unwrap();
        oldest.pinned = true;
        oldest.id = "pinned".into();

        save_entries(&path, entries).unwrap();
        let loaded = load_entries(&path);
        assert_eq!(loaded.len(), HISTORY_LIMIT);
        assert_eq!(loaded.first().map(|e| e.id.as_str()), Some("e0"));
        assert!(
            loaded.iter().any(|entry| entry.id == "pinned"),
            "pinned entries are evicted last"
        );
        assert!(
            !loaded
                .iter()
                .any(|entry| entry.id == format!("e{}", HISTORY_LIMIT + 9)),
            "the older unpinned tail is evicted"
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn oversized_thumbnail_is_dropped_but_the_entry_stays() {
        let dir = temp_dir("thumb");
        let path = dir.join("history.json");
        let mut big = entry("big", 2);
        big.thumbnail = Some("x".repeat(THUMBNAIL_MAX_BYTES + 1));
        let mut edge = entry("edge", 1);
        edge.thumbnail = Some("y".repeat(THUMBNAIL_MAX_BYTES));

        save_entries(&path, vec![big, edge]).unwrap();
        let loaded = load_entries(&path);
        assert_eq!(ids(&loaded), vec!["big", "edge"]);
        assert!(
            loaded[0].thumbnail.is_none(),
            "oversized preview is dropped"
        );
        assert_eq!(
            loaded[1].thumbnail.as_ref().map(String::len),
            Some(THUMBNAIL_MAX_BYTES),
            "a thumbnail at the limit is kept"
        );
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn legacy_migration_only_runs_once() {
        let dir = temp_dir("migrate");
        let path = dir.join("history.json");
        let imported = migrate_entries(&path, vec![entry("legacy", 1)]).unwrap();
        assert_eq!(ids(&imported), vec!["legacy"]);

        // A second launch with a different localStorage copy must not duplicate.
        let again = migrate_entries(&path, vec![entry("other", 2)]).unwrap();
        assert_eq!(ids(&again), vec!["legacy"]);

        // Clearing history keeps the migrated flag, so the old copy stays gone.
        save_entries(&path, Vec::new()).unwrap();
        assert!(migrate_entries(&path, vec![entry("other", 2)])
            .unwrap()
            .is_empty());
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn migration_is_skipped_when_the_store_already_has_entries() {
        let dir = temp_dir("migrate-skip");
        let path = dir.join("history.json");
        save_entries(&path, vec![entry("disk", 1)]).unwrap();
        let kept = migrate_entries(&path, vec![entry("legacy", 2)]).unwrap();
        assert_eq!(ids(&kept), vec!["disk"]);
        fs::remove_dir_all(dir).ok();
    }
}
