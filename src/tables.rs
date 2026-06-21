//! Discovering `.vpx` tables on the filesystem.
//!
//! Tables live in a folder (default `~/vpinball/tables`, override with
//! `VPINBALL_TABLES`) using the standard Visual Pinball layout, where each table
//! sits in its own sub-folder alongside its media. We register that folder as a
//! Bevy asset source ([`TABLES_SOURCE`]) so tables load straight from disk, scan
//! it recursively for `.vpx` files, and read each table's name from its metadata
//! in a background task to build a [`TableIndex`] for the picker.

use crate::screens::Screen;
use bevy::asset::io::{
    AssetReader, AssetReaderError, AssetSourceBuilder, ErasedAssetReader, PathStream, Reader,
    VecReader,
};
use bevy::prelude::*;
use bevy::settings::{ReflectSettingsGroup, SettingsGroup};
use bevy::tasks::{IoTaskPool, Task, block_on, poll_once};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Name of the Bevy asset source that reads `.vpx` tables from [`TablesDir`].
pub(crate) const TABLES_SOURCE: &str = "tables";

/// The directory `.vpx` tables are scanned and loaded from.
#[derive(Resource, Clone)]
pub(crate) struct TablesDir(pub(crate) PathBuf);

/// Shared, runtime-mutable root for the `tables` asset source. Swapping the inner
/// path re-points table loading without re-registering the source (Bevy asset
/// sources are fixed once registered), so the tables folder can change at runtime.
#[derive(Resource, Clone)]
pub(crate) struct TablesRoot(pub(crate) Arc<RwLock<PathBuf>>);

/// Persisted tables-folder setting (bevy 0.19 App Settings). `dir = None` means
/// "use the default / `VPINBALL_TABLES` / CLI". Written to `settings.toml` under
/// the app config dir; the folder picker updates it. See [`crate::tables`].
#[derive(Resource, Reflect, Default, SettingsGroup)]
#[reflect(Resource, SettingsGroup)]
#[settings_group(group = "tables")]
pub(crate) struct TablesSettings {
    /// Absolute path to the tables folder, or `None` for the default.
    pub(crate) dir: Option<String>,
}

/// Build the `tables` asset-source reader factory over a shared, mutable `root`.
/// Registered before `AssetPlugin`; see [`TablesRoot`].
pub(crate) fn tables_source_builder(root: Arc<RwLock<PathBuf>>) -> AssetSourceBuilder {
    AssetSourceBuilder::new(move || {
        Box::new(MutableRootReader { root: root.clone() }) as Box<dyn ErasedAssetReader>
    })
}

/// A read-only [`AssetReader`] that resolves every path against a runtime-mutable
/// root, returning the file's bytes. The picker lists tables with `std::fs` (see
/// [`scan_vpx`]); this reader is only used to load a selected table's `.vpx`.
struct MutableRootReader {
    root: Arc<RwLock<PathBuf>>,
}

impl MutableRootReader {
    fn full_path(&self, path: &Path) -> PathBuf {
        self.root.read().unwrap().join(path)
    }
}

impl AssetReader for MutableRootReader {
    async fn read<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        let full = self.full_path(path);
        match std::fs::read(&full) {
            Ok(bytes) => Ok(VecReader::new(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(AssetReaderError::NotFound(full))
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn read_meta<'a>(&'a self, path: &'a Path) -> Result<impl Reader + 'a, AssetReaderError> {
        // vpx tables ship no `.meta` sidecars; report missing so the loader is
        // chosen by file extension (matching the default file source's behaviour).
        let mut full = self.full_path(path).into_os_string();
        full.push(".meta");
        let full = PathBuf::from(full);
        match std::fs::read(&full) {
            Ok(bytes) => Ok(VecReader::new(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(AssetReaderError::NotFound(full))
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn read_directory<'a>(
        &'a self,
        path: &'a Path,
    ) -> Result<Box<PathStream>, AssetReaderError> {
        Err(AssetReaderError::NotFound(self.full_path(path)))
    }

    async fn is_directory<'a>(&'a self, path: &'a Path) -> Result<bool, AssetReaderError> {
        Ok(self.full_path(path).is_dir())
    }
}

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<TableIndex>();
    app.add_systems(OnEnter(Screen::TableSelect), start_indexing);
    app.add_systems(
        Update,
        (
            poll_scanning.run_if(resource_exists::<ScanTask>),
            poll_indexing.run_if(resource_exists::<IndexTask>),
        ),
    );
    #[cfg(not(target_arch = "wasm32"))]
    app.add_systems(
        Update,
        folder_picker::poll_folder_dialog
            .run_if(resource_exists::<folder_picker::FolderDialogTask>),
    );
}

/// A single table found in the tables folder.
#[derive(Clone)]
pub(crate) struct TableEntry {
    /// Path of the `.vpx` file relative to the tables folder, used as the asset
    /// path under [`TABLES_SOURCE`].
    pub(crate) rel_path: String,
    /// Display name: the table's metadata name once indexed, otherwise the
    /// containing folder name as a fallback.
    pub(crate) title: String,
    /// Whether this table ships a script sidecar (`.lua` and/or `.table.json`).
    pub(crate) has_script: bool,
}

impl TableEntry {
    /// Build an entry from its relative path and an optional metadata title.
    fn build(rel_path: &str, title: Option<String>, tables_dir: &Path) -> Self {
        // Scripted: a `.lua` and/or `.table.json` sidecar next to the vpx.
        let has_script = crate::scripting::has_script_sidecar(tables_dir, rel_path);
        let title = title.unwrap_or_else(|| fallback_title(rel_path));
        Self {
            rel_path: rel_path.to_string(),
            title,
            has_script,
        }
    }
}

/// The tables found in the tables folder, with their display names.
#[derive(Resource, Default)]
pub(crate) struct TableIndex {
    pub(crate) entries: Vec<TableEntry>,
    /// True once the directory scan has finished, so `entries` is the full list
    /// (with fallback titles). Until then the picker shows a "scanning" state.
    pub(crate) scanned: bool,
    /// True once the background metadata read has finished and titles are final.
    pub(crate) indexed: bool,
}

/// The in-flight background directory scan (the slow part on a network share).
#[derive(Resource)]
struct ScanTask(Task<Vec<TableEntry>>);

/// The in-flight background metadata read.
#[derive(Resource)]
struct IndexTask(Task<Vec<TableEntry>>);

/// On entering the picker, kick off the directory scan in the background so the
/// UI renders immediately (the scan can be slow on a network share). The scan
/// builds entries with fallback titles; metadata names are read in a second
/// background pass (see [`poll_scanning`] / [`poll_indexing`]).
fn start_indexing(
    mut commands: Commands,
    tables_dir: Res<TablesDir>,
    index: Res<TableIndex>,
    scanning: Option<Res<ScanTask>>,
    reading: Option<Res<IndexTask>>,
) {
    // Already done, or already in flight from a previous entry; keep the result.
    if index.scanned || scanning.is_some() || reading.is_some() {
        return;
    }

    spawn_scan(&mut commands, tables_dir.0.clone());
}

/// Spawn the background directory scan for `root`, building entries with fallback
/// titles. Used on first display and when the folder changes.
fn spawn_scan(commands: &mut Commands, root: PathBuf) {
    let task = IoTaskPool::get().spawn(async move {
        scan_vpx(&root)
            .into_iter()
            .map(|rel| TableEntry::build(&rel, None, &root))
            .collect::<Vec<_>>()
    });
    commands.insert_resource(ScanTask(task));
}

/// When the scan finishes, show the entries (fallback titles) immediately and
/// start the background metadata read for their real names.
fn poll_scanning(
    mut commands: Commands,
    mut task: ResMut<ScanTask>,
    mut index: ResMut<TableIndex>,
    tables_dir: Res<TablesDir>,
) {
    if let Some(entries) = block_on(poll_once(&mut task.0)) {
        let root = tables_dir.0.clone();
        let rel_paths: Vec<String> = entries.iter().map(|e| e.rel_path.clone()).collect();

        index.entries = entries;
        index.scanned = true;
        commands.remove_resource::<ScanTask>();

        let task = IoTaskPool::get().spawn(async move {
            // Read each table's metadata concurrently: opening 1000+ vpx files one
            // by one is slow (especially on a network share). Spawn all the reads
            // up front so they run in parallel on the pool, then collect them back
            // in order.
            let pool = IoTaskPool::get();
            let reads: Vec<_> = rel_paths
                .into_iter()
                .map(|rel| {
                    let root = root.clone();
                    pool.spawn(async move {
                        let title = read_title(&root.join(&rel));
                        TableEntry::build(&rel, title, &root)
                    })
                })
                .collect();
            let mut entries = Vec::with_capacity(reads.len());
            for read in reads {
                entries.push(read.await);
            }
            entries
        });
        commands.insert_resource(IndexTask(task));
    }
}

/// Swap in the indexed entries (with metadata titles) once ready.
fn poll_indexing(
    mut commands: Commands,
    mut task: ResMut<IndexTask>,
    mut index: ResMut<TableIndex>,
) {
    if let Some(entries) = block_on(poll_once(&mut task.0)) {
        index.entries = entries;
        index.indexed = true;
        commands.remove_resource::<IndexTask>();
    }
}

/// Native folder picker for choosing the tables directory. Web has no equivalent,
/// so the whole module is compiled out there (the persisted setting still loads).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod folder_picker {
    use super::*;

    /// In-flight native folder-picker dialog.
    #[derive(Resource)]
    pub(crate) struct FolderDialogTask(Task<Option<rfd::FileHandle>>);

    /// Open the OS folder picker, starting at `current`. Inserts [`FolderDialogTask`]
    /// which [`poll_folder_dialog`] consumes. Callers should skip if one is open.
    pub(crate) fn open_folder_dialog(commands: &mut Commands, current: &Path) {
        let start = current.to_path_buf();
        let task = IoTaskPool::get().spawn(async move {
            rfd::AsyncFileDialog::new()
                .set_title("Select the tables folder")
                .set_directory(start)
                .pick_folder()
                .await
        });
        commands.insert_resource(FolderDialogTask(task));
    }

    /// When the picker returns, switch to the chosen folder: re-point the asset
    /// source root, persist the choice, and re-scan. Cancelled picks are ignored.
    pub(crate) fn poll_folder_dialog(
        mut commands: Commands,
        mut task: ResMut<FolderDialogTask>,
        root: Res<TablesRoot>,
        mut tables_dir: ResMut<TablesDir>,
        mut settings: ResMut<TablesSettings>,
        mut index: ResMut<TableIndex>,
    ) {
        let Some(result) = block_on(poll_once(&mut task.0)) else {
            return;
        };
        commands.remove_resource::<FolderDialogTask>();
        let Some(handle) = result else {
            return; // user cancelled
        };

        let dir = handle.path().to_path_buf();
        if let Ok(mut root) = root.0.write() {
            *root = dir.clone();
        }
        tables_dir.0 = dir.clone();
        settings.dir = Some(dir.to_string_lossy().into_owned());
        commands.queue(bevy::settings::SaveSettingsDeferred::default());

        // Reset and re-scan from the new folder (re-uses the background scan, so
        // even a slow network share never blocks the UI).
        commands.remove_resource::<ScanTask>();
        commands.remove_resource::<IndexTask>();
        *index = TableIndex::default();
        spawn_scan(&mut commands, dir);
    }
}

/// Read a table's display name from its `.vpx` metadata. Returns `None` if the
/// file can't be opened or has no (non-empty) table name.
fn read_title(abs_path: &Path) -> Option<String> {
    let mut vpx = vpin::vpx::open(abs_path).ok()?;
    let info = vpx.read_tableinfo().ok()?;
    info.table_name
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
}

/// Display name to use before (or instead of) metadata: the containing folder
/// name, which in the standard layout is the clean table title, else the file
/// stem.
fn fallback_title(rel_path: &str) -> String {
    let path = Path::new(rel_path);
    match path.parent().and_then(|p| p.file_name()) {
        Some(folder) if !folder.is_empty() => folder.to_string_lossy().into_owned(),
        _ => path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| rel_path.to_string()),
    }
}

/// Recursively collect `.vpx` files under `root`, returned as forward-slashed
/// paths relative to `root` and sorted. Hidden folders are skipped.
fn scan_vpx(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(read_dir) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                let hidden = path
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with('.'));
                if !hidden {
                    stack.push(path);
                }
            } else if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("vpx"))
                && let Ok(rel) = path.strip_prefix(root)
            {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort_unstable();
    out
}

/// Resolve the tables directory and an optional table requested on the command line.
///
/// The directory defaults to `~/vpinball/tables` and can be overridden with the
/// `VPINBALL_TABLES` environment variable. A command-line argument selects a table
/// directly: a relative path (a bare file name or a `sub-folder/table.vpx`) is
/// resolved against the tables directory, while an absolute path can point at a
/// `.vpx` file anywhere on disk. The selected file's parent folder becomes the
/// tables directory (the asset-source root) for that run.
// TODO (next PR): make the tables directory a persisted, runtime-changeable
// setting. App Settings alone can't drive it: the `tables` asset source is
// registered before `AssetPlugin` (see main.rs), so the dir is consumed before
// any settings resource loads. Plan: register the `tables` source with a custom
// `AssetReader` whose root is a runtime-mutable value, persist the dir via the
// bevy 0.19 App Settings framework (`#[derive(SettingsGroup)]`) or a config
// file, and re-scan the index when it changes - no restart. The background
// scan/metadata above is the enabler so a re-scan never blocks the UI. App
// Settings also fits display/window prefs and persisting `PickerMemory`.
pub(crate) fn resolve_tables() -> (PathBuf, Option<String>) {
    let default_dir = match std::env::var_os("VPINBALL_TABLES") {
        Some(dir) => PathBuf::from(dir),
        None => std::env::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("vpinball")
            .join("tables"),
    };

    let cli_arg = std::env::args().nth(1).filter(|arg| !arg.starts_with('-'));
    let (dir, table) = match cli_arg {
        Some(arg) => {
            let arg = Path::new(&arg);
            let full = if arg.is_absolute() {
                arg.to_path_buf()
            } else {
                default_dir.join(arg)
            };
            let dir = full.parent().map(Path::to_path_buf).unwrap_or(default_dir);
            let table = full.file_name().map(|f| f.to_string_lossy().into_owned());
            (dir, table)
        }
        None => (default_dir, None),
    };

    // Make the source root absolute so the file reader does not resolve it
    // relative to the Bevy asset root.
    let dir = std::path::absolute(&dir).unwrap_or(dir);
    (dir, table)
}
