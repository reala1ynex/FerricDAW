//! Install-location hygiene (I-31).
//!
//! Older updaters (v1.1.12, v1.1.13) copied every file of the release ZIP into
//! the folder that held the running `ferricdaw.exe`. When that folder was the
//! Desktop, the Desktop received the whole package. This module:
//!
//! * recognises shared folders (Desktop, Downloads, Documents, profile root)
//!   and offers to move FerricDAW into its own folder,
//!   `%LOCALAPPDATA%\Programs\FerricDAW`;
//! * finds older FerricDAW copies (Desktop, Downloads, shortcut targets, the
//!   recorded install history and the dedicated install folder) and removes
//!   only files that match a known package manifest by relative path, size
//!   and SHA-256;
//! * repairs shortcuts that pointed at removed copies and offers a desktop
//!   shortcut.
//!
//! Safety rules:
//! * A folder is only treated as an old install when its `ferricdaw.exe`
//!   matches a known package. Anything else in it (plugins, presets, projects,
//!   settings, audio setups, samples, logs, edited files) stays.
//! * Folders are never deleted recursively. Only empty `demo`/`docs` package
//!   folders and an emptied former install folder are removed with
//!   `remove_dir`, which refuses non-empty folders.
//! * The running folder and anything inside a source checkout are skipped.
//! * Cleanup runs from a new version only after it has started. Each folder is
//!   staged into a hidden holding folder and restored if any file cannot move.
//! * Settings, recovery data and plugin caches live in `%LOCALAPPDATA%\FerricDAW`,
//!   which is never a cleanup candidate.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};

pub const EXE_NAME: &str = "ferricdaw.exe";
pub const MANIFEST_NAME: &str = "ferricdaw-manifest.json";
const KNOWN_PACKAGES: &str = include_str!("known_packages.json");
const PACKAGE_SUBDIRS: [&str; 2] = ["demo", "docs"];
const HOLD_PREFIX: &str = ".ferricdaw-cleanup-";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageManifest {
    pub version: String,
    #[serde(default)]
    pub zip_sha256: Option<String>,
    pub files: Vec<ManifestEntry>,
}

#[derive(Deserialize)]
struct KnownPackages {
    packages: Vec<PackageManifest>,
}

/// Manifests of every published portable ZIP up to the previous release.
pub fn embedded_manifests() -> Vec<PackageManifest> {
    serde_json::from_str::<KnownPackages>(KNOWN_PACKAGES)
        .map(|k| k.packages)
        .unwrap_or_default()
}

/// The manifest shipped beside an executable (v1.1.14 and later).
pub fn read_manifest(dir: &Path) -> Option<PackageManifest> {
    let text = std::fs::read_to_string(dir.join(MANIFEST_NAME)).ok()?;
    let manifest: PackageManifest =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    let valid = !manifest.version.trim().is_empty()
        && manifest.files.iter().any(|f| f.path.eq_ignore_ascii_case(EXE_NAME))
        && manifest
            .files
            .iter()
            .all(|f| is_safe_relative(&f.path) && is_hex64(&f.sha256));
    valid.then_some(manifest)
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn norm(rel: &str) -> String {
    rel.replace('\\', "/").to_ascii_lowercase()
}

/// Relative, no drive, no root, no `.`/`..` components.
pub fn is_safe_relative(rel: &str) -> bool {
    let p = rel.replace('\\', "/");
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains(':')
        && p.split('/').all(|c| !c.is_empty() && c != "." && c != "..")
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Lookup of every known package file: normalised path → (size, sha256, version).
#[derive(Default, Clone)]
pub struct ManifestIndex {
    entries: BTreeMap<String, (String, Vec<(u64, String, String)>)>,
    zip_hashes: BTreeSet<String>,
}

impl ManifestIndex {
    pub fn new(manifests: &[PackageManifest]) -> Self {
        let mut index = Self::default();
        for m in manifests {
            index.add(m);
        }
        index
    }

    pub fn add(&mut self, manifest: &PackageManifest) {
        if let Some(zip) = &manifest.zip_sha256 {
            if is_hex64(zip) {
                self.zip_hashes.insert(zip.to_ascii_lowercase());
            }
        }
        for f in &manifest.files {
            if !is_safe_relative(&f.path) || !is_hex64(&f.sha256) {
                continue;
            }
            let slot = self
                .entries
                .entry(norm(&f.path))
                .or_insert_with(|| (f.path.replace('\\', "/"), Vec::new()));
            let item = (f.size, f.sha256.to_ascii_lowercase(), manifest.version.clone());
            if !slot.1.contains(&item) {
                slot.1.push(item);
            }
        }
    }

    /// Version of the known package whose entry matches `dir/rel` exactly.
    /// Links and folders never match.
    pub fn match_file(&self, dir: &Path, rel: &str) -> Option<String> {
        let (original, list) = self.entries.get(&norm(rel))?;
        let path = dir.join(original);
        let meta = std::fs::symlink_metadata(&path).ok()?;
        if !meta.file_type().is_file() {
            return None;
        }
        let size = meta.len();
        if !list.iter().any(|(s, _, _)| *s == size) {
            return None;
        }
        let hash = sha256_file(&path).ok()?;
        list.iter()
            .find(|(s, h, _)| *s == size && *h == hash)
            .map(|(_, _, v)| v.clone())
    }

    pub fn is_known_zip(&self, path: &Path) -> bool {
        !self.zip_hashes.is_empty()
            && sha256_file(path)
                .map(|h| self.zip_hashes.contains(&h))
                .unwrap_or(false)
    }

    fn originals(&self) -> impl Iterator<Item = &String> {
        self.entries.values().map(|(original, _)| original)
    }
}

/// Files in one folder that will be removed, all positively identified.
#[derive(Debug, Clone, PartialEq)]
pub struct DirPlan {
    pub dir: PathBuf,
    /// Version of the recognised executable (reported in tests and logs).
    #[cfg_attr(not(test), allow(dead_code))]
    pub exe_version: String,
    pub files: Vec<PathBuf>,
}

/// Plan removal of an old/stray copy in `dir`. `None` unless the folder's
/// `ferricdaw.exe` is a known package executable no newer than `current_version`.
pub fn plan_dir(dir: &Path, index: &ManifestIndex, current_version: &str) -> Option<DirPlan> {
    let exe_version = index.match_file(dir, EXE_NAME)?;
    if crate::update::version_is_newer(&exe_version, current_version) {
        return None;
    }
    let mut files: Vec<PathBuf> = index
        .originals()
        .filter(|rel| index.match_file(dir, rel).is_some())
        .map(|rel| dir.join(rel))
        .collect();
    // A package manifest is identified by its structure and an older-or-equal version.
    if let Some(m) = read_manifest(dir) {
        if !crate::update::version_is_newer(&m.version, current_version) {
            files.push(dir.join(MANIFEST_NAME));
        }
    }
    Some(DirPlan {
        dir: dir.to_path_buf(),
        exe_version,
        files,
    })
}

#[derive(Debug, Default, Clone)]
pub struct CleanupOutcome {
    pub removed: Vec<PathBuf>,
    pub dirs_removed: Vec<PathBuf>,
}

/// Remove the planned files. Every file is first moved into a hidden holding
/// folder in the same directory; if any move fails, all moved files go back
/// and nothing is removed.
pub fn execute_plan(plan: &DirPlan, protected_dirs: &[PathBuf]) -> Result<CleanupOutcome, String> {
    if plan.files.is_empty() {
        return Ok(CleanupOutcome::default());
    }
    let hold = plan
        .dir
        .join(format!("{HOLD_PREFIX}{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir(&hold).map_err(|e| format!("{}: {e}", plan.dir.display()))?;
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (i, file) in plan.files.iter().enumerate() {
        let held = hold.join(i.to_string());
        if let Err(error) = std::fs::rename(file, &held) {
            for (original, held) in moved.iter().rev() {
                let _ = std::fs::rename(held, original);
            }
            let _ = std::fs::remove_dir(&hold);
            return Err(format!(
                "Could not remove {}: {error}. Nothing in that folder was changed.",
                file.display()
            ));
        }
        moved.push((file.clone(), held));
    }
    for (_, held) in &moved {
        let _ = std::fs::remove_file(held);
    }
    let _ = std::fs::remove_dir(&hold);

    let mut outcome = CleanupOutcome {
        removed: plan.files.clone(),
        dirs_removed: Vec::new(),
    };
    for sub in PACKAGE_SUBDIRS {
        let p = plan.dir.join(sub);
        if is_plain_dir(&p) && std::fs::remove_dir(&p).is_ok() {
            outcome.dirs_removed.push(p);
        }
    }
    if !protected_dirs.iter().any(|p| same_path(p, &plan.dir))
        && is_plain_dir(&plan.dir)
        && std::fs::remove_dir(&plan.dir).is_ok()
    {
        outcome.dirs_removed.push(plan.dir.clone());
    }
    Ok(outcome)
}

fn is_plain_dir(p: &Path) -> bool {
    std::fs::symlink_metadata(p)
        .map(|m| m.file_type().is_dir())
        .unwrap_or(false)
}

fn path_key(p: &Path) -> String {
    let resolved = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let s = resolved.to_string_lossy().replace('/', "\\");
    s.trim_start_matches(r"\\?\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

pub fn same_path(a: &Path, b: &Path) -> bool {
    path_key(a) == path_key(b)
}

/// True when `dir` or an ancestor holds a `.git` entry (developer checkout).
pub fn inside_source_checkout(dir: &Path) -> bool {
    dir.ancestors().any(|a| a.join(".git").exists())
}

/// Well-known user folders and the dedicated install folder.
#[derive(Debug, Clone, Default)]
pub struct Locations {
    pub desktops: Vec<PathBuf>,
    pub downloads: Vec<PathBuf>,
    pub documents: Vec<PathBuf>,
    pub profile: Option<PathBuf>,
    pub start_menu: Vec<PathBuf>,
    pub install_dir: PathBuf,
    pub appdata: PathBuf,
}

impl Locations {
    /// Folders a loose FerricDAW package must never be installed into.
    pub fn shared_folders(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = Vec::new();
        v.extend(self.desktops.iter().cloned());
        v.extend(self.downloads.iter().cloned());
        v.extend(self.documents.iter().cloned());
        v.extend(self.profile.iter().cloned());
        v
    }

    pub fn is_shared_folder(&self, dir: &Path) -> bool {
        dir.parent().is_none() || self.shared_folders().iter().any(|s| same_path(s, dir))
    }

    /// Short label for UI copy.
    pub fn folder_label(&self, dir: &Path) -> String {
        if self.desktops.iter().any(|d| same_path(d, dir)) {
            "Desktop".into()
        } else if self.downloads.iter().any(|d| same_path(d, dir)) {
            "Downloads folder".into()
        } else if self.documents.iter().any(|d| same_path(d, dir)) {
            "Documents folder".into()
        } else {
            dir.display().to_string()
        }
    }

    pub fn history_file(&self) -> PathBuf {
        self.appdata.join("install_history.txt")
    }
}

pub fn default_install_dir(local_appdata: &Path) -> PathBuf {
    local_appdata.join("Programs").join("FerricDAW")
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Resolve user folders. `FERRIC_DESKTOP_DIR`, `FERRIC_DOWNLOADS_DIR`,
/// `FERRIC_DOCUMENTS_DIR`, `FERRIC_START_MENU_DIR` and `FERRIC_PROFILE_DIR`
/// override the system values (used by the sandbox update test).
pub fn probe_locations() -> Locations {
    let local = env_dir("LOCALAPPDATA").unwrap_or_else(std::env::temp_dir);
    let mut locs = Locations {
        install_dir: env_dir("FERRIC_INSTALL_DIR").unwrap_or_else(|| default_install_dir(&local)),
        appdata: local.join("FerricDAW"),
        profile: env_dir("FERRIC_PROFILE_DIR").or_else(|| env_dir("USERPROFILE")),
        ..Default::default()
    };
    let overrides = [
        env_dir("FERRIC_DESKTOP_DIR"),
        env_dir("FERRIC_DOWNLOADS_DIR"),
        env_dir("FERRIC_DOCUMENTS_DIR"),
        env_dir("FERRIC_START_MENU_DIR"),
    ];
    let system = if overrides.iter().all(Option::is_some) {
        None
    } else {
        shell::known_folders().ok()
    };
    let pick = |i: usize, key: &str| -> Vec<PathBuf> {
        overrides[i]
            .clone()
            .or_else(|| {
                system
                    .as_ref()
                    .and_then(|m| m.get(key).cloned())
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
            })
            .into_iter()
            .collect()
    };
    locs.desktops = pick(0, "desktop");
    locs.downloads = pick(1, "downloads");
    locs.documents = pick(2, "documents");
    locs.start_menu = pick(3, "programs");
    locs
}

pub fn read_history(locs: &Locations) -> Vec<PathBuf> {
    std::fs::read_to_string(locs.history_file())
        .map(|t| {
            t.lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default()
}

pub fn record_history(locs: &Locations, dir: &Path) {
    let mut entries = read_history(locs);
    entries.retain(|p| !same_path(p, dir));
    entries.insert(0, dir.to_path_buf());
    entries.truncate(16);
    let _ = std::fs::create_dir_all(&locs.appdata);
    let body: String = entries
        .iter()
        .map(|p| format!("{}\n", p.display()))
        .collect();
    let _ = std::fs::write(locs.history_file(), body);
}

/// Folders that may hold an older FerricDAW copy. No disk-wide scan:
/// Desktop and Downloads (plus their direct subfolders that hold
/// `ferricdaw.exe`), shortcut targets, recorded history, the install folder
/// and an explicit relocation source.
pub fn candidate_dirs(
    locs: &Locations,
    shortcut_targets: &[PathBuf],
    history: &[PathBuf],
    extra: &[PathBuf],
    current_dir: &Path,
) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for root in locs.desktops.iter().chain(locs.downloads.iter()) {
        found.push(root.clone());
        if let Ok(rd) = std::fs::read_dir(root) {
            for entry in rd.flatten() {
                let p = entry.path();
                if is_plain_dir(&p) && p.join(EXE_NAME).is_file() {
                    found.push(p);
                }
            }
        }
    }
    for target in shortcut_targets {
        let is_ferric = target
            .file_name()
            .map(|n| n.to_string_lossy().eq_ignore_ascii_case(EXE_NAME))
            .unwrap_or(false);
        if is_ferric {
            if let Some(parent) = target.parent() {
                found.push(parent.to_path_buf());
            }
        }
    }
    found.extend(history.iter().cloned());
    found.push(locs.install_dir.clone());
    found.extend(extra.iter().cloned());

    let mut out: Vec<PathBuf> = Vec::new();
    for dir in found {
        if !is_plain_dir(&dir)
            || same_path(&dir, current_dir)
            || inside_source_checkout(&dir)
            || same_path(&dir, &locs.appdata)
            || out.iter().any(|d| same_path(d, &dir))
        {
            continue;
        }
        out.push(dir);
    }
    out
}

/// Leftovers in the temp folder from older updaters: downloaded ZIPs whose
/// SHA-256 matches a published package, the v1.1.12 helper script, and
/// manifest-matched files in `ferricdaw-update-stage-*` folders.
/// CRC-32 (IEEE) of a file, as stored in ZIP central directories.
fn crc32_file(path: &Path) -> std::io::Result<u32> {
    let mut table = [0u32; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *slot = c;
    }
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 1 << 16];
    let mut crc = 0xFFFF_FFFFu32;
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        for &b in &buf[..n] {
            crc = table[((crc ^ u32::from(b)) & 0xFF) as usize] ^ (crc >> 8);
        }
    }
    Ok(!crc)
}

/// (name, uncompressed size, crc32) for every entry in a ZIP's central directory.
fn zip_entries(path: &Path) -> Option<Vec<(String, u64, u32)>> {
    let data = std::fs::read(path).ok()?;
    let u16_at = |o: usize| data.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| data.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let lowest = data.len().saturating_sub(22 + 65_535);
    let eocd = (lowest..=data.len().checked_sub(22)?)
        .rev()
        .find(|&o| u32_at(o) == Some(0x0605_4b50))?;
    let count = u16_at(eocd + 10)? as usize;
    let mut at = u32_at(eocd + 16)? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if u32_at(at)? != 0x0201_4b50 {
            return None;
        }
        let crc = u32_at(at + 16)?;
        let size = u64::from(u32_at(at + 24)?);
        let name_len = u16_at(at + 28)? as usize;
        let extra = u16_at(at + 30)? as usize;
        let comment = u16_at(at + 32)? as usize;
        let name = String::from_utf8(data.get(at + 46..at + 46 + name_len)?.to_vec()).ok()?;
        out.push((name, size, crc));
        at += 46 + name_len + extra + comment;
    }
    Some(out)
}

/// True when `zip` holds exactly the package installed in `dir`: the same
/// file list, and every entry's size and CRC-32 equal the installed file,
/// which itself must match a known manifest (the running release included).
/// This recognises the update ZIP an older updater left behind for the
/// running release, whose own hash no build can know in advance.
fn zip_matches_installed(zip: &Path, dir: &Path, index: &ManifestIndex) -> bool {
    let Some(entries) = zip_entries(zip) else { return false };
    let mut saw_exe = false;
    let mut any = false;
    for (name, size, crc) in entries {
        let rel = name.replace('\\', "/");
        if rel.ends_with('/') {
            continue;
        }
        if !is_safe_relative(&rel) {
            return false;
        }
        let file = dir.join(&rel);
        let known = if rel.eq_ignore_ascii_case(MANIFEST_NAME) {
            read_manifest(dir).is_some()
        } else {
            index.match_file(dir, &rel).is_some()
        };
        let same = known
            && std::fs::metadata(&file).map(|m| m.len() == size).unwrap_or(false)
            && crc32_file(&file).map(|c| c == crc).unwrap_or(false);
        if !same {
            return false;
        }
        saw_exe |= rel.eq_ignore_ascii_case(EXE_NAME);
        any = true;
    }
    any && saw_exe
}

pub fn temp_leftovers(temp: &Path, index: &ManifestIndex, current_dir: &Path) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut files = Vec::new();
    let mut stage_dirs = Vec::new();
    let Ok(rd) = std::fs::read_dir(temp) else {
        return (files, stage_dirs);
    };
    for entry in rd.flatten() {
        let p = entry.path();
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
        if meta.file_type().is_file() {
            let zip = name.ends_with(".zip")
                && (name.starts_with("ferricdaw-update-")
                    || (name.starts_with("ferricdaw-v") && name.ends_with("-update.zip")));
            if zip && (index.is_known_zip(&p) || zip_matches_installed(&p, current_dir, index)) {
                files.push(p);
            } else if name == "ferricdaw-apply-update.ps1" {
                let ours = std::fs::read_to_string(&p)
                    .map(|t| t.contains("ferricdaw-update-stage-") && t.contains("Expand-Archive"))
                    .unwrap_or(false);
                if ours {
                    files.push(p);
                }
            }
        } else if meta.file_type().is_dir() && name.starts_with("ferricdaw-update-stage-") {
            for sub in [p.clone(), p.join("files"), p.join("previous")] {
                for rel in index.originals() {
                    if index.match_file(&sub, rel).is_some() {
                        files.push(sub.join(rel));
                    }
                }
            }
            stage_dirs.push(p);
        }
    }
    (files, stage_dirs)
}

/// Copy the running package into `install_dir`. Every source file must match
/// `manifest`; replaced files are restored if any step fails.
pub fn install_copy(src_dir: &Path, manifest: &PackageManifest, install_dir: &Path) -> Result<(), String> {
    for f in &manifest.files {
        if !is_safe_relative(&f.path) {
            return Err(format!("Unsafe manifest path {}", f.path));
        }
        let p = src_dir.join(&f.path);
        let ok = std::fs::metadata(&p).map(|m| m.len() == f.size).unwrap_or(false)
            && sha256_file(&p).map(|h| h.eq_ignore_ascii_case(&f.sha256)).unwrap_or(false);
        if !ok {
            return Err(format!(
                "{} does not match this FerricDAW release; nothing was copied.",
                p.display()
            ));
        }
    }
    std::fs::create_dir_all(install_dir).map_err(|e| e.to_string())?;
    let backup = install_dir.join(format!("{HOLD_PREFIX}{}", uuid::Uuid::new_v4().simple()));
    let mut placed: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
    let mut names: Vec<String> = manifest.files.iter().map(|f| f.path.clone()).collect();
    names.push(MANIFEST_NAME.into());
    let result = (|| -> Result<(), String> {
        for (i, rel) in names.iter().enumerate() {
            let src = src_dir.join(rel);
            let dst = install_dir.join(rel);
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut previous = None;
            if dst.exists() {
                if !std::fs::symlink_metadata(&dst).map(|m| m.is_file()).unwrap_or(false) {
                    return Err(format!("{} is not a file.", dst.display()));
                }
                std::fs::create_dir_all(&backup).map_err(|e| e.to_string())?;
                let held = backup.join(i.to_string());
                std::fs::rename(&dst, &held).map_err(|e| format!("{}: {e}", dst.display()))?;
                previous = Some(held);
            }
            placed.push((dst.clone(), previous));
            std::fs::copy(&src, &dst).map_err(|e| format!("{}: {e}", dst.display()))?;
        }
        for f in &manifest.files {
            let h = sha256_file(&install_dir.join(&f.path)).map_err(|e| e.to_string())?;
            if !h.eq_ignore_ascii_case(&f.sha256) {
                return Err(format!("Copy verification failed for {}", f.path));
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            for (_, previous) in &placed {
                if let Some(p) = previous {
                    let _ = std::fs::remove_file(p);
                }
            }
            let _ = std::fs::remove_dir(&backup);
            Ok(())
        }
        Err(error) => {
            for (dst, previous) in placed.iter().rev() {
                let _ = std::fs::remove_file(dst);
                if let Some(p) = previous {
                    let _ = std::fs::rename(p, dst);
                }
            }
            let _ = std::fs::remove_dir(&backup);
            Err(error)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HousekeepingReport {
    pub removed_files: usize,
    pub cleaned_dirs: Vec<PathBuf>,
    #[allow(dead_code)]
    pub removed_dirs: Vec<PathBuf>,
    pub retargeted_shortcuts: Vec<PathBuf>,
    pub created_shortcut: Option<PathBuf>,
    pub errors: Vec<String>,
}

/// Remove older copies and temp leftovers, retarget shortcuts that pointed at
/// removed executables, and record this folder. `wait_for` retries the
/// relocation source while the previous instance still holds its files.
pub fn clean_old_copies(
    locs: &Locations,
    index: &ManifestIndex,
    current_version: &str,
    current_exe: &Path,
    relocated_from: Option<&Path>,
    temp: &Path,
    shortcuts: &[shell::Shortcut],
) -> HousekeepingReport {
    let mut report = HousekeepingReport::default();
    let Some(current_dir) = current_exe.parent() else { return report };
    let targets: Vec<PathBuf> = shortcuts.iter().map(|s| s.target.clone()).collect();
    let extra: Vec<PathBuf> = relocated_from.map(Path::to_path_buf).into_iter().collect();
    let dirs = candidate_dirs(locs, &targets, &read_history(locs), &extra, current_dir);
    let protected = locs.shared_folders();
    let mut removed_exes: Vec<PathBuf> = Vec::new();
    for dir in dirs {
        let is_source = relocated_from.map(|r| same_path(r, &dir)).unwrap_or(false);
        let attempts = if is_source { 30 } else { 1 };
        for attempt in 0..attempts {
            let Some(plan) = plan_dir(&dir, index, current_version) else { break };
            match execute_plan(&plan, &protected) {
                Ok(outcome) => {
                    if !outcome.removed.is_empty() {
                        report.removed_files += outcome.removed.len();
                        report.cleaned_dirs.push(dir.clone());
                        removed_exes.push(dir.join(EXE_NAME));
                    }
                    report.removed_dirs.extend(outcome.dirs_removed);
                    break;
                }
                Err(error) => {
                    if attempt + 1 == attempts {
                        report.errors.push(error);
                    } else {
                        std::thread::sleep(std::time::Duration::from_secs(1));
                    }
                }
            }
        }
    }
    let (temp_files, stage_dirs) = temp_leftovers(temp, index, current_dir);
    for f in temp_files {
        if std::fs::remove_file(&f).is_ok() {
            report.removed_files += 1;
        }
    }
    for d in stage_dirs {
        for sub in [d.join("files"), d.join("previous")] {
            for pkg in PACKAGE_SUBDIRS {
                let _ = std::fs::remove_dir(sub.join(pkg));
            }
            let _ = std::fs::remove_dir(&sub);
        }
        for pkg in PACKAGE_SUBDIRS {
            let _ = std::fs::remove_dir(d.join(pkg));
        }
        let _ = std::fs::remove_dir(&d);
    }
    for s in shortcuts {
        if removed_exes.iter().any(|e| same_path(e, &s.target) || path_key(e) == path_key(&s.target)) {
            match shell::retarget_shortcut(&s.path, current_exe) {
                Ok(()) => report.retargeted_shortcuts.push(s.path.clone()),
                Err(e) => report.errors.push(e),
            }
        }
    }
    record_history(locs, current_dir);
    report
}

/// Windows shell helpers (known folders and `.lnk` shortcuts) through
/// `WScript.Shell`, run hidden with arguments passed in environment variables.
pub mod shell {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    #[derive(Debug, Clone)]
    pub struct Shortcut {
        pub path: PathBuf,
        pub target: PathBuf,
    }

    fn base64(data: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    pub(crate) fn encode_command(script: &str) -> String {
        let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        base64(&utf16)
    }

    fn run(script: &str, env: &[(&str, &Path)]) -> Result<String, String> {
        let mut cmd = std::process::Command::new("powershell");
        cmd.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand"])
            .arg(encode_command(script));
        for (k, v) in env {
            cmd.env(k, v);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let out = cmd.output().map_err(|e| format!("PowerShell: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    pub fn known_folders() -> Result<BTreeMap<String, String>, String> {
        let script = r#"
$ErrorActionPreference = 'Stop'
$downloads = ''
try { $downloads = (New-Object -ComObject Shell.Application).NameSpace('shell:Downloads').Self.Path } catch {}
[Console]::OutputEncoding = [Text.Encoding]::UTF8
[ordered]@{
  desktop = [Environment]::GetFolderPath('DesktopDirectory')
  documents = [Environment]::GetFolderPath('MyDocuments')
  programs = [Environment]::GetFolderPath('Programs')
  downloads = $downloads
} | ConvertTo-Json -Compress
"#;
        let out = run(script, &[])?;
        serde_json::from_str(out.trim()).map_err(|e| e.to_string())
    }

    /// `.lnk` files directly on the desktops and anywhere under the Start Menu
    /// programs folders, with their resolved targets.
    pub fn list_shortcuts(desktops: &[PathBuf], start_menus: &[PathBuf]) -> Vec<Shortcut> {
        let join = |v: &[PathBuf]| {
            v.iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("|")
        };
        let flat = PathBuf::from(join(desktops));
        let deep = PathBuf::from(join(start_menus));
        let script = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$shell = New-Object -ComObject WScript.Shell
$items = @()
foreach ($d in ($env:FERRIC_LNK_FLAT -split '\|')) { if ($d -and (Test-Path -LiteralPath $d)) { $items += Get-ChildItem -LiteralPath $d -Filter *.lnk -File -ErrorAction SilentlyContinue } }
foreach ($d in ($env:FERRIC_LNK_DEEP -split '\|')) { if ($d -and (Test-Path -LiteralPath $d)) { $items += Get-ChildItem -LiteralPath $d -Filter *.lnk -File -Recurse -ErrorAction SilentlyContinue } }
foreach ($i in $items) {
  try { $t = $shell.CreateShortcut($i.FullName).TargetPath } catch { $t = '' }
  if ($t) { [pscustomobject]@{ path = $i.FullName; target = $t } | ConvertTo-Json -Compress }
}
"#;
        let Ok(out) = run(script, &[("FERRIC_LNK_FLAT", &flat), ("FERRIC_LNK_DEEP", &deep)]) else {
            return Vec::new();
        };
        out.lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l.trim()).ok())
            .filter_map(|v| {
                Some(Shortcut {
                    path: PathBuf::from(v.get("path")?.as_str()?),
                    target: PathBuf::from(v.get("target")?.as_str()?),
                })
            })
            .collect()
    }

    pub fn retarget_shortcut(lnk: &Path, exe: &Path) -> Result<(), String> {
        let dir = exe.parent().unwrap_or(exe);
        let script = r#"
$ErrorActionPreference = 'Stop'
$s = (New-Object -ComObject WScript.Shell).CreateShortcut($env:FERRIC_LNK)
$s.TargetPath = $env:FERRIC_EXE
$s.WorkingDirectory = $env:FERRIC_DIR
$s.IconLocation = "$($env:FERRIC_EXE),0"
$s.Save()
"#;
        run(script, &[("FERRIC_LNK", lnk), ("FERRIC_EXE", exe), ("FERRIC_DIR", dir)])
            .map(|_| ())
            .map_err(|e| format!("Could not update shortcut {}: {e}", lnk.display()))
    }

    /// Create `FerricDAW.lnk` on `desktop`, or reuse an existing shortcut that
    /// already targets `exe`. Never overwrites an unrelated file.
    pub fn create_desktop_shortcut(desktop: &Path, exe: &Path, existing: &[Shortcut]) -> Result<PathBuf, String> {
        if let Some(s) = existing
            .iter()
            .find(|s| s.path.parent().map(|p| super::same_path(p, desktop)).unwrap_or(false) && super::same_path(&s.target, exe))
        {
            return Ok(s.path.clone());
        }
        let mut lnk = desktop.join("FerricDAW.lnk");
        let mut n = 2;
        while lnk.exists() {
            lnk = desktop.join(format!("FerricDAW ({n}).lnk"));
            n += 1;
        }
        retarget_shortcut(&lnk, exe)?;
        Ok(lnk)
    }

    pub fn has_desktop_shortcut(desktops: &[PathBuf], exe: &Path, existing: &[Shortcut]) -> bool {
        existing.iter().any(|s| {
            super::same_path(&s.target, exe)
                && s.path
                    .parent()
                    .map(|p| desktops.iter().any(|d| super::same_path(d, p)))
                    .unwrap_or(false)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const CURRENT: &str = "1.1.14";

    fn put(dir: &Path, rel: &str, body: &[u8]) {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    fn entry(rel: &str, body: &[u8]) -> ManifestEntry {
        ManifestEntry {
            path: rel.into(),
            size: body.len() as u64,
            sha256: hex::encode(Sha256::digest(body)),
        }
    }

    const OLD: [(&str, &[u8]); 6] = [
        ("ferricdaw.exe", b"old exe 1.1.12"),
        ("ferric-plugin-bridge.exe", b"bridge"),
        ("README.md", b"readme 1.1.12"),
        ("update_feed.url", b"https://github.com/reala1ynex/FerricDAW/releases/latest/download/latest.json"),
        ("demo/NightCurrent-Demo.wav", b"RIFF demo"),
        ("docs/DEVLOG.md", b"devlog 1.1.12"),
    ];

    fn old_manifest(version: &str) -> PackageManifest {
        PackageManifest {
            version: version.into(),
            zip_sha256: None,
            files: OLD.iter().map(|(r, b)| entry(r, b)).collect(),
        }
    }

    fn lay_down(dir: &Path) {
        for (rel, body) in OLD {
            put(dir, rel, body);
        }
    }

    fn user_data(dir: &Path) {
        put(dir, "plugins/MyVerb.vst3/Contents/x86_64-win/MyVerb.vst3", b"user plugin");
        put(dir, "plugins/Grit.clap", b"user clap");
        put(dir, "My Song.ferric", b"project");
        put(dir, "settings.json", b"{\"buffer\":256}");
        put(dir, "audio-setup.txt", b"Realtek / Line In");
        put(dir, "presets/Dark.ferricpreset", b"preset");
        put(dir, "demo/my-sample.wav", b"user sample");
        put(dir, "ferric.log", b"log");
    }

    fn assert_user_data(dir: &Path) {
        for rel in [
            "plugins/MyVerb.vst3/Contents/x86_64-win/MyVerb.vst3",
            "plugins/Grit.clap",
            "My Song.ferric",
            "settings.json",
            "audio-setup.txt",
            "presets/Dark.ferricpreset",
            "demo/my-sample.wav",
            "ferric.log",
        ] {
            assert!(dir.join(rel).is_file(), "user file removed: {rel}");
        }
    }

    fn locs_in(root: &Path) -> Locations {
        let mk = |n: &str| {
            let p = root.join(n);
            fs::create_dir_all(&p).unwrap();
            p
        };
        Locations {
            desktops: vec![mk("Desktop")],
            downloads: vec![mk("Downloads")],
            documents: vec![mk("Documents")],
            profile: Some(root.to_path_buf()),
            start_menu: vec![mk("StartMenu")],
            install_dir: root.join("AppData/Local/Programs/FerricDAW"),
            appdata: mk("AppData/Local/FerricDAW"),
        }
    }

    #[test]
    fn embedded_manifests_cover_published_packages() {
        let all = embedded_manifests();
        let zips: Vec<_> = all.iter().filter_map(|m| m.zip_sha256.clone()).collect();
        // Public v1.1.12 and v1.1.13 release assets.
        assert!(zips.contains(&"df47ae3c82ec0f1afbb92b629bcf6439a06c83597db79d408b80e3d8a871f91b".into()));
        assert!(zips.contains(&"4037dc01d1c6aa3536847d0ee2384e1f0a8c230fbe644dc54a06efa6557a72ae".into()));
        for m in &all {
            assert!(m.files.iter().any(|f| f.path == EXE_NAME), "{} has no exe", m.version);
            assert!(m.files.iter().all(|f| is_safe_relative(&f.path)));
        }
    }

    #[test]
    fn desktop_dump_removes_only_exact_package_files() {
        let root = tempfile::tempdir().unwrap();
        let locs = locs_in(root.path());
        let desktop = &locs.desktops[0];
        lay_down(desktop);
        user_data(desktop);
        put(desktop, "docs/DEVLOG.md", b"edited by the user"); // same name, different bytes
        put(desktop, "README.md", b"readme 1.1.12");
        let index = ManifestIndex::new(&[old_manifest("1.1.12")]);

        let plan = plan_dir(desktop, &index, CURRENT).expect("recognised copy");
        assert_eq!(plan.exe_version, "1.1.12");
        let out = execute_plan(&plan, &locs.shared_folders()).unwrap();
        assert_eq!(out.removed.len(), 5);
        for rel in ["ferricdaw.exe", "ferric-plugin-bridge.exe", "README.md", "update_feed.url", "demo/NightCurrent-Demo.wav"] {
            assert!(!desktop.join(rel).exists(), "{rel} should be gone");
        }
        assert!(desktop.join("docs/DEVLOG.md").is_file(), "edited file kept");
        assert_user_data(desktop);
        assert!(desktop.is_dir(), "Desktop itself is never removed");
        assert!(fs::read_dir(desktop).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with(HOLD_PREFIX)));
    }

    #[test]
    fn folder_without_recognised_exe_is_left_alone() {
        let root = tempfile::tempdir().unwrap();
        lay_down(root.path());
        put(root.path(), "ferricdaw.exe", b"someone else's build");
        let index = ManifestIndex::new(&[old_manifest("1.1.12")]);
        assert!(plan_dir(root.path(), &index, CURRENT).is_none());
        assert!(root.path().join("README.md").is_file());
    }

    #[test]
    fn newer_install_is_never_removed() {
        let root = tempfile::tempdir().unwrap();
        lay_down(root.path());
        let index = ManifestIndex::new(&[old_manifest("1.2.0")]);
        assert!(plan_dir(root.path(), &index, CURRENT).is_none());
    }

    #[cfg(windows)]
    #[test]
    fn locked_file_rolls_back_the_whole_folder() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        lay_down(root.path());
        let index = ManifestIndex::new(&[old_manifest("1.1.12")]);
        let plan = plan_dir(root.path(), &index, CURRENT).unwrap();
        // Hold the last planned file open without delete sharing, like a running exe.
        let locked = plan.files.last().unwrap().clone();
        let guard = fs::OpenOptions::new().read(true).share_mode(1).open(&locked).unwrap();
        let err = execute_plan(&plan, &[]).unwrap_err();
        assert!(err.contains("Nothing in that folder was changed"));
        drop(guard);
        for (rel, body) in OLD {
            assert_eq!(fs::read(root.path().join(rel)).unwrap(), body, "{rel} restored");
        }
        assert!(fs::read_dir(root.path()).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with(HOLD_PREFIX)));
    }

    #[test]
    fn emptied_former_install_folder_is_removed_but_user_folder_kept() {
        let root = tempfile::tempdir().unwrap();
        let only_program = root.path().join("FerricDAW-v1.1.12");
        let with_plugins = root.path().join("FerricDAW-old");
        lay_down(&only_program);
        lay_down(&with_plugins);
        put(&with_plugins, "plugins/Grit.clap", b"user clap");
        let index = ManifestIndex::new(&[old_manifest("1.1.12")]);
        execute_plan(&plan_dir(&only_program, &index, CURRENT).unwrap(), &[]).unwrap();
        execute_plan(&plan_dir(&with_plugins, &index, CURRENT).unwrap(), &[]).unwrap();
        assert!(!only_program.exists());
        assert!(with_plugins.join("plugins/Grit.clap").is_file());
        assert!(!with_plugins.join("ferricdaw.exe").exists());
    }

    #[test]
    fn discovery_finds_copies_in_every_known_place_and_skips_unsafe_ones() {
        let root = tempfile::tempdir().unwrap();
        let locs = locs_in(root.path());
        let current = root.path().join("AppData/Local/Programs/FerricDAW");
        fs::create_dir_all(&current).unwrap();
        let desktop = locs.desktops[0].clone();
        let download_copy = locs.downloads[0].join("FerricDAW-v1.1.12-win-x64");
        let shortcut_dir = root.path().join("D/Music Apps/FerricDAW");
        let history_dir = root.path().join("E/Old Ferric");
        let checkout = root.path().join("src/FerricDAW");
        let checkout_build = checkout.join("dist/FerricDAW");
        let no_exe = locs.downloads[0].join("Some Other Folder");
        for d in [&desktop, &download_copy, &shortcut_dir, &history_dir, &checkout_build] {
            lay_down(d);
        }
        fs::create_dir_all(checkout.join(".git")).unwrap();
        put(&no_exe, "README.md", b"readme 1.1.12");
        user_data(&shortcut_dir);
        record_history(&locs, &history_dir);

        let dirs = candidate_dirs(
            &locs,
            &[shortcut_dir.join("ferricdaw.exe"), checkout_build.join("ferricdaw.exe")],
            &read_history(&locs),
            &[],
            &current,
        );
        let has = |p: &Path| dirs.iter().any(|d| same_path(d, p));
        assert!(has(&desktop) && has(&download_copy) && has(&shortcut_dir) && has(&history_dir));
        assert!(!has(&checkout_build), "source checkouts are skipped");
        assert!(!has(&current), "running folder is skipped");
        assert!(!has(&locs.appdata), "settings folder is skipped");

        let index = ManifestIndex::new(&[old_manifest("1.1.12")]);
        let mut cleaned = 0;
        for d in &dirs {
            if let Some(plan) = plan_dir(d, &index, CURRENT) {
                execute_plan(&plan, &locs.shared_folders()).unwrap();
                cleaned += 1;
            }
        }
        assert_eq!(cleaned, 4);
        assert!(!download_copy.exists() && !history_dir.exists());
        assert_user_data(&shortcut_dir);
        assert!(!shortcut_dir.join("ferricdaw.exe").exists());
        assert!(checkout_build.join("ferricdaw.exe").is_file());
        assert!(no_exe.join("README.md").is_file());
        assert!(desktop.is_dir() && locs.downloads[0].is_dir());
    }

    #[test]
    fn shared_folders_are_detected() {
        let root = tempfile::tempdir().unwrap();
        let locs = locs_in(root.path());
        assert!(locs.is_shared_folder(&locs.desktops[0]));
        assert!(locs.is_shared_folder(&locs.downloads[0]));
        assert!(locs.is_shared_folder(root.path()));
        assert!(!locs.is_shared_folder(&root.path().join("AppData/Local/Programs/FerricDAW")));
        assert_eq!(locs.folder_label(&locs.desktops[0]), "Desktop");
    }

    #[test]
    fn install_copy_verifies_and_rolls_back() {
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("Desktop");
        let dst = root.path().join("Programs/FerricDAW");
        lay_down(&src);
        let manifest = old_manifest("1.1.14");
        fs::write(src.join(MANIFEST_NAME), serde_json::to_vec(&manifest).unwrap()).unwrap();
        put(&dst, "README.md", b"previous readme");
        put(&dst, "plugins/Keep.clap", b"user");

        // Corrupt source: nothing copied, destination untouched.
        fs::write(src.join("docs/DEVLOG.md"), b"tampered").unwrap();
        assert!(install_copy(&src, &manifest, &dst).is_err());
        assert_eq!(fs::read(dst.join("README.md")).unwrap(), b"previous readme");
        assert!(!dst.join("ferricdaw.exe").exists());

        fs::write(src.join("docs/DEVLOG.md"), b"devlog 1.1.12").unwrap();
        install_copy(&src, &manifest, &dst).unwrap();
        for (rel, body) in OLD {
            assert_eq!(fs::read(dst.join(rel)).unwrap(), body);
        }
        assert!(read_manifest(&dst).is_some());
        assert!(dst.join("plugins/Keep.clap").is_file());
        assert!(fs::read_dir(&dst).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with(HOLD_PREFIX)));
    }

    /// Minimal stored (uncompressed) ZIP writer for tests.
    fn make_zip(path: &Path, entries: &[(&str, &[u8])]) {
        fn crc(data: &[u8]) -> u32 {
            let mut c = 0xFFFF_FFFFu32;
            for &b in data {
                c ^= u32::from(b);
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                }
            }
            !c
        }
        let mut out: Vec<u8> = Vec::new();
        let mut central: Vec<u8> = Vec::new();
        for (name, body) in entries {
            let offset = out.len() as u32;
            let (c, n) = (crc(body), body.len() as u32);
            out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
            out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            for v in [c, n, n] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes());
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(body);
            central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            central.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
            for v in [c, n, n] {
                central.extend_from_slice(&v.to_le_bytes());
            }
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&[0u8; 12]);
            central.extend_from_slice(&offset.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_offset = out.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(central.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, out).unwrap();
    }

    #[test]
    fn update_zip_of_the_running_release_is_recognised_by_its_contents() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("Programs/FerricDAW");
        lay_down(&install);
        let index = ManifestIndex::new(&[old_manifest("1.1.14")]);
        let temp = root.path().join("Temp");
        let same: Vec<(&str, &[u8])> = OLD.iter().map(|(r, b)| (*r, *b)).collect();
        make_zip(&temp.join("FerricDAW-v1.1.14-update.zip"), &same);
        let mut extra = same.clone();
        extra.push(("plugins/Grit.clap", b"user clap"));
        make_zip(&temp.join("FerricDAW-v1.1.15-update.zip"), &extra);
        let mut changed = same.clone();
        changed[0].1 = b"tampered";
        make_zip(&temp.join("ferricdaw-update-tampered.zip"), &changed);
        let missing_exe: Vec<(&str, &[u8])> = same.iter().copied().filter(|(r, _)| *r != EXE_NAME).collect();
        make_zip(&temp.join("FerricDAW-update-noexe.zip"), &missing_exe);
        make_zip(&temp.join("my-backup.zip"), &same);
        let (files, _) = temp_leftovers(&temp, &index, &install);
        let names: Vec<String> = files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into()).collect();
        assert_eq!(names, vec!["FerricDAW-v1.1.14-update.zip".to_string()]);
        // Without a verified install to compare against nothing matches.
        let (files, _) = temp_leftovers(&temp, &index, &root.path().join("nowhere"));
        assert!(files.is_empty());
    }

    #[test]
    fn stray_manifest_and_temp_leftovers_are_identified_by_content() {
        let root = tempfile::tempdir().unwrap();
        let desktop = root.path().join("Desktop");
        lay_down(&desktop);
        fs::write(desktop.join(MANIFEST_NAME), serde_json::to_vec(&old_manifest("1.1.14")).unwrap()).unwrap();
        let index = ManifestIndex::new(&[old_manifest("1.1.14")]);
        let plan = plan_dir(&desktop, &index, CURRENT).unwrap();
        assert!(plan.files.iter().any(|f| f.ends_with(MANIFEST_NAME)));

        let temp = root.path().join("Temp");
        let zip_body = b"PK pretend zip";
        put(&temp, "FerricDAW-v1.1.13-update.zip", zip_body);
        put(&temp, "FerricDAW-v1.1.13-update-unknown.zip", b"other");
        put(&temp, "FerricDAW-update-1234.zip", b"not a release");
        put(&temp, "ferricdaw-apply-update.ps1", b"$stage = 'ferricdaw-update-stage-'; Expand-Archive x");
        put(&temp, "unrelated.zip", zip_body);
        lay_down(&temp.join("ferricdaw-update-stage-abc"));
        put(&temp.join("ferricdaw-update-stage-abc"), "notes.txt", b"keep");
        let mut m = old_manifest("1.1.13");
        m.zip_sha256 = Some(hex::encode(Sha256::digest(zip_body)));
        let index = ManifestIndex::new(&[m]);
        let (files, stages) = temp_leftovers(&temp, &index, &root.path().join("nowhere"));
        let names: Vec<String> = files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into()).collect();
        assert!(names.contains(&"FerricDAW-v1.1.13-update.zip".to_string()));
        assert!(names.contains(&"ferricdaw-apply-update.ps1".to_string()));
        assert!(!names.iter().any(|n| n == "unrelated.zip" || n == "FerricDAW-update-1234.zip" || n == "notes.txt"));
        assert_eq!(stages.len(), 1);
        assert_eq!(files.len(), 2 + OLD.len());
    }

    #[test]
    fn unsafe_manifest_paths_are_ignored() {
        assert!(!is_safe_relative("../Desktop/x"));
        assert!(!is_safe_relative("C:/Windows/x"));
        assert!(!is_safe_relative("/abs"));
        assert!(is_safe_relative("docs/DEVLOG.md"));
        let mut m = old_manifest("1.1.12");
        m.files.push(entry("../outside.txt", b"x"));
        let index = ManifestIndex::new(&[m]);
        assert!(index.originals().all(|p| is_safe_relative(p)));
    }

    #[test]
    fn encoded_command_round_trips() {
        assert_eq!(shell::encode_command("a"), "YQA=");
    }
}
