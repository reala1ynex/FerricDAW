//! Portable-zip auto-update: check `latest.json`, download, apply on restart.
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::Duration;

/// Override with env `FERRIC_UPDATE_FEED`, or ship `update_feed.url` next to the exe.
pub const DEFAULT_FEED_URL: &str =
    "https://github.com/reala1ynex/FerricDAW/releases/latest/download/latest.json";

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateFeed {
    pub version: String,
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone)]
pub enum UpdateEvent {
    UpToDate {
        #[allow(dead_code)]
        current: String,
    },
    Available(UpdateFeed),
    Error(String),
    #[allow(dead_code)]
    DownloadProgress { received: u64, total: Option<u64> },
    Downloaded { zip_path: PathBuf },
    AppliedRestarting,
}

pub struct UpdateService {
    tx: Sender<UpdateEvent>,
    rx: Receiver<UpdateEvent>,
    pub pending: Option<UpdateFeed>,
    pub last_error: Option<String>,
    pub downloading: bool,
    pub download_path: Option<PathBuf>,
    pub checked_this_session: bool,
}

impl Default for UpdateService {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            pending: None,
            last_error: None,
            downloading: false,
            download_path: None,
            checked_this_session: false,
        }
    }
}

impl UpdateService {
    pub fn feed_url() -> String {
        if let Ok(u) = std::env::var("FERRIC_UPDATE_FEED") {
            if !u.trim().is_empty() {
                return u.trim().to_string();
            }
        }
        if let Some(beside) = exe_dir().map(|d| d.join("update_feed.url")) {
            if let Ok(s) = std::fs::read_to_string(beside) {
                let t = s.trim().to_string();
                if !t.is_empty() && !t.contains("OWNER/") {
                    return t;
                }
            }
        }
        DEFAULT_FEED_URL.to_string()
    }

    pub fn current_version() -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    pub fn feed_configured() -> bool {
        let u = Self::feed_url();
        feed_url_is_allowed(&u)
    }

    pub fn poll(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                UpdateEvent::UpToDate { .. } => {
                    self.pending = None;
                    self.last_error = None;
                }
                UpdateEvent::Available(feed) => {
                    self.pending = Some(feed);
                    self.last_error = None;
                }
                UpdateEvent::Error(e) => {
                    self.last_error = Some(e);
                    self.downloading = false;
                }
                UpdateEvent::DownloadProgress { .. } => {}
                UpdateEvent::Downloaded { zip_path } => {
                    self.downloading = false;
                    self.download_path = Some(zip_path);
                }
                UpdateEvent::AppliedRestarting => {
                    self.downloading = false;
                }
            }
        }
    }

    pub fn check_async(&mut self) {
        self.checked_this_session = true;
        self.last_error = None;
        let tx = self.tx.clone();
        let feed_url = Self::feed_url();
        let current = Self::current_version().to_string();
        thread::spawn(move || {
            match fetch_feed(&feed_url) {
                Ok(feed) => {
                    if version_is_newer(&feed.version, &current) {
                        let _ = tx.send(UpdateEvent::Available(feed));
                    } else {
                        let _ = tx.send(UpdateEvent::UpToDate { current });
                    }
                }
                Err(e) => {
                    let _ = tx.send(UpdateEvent::Error(e));
                }
            }
        });
    }

    pub fn download_async(&mut self) {
        let Some(feed) = self.pending.clone() else {
            return;
        };
        self.downloading = true;
        self.download_path = None;
        let tx = self.tx.clone();
        thread::spawn(move || match download_zip(&feed) {
            Ok(path) => {
                let _ = tx.send(UpdateEvent::Downloaded { zip_path: path });
            }
            Err(e) => {
                let _ = tx.send(UpdateEvent::Error(e));
            }
        });
    }

    /// Launch helper script to replace files after this process exits, then return true so caller can quit.
    pub fn apply_and_restart(&mut self) -> Result<(), String> {
        let zip = self
            .download_path
            .clone()
            .ok_or_else(|| "Download the update first.".to_string())?;
        let install = exe_dir().ok_or_else(|| "Could not locate install folder.".to_string())?;
        let exe = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .file_name()
            .map(|s| s.to_os_string())
            .ok_or_else(|| "Missing exe name".to_string())?;

        let helper = std::env::temp_dir().join("ferricdaw-apply-update.ps1");
        let script = format!(
            r#"
$ErrorActionPreference = 'Stop'
$zip = {zip}
$dest = {dest}
$exeName = {exe}
# Wait for FerricDAW to exit (file unlock)
$procName = [IO.Path]::GetFileNameWithoutExtension($exeName)
Get-Process -Name $procName -ErrorAction SilentlyContinue | ForEach-Object {{
  try {{ $_.WaitForExit(60000) }} catch {{}}
}}
Start-Sleep -Seconds 1
$stage = Join-Path $env:TEMP ("ferricdaw-update-stage-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Expand-Archive -LiteralPath $zip -DestinationPath $stage -Force
# If zip contains a single top folder, use its contents
$kids = Get-ChildItem -LiteralPath $stage
if ($kids.Count -eq 1 -and $kids[0].PSIsContainer) {{
  $src = $kids[0].FullName
}} else {{
  $src = $stage
}}
Copy-Item -Path (Join-Path $src '*') -Destination $dest -Recurse -Force
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
Start-Process -FilePath (Join-Path $dest $exeName) -WorkingDirectory $dest
"#,
            zip = ps_quote(&zip),
            dest = ps_quote(&install),
            exe = ps_quote_str(&exe.to_string_lossy()),
        );
        std::fs::write(&helper, script).map_err(|e| e.to_string())?;

        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-WindowStyle",
                "Hidden",
                "-File",
                helper.to_str().unwrap_or(""),
            ])
            .spawn()
            .map_err(|e| e.to_string())?;

        let _ = self.tx.send(UpdateEvent::AppliedRestarting);
        Ok(())
    }
}

fn ps_quote(path: &Path) -> String {
    ps_quote_str(&path.to_string_lossy())
}

fn ps_quote_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

/// Reject non-HTTPS / non-GitHub release hosts (portable updater threat model).
pub fn feed_url_is_allowed(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    if lower.contains("owner/") || lower.contains("..") {
        return false;
    }
    if !(lower.starts_with("https://github.com/")
        || lower.starts_with("https://objects.githubusercontent.com/")
        || lower.starts_with("https://release-assets.githubusercontent.com/")
        || lower.starts_with("https://github-releases.githubusercontent.com/"))
    {
        // Allow any https://*.githubusercontent.com/ path used by Releases CDN.
        if let Some(rest) = lower.strip_prefix("https://") {
            let host = rest.split('/').next().unwrap_or("");
            if host.ends_with(".githubusercontent.com") && !host.contains('@') {
                return true;
            }
        }
        return false;
    }
    true
}

fn fetch_feed(url: &str) -> Result<UpdateFeed, String> {
    if url.contains("OWNER/") {
        return Err(
            "Update feed not configured yet — set GitHub Releases URL (update_feed.url)."
                .into(),
        );
    }
    if !feed_url_is_allowed(url) {
        return Err(
            "Update feed URL must be HTTPS on github.com / GitHub release CDN hosts.".into(),
        );
    }
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(8))
        .timeout_read(Duration::from_secs(20))
        .build();
    let resp = agent
        .get(url)
        .set("User-Agent", "FerricDAW-Updater")
        .call()
        .map_err(|e| format!("Update check failed: {e}"))?;
    let mut body = resp
        .into_string()
        .map_err(|e| format!("Could not read latest.json: {e}"))?;
    // PowerShell Set-Content -Encoding utf8 historically wrote a BOM; strip it.
    if body.starts_with('\u{feff}') {
        body = body.trim_start_matches('\u{feff}').to_string();
    }
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err("latest.json was empty.".into());
    }
    if !trimmed.starts_with('{') {
        let preview: String = trimmed.chars().take(80).collect();
        return Err(format!(
            "latest.json is not JSON (got: {preview}…). Check the release asset."
        ));
    }
    let feed: UpdateFeed = serde_json::from_str(trimmed)
        .map_err(|e| format!("Invalid latest.json: {e}"))?;
    validate_feed(&feed)?;
    Ok(feed)
}

pub fn validate_feed(feed: &UpdateFeed) -> Result<(), String> {
    if feed.version.trim().is_empty() {
        return Err("latest.json missing version.".into());
    }
    if feed.sha256.trim().is_empty() {
        return Err("latest.json missing required sha256 — refusing update.".into());
    }
    if feed.sha256.len() != 64 || !feed.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("latest.json sha256 must be a 64-char hex digest.".into());
    }
    if !feed_url_is_allowed(&feed.url) {
        return Err(
            "latest.json zip URL must be HTTPS on github.com / GitHub release CDN hosts.".into(),
        );
    }
    Ok(())
}

fn download_zip(feed: &UpdateFeed) -> Result<PathBuf, String> {
    validate_feed(feed)?;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(10))
        .timeout_read(Duration::from_secs(600))
        .build();
    let resp = agent
        .get(&feed.url)
        .set("User-Agent", "FerricDAW-Updater")
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    let mut reader = resp.into_reader();
    let out = std::env::temp_dir().join(format!(
        "FerricDAW-v{}-update.zip",
        feed.version.replace(['/', '\\', ':'], "_")
    ));
    let mut file = std::fs::File::create(&out).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = std::io::Read::read(&mut reader, &mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n]).map_err(|e| e.to_string())?;
        hasher.update(&buf[..n]);
    }
    drop(file);
    let digest = hex::encode(hasher.finalize());
    if !feed.sha256.eq_ignore_ascii_case(&digest) {
        let _ = std::fs::remove_file(&out);
        return Err(format!(
            "SHA-256 mismatch (got {digest}, expected {}).",
            feed.sha256
        ));
    }
    Ok(out)
}

/// Semver-ish compare: returns true if `remote` is newer than `local`.
pub fn version_is_newer(remote: &str, local: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.trim()
            .trim_start_matches('v')
            .split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .take(3)
            .filter_map(|p| p.parse().ok())
            .collect()
    };
    let mut a = parse(remote);
    let mut b = parse(local);
    while a.len() < 3 {
        a.push(0);
    }
    while b.len() < 3 {
        b.push(0);
    }
    a > b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_compare() {
        assert!(version_is_newer("1.1.2", "1.1.1"));
        assert!(!version_is_newer("1.1.1", "1.1.1"));
        assert!(!version_is_newer("1.0.9", "1.1.0"));
        assert!(version_is_newer("v2.0.0", "1.9.9"));
    }

    #[test]
    fn feed_parses_with_bom() {
        let raw = "\u{feff}{\"version\":\"1.1.4\",\"url\":\"https://github.com/reala1ynex/FerricDAW/releases/download/v1.1.4/x.zip\",\"sha256\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"notes\":\"n\"}";
        let trimmed = raw.trim_start_matches('\u{feff}').trim();
        let feed: UpdateFeed = serde_json::from_str(trimmed).unwrap();
        assert_eq!(feed.version, "1.1.4");
        validate_feed(&feed).expect("github feed with sha should validate");
    }

    #[test]
    fn reject_non_github_url_and_missing_sha() {
        assert!(!feed_url_is_allowed("http://github.com/x"));
        assert!(!feed_url_is_allowed("https://evil.example/x.zip"));
        assert!(feed_url_is_allowed(
            "https://github.com/reala1ynex/FerricDAW/releases/latest/download/latest.json"
        ));
        let bad = UpdateFeed {
            version: "1.0.0".into(),
            url: "https://evil.example/x.zip".into(),
            sha256: String::new(),
            notes: String::new(),
        };
        assert!(validate_feed(&bad).is_err());
        let no_sha = UpdateFeed {
            version: "1.0.0".into(),
            url: "https://github.com/reala1ynex/FerricDAW/releases/download/v1/x.zip".into(),
            sha256: String::new(),
            notes: String::new(),
        };
        assert!(validate_feed(&no_sha).is_err());
    }
}
