//! In-app update, the parts before the restart (SPEC §15): ask GitHub for the latest release, compare its version,
//! download the release zip and `SHA256SUMS.txt`, verify and unpack them into a temp folder. Never on the timing
//! path: callers run these in their own tasks. The file swap itself is `update_apply` (the helper).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Where releases are asked for. `--update-source` replaces it for tests (SPEC §15).
pub const DEFAULT_SOURCE: &str = "https://api.github.com";
static SOURCE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Sets the source for this process (`--update-source`); the first call wins.
pub fn set_source(url: Option<&str>) {
    if let Some(url) = url {
        let _ = SOURCE.set(url.trim_end_matches('/').to_owned());
    }
}

/// [`DEFAULT_SOURCE`], or what `--update-source` set.
pub fn source() -> &'static str {
    SOURCE.get().map_or(DEFAULT_SOURCE, String::as_str)
}

/// `owner/name` of the repository whose releases are offered.
pub const REPO: &str = "ghostrider/clip-sequencer";
/// Release page shown when something fails.
pub const RELEASES_PAGE: &str = "https://github.com/ghostrider/clip-sequencer/releases";
/// The folder at the top of the release zip (`scripts/release.ps1`).
pub const ZIP_FOLDER: &str = "clip-sequencer";

/// A release newer than the running version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Release {
    /// Without the `v`: `0.5.1`.
    pub version: String,
    /// The release body (Markdown), shown as plain text.
    pub notes: String,
    /// Its page on GitHub.
    pub page: String,
    pub zip_name: String,
    pub zip_url: String,
    pub zip_size: u64,
    pub sums_url: String,
}

/// What a check found (sent to the UI as is).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CheckResult {
    UpToDate { current: String },
    Available { current: String, release: Release },
    Error { current: String, message: String },
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// The release zip of a version (`scripts/release.ps1`).
pub fn zip_name(version: &str) -> String {
    format!("clip-sequencer-{version}-windows-x64.zip")
}

fn parse_version(s: &str) -> Option<semver::Version> {
    semver::Version::parse(s.trim().trim_start_matches('v')).ok()
}

/// Reads GitHub's "latest release" answer: `Ok(None)` when it is not newer than `current` (or a draft or
/// pre-release, which that endpoint never returns anyway); `Err` when it is newer but unusable.
pub fn parse_release(json: &str, current: &str) -> Result<Option<Release>, String> {
    let r: GhRelease =
        serde_json::from_str(json).map_err(|e| format!("unexpected answer from GitHub: {e}"))?;
    let current = parse_version(current).ok_or_else(|| format!("bad own version {current:?}"))?;
    let Some(version) = parse_version(&r.tag_name) else {
        return Err(format!(
            "the latest release has no version tag ({:?})",
            r.tag_name
        ));
    };
    if r.draft || r.prerelease || !version.pre.is_empty() || version <= current {
        return Ok(None);
    }
    let version = version.to_string();
    let want = zip_name(&version);
    let zip = r
        .assets
        .iter()
        .find(|a| a.name == want)
        .ok_or_else(|| format!("release {version} has no Windows zip ({want})"))?;
    let sums = r
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS.txt")
        .ok_or_else(|| format!("release {version} has no SHA256SUMS.txt"))?;
    Ok(Some(Release {
        notes: r.body.unwrap_or_default().trim().to_owned(),
        page: r.html_url.unwrap_or_else(|| RELEASES_PAGE.to_owned()),
        zip_name: zip.name.clone(),
        zip_url: zip.browser_download_url.clone(),
        zip_size: zip.size,
        sums_url: sums.browser_download_url.clone(),
        version,
    }))
}

/// An HTTP client for update requests: our user agent, a timeout per request.
pub fn client(current: &str) -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(format!("clip-sequencer/{current}"))
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("static client settings")
}

/// Asks `source` (`https://api.github.com`, or a test server) for the latest release.
pub async fn check(client: &reqwest::Client, source: &str, current: &str) -> CheckResult {
    let url = format!(
        "{}/repos/{REPO}/releases/latest",
        source.trim_end_matches('/')
    );
    let error = |message: String| CheckResult::Error {
        current: current.to_owned(),
        message,
    };
    let resp = match client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return error(format!("could not reach {source}: {}", short(&e))),
    };
    if !resp.status().is_success() {
        return error(format!("{source} answered {}", resp.status()));
    }
    let text = match resp.text().await {
        Ok(t) => t,
        Err(e) => return error(format!("reading the answer failed: {}", short(&e))),
    };
    match parse_release(&text, current) {
        Ok(None) => CheckResult::UpToDate {
            current: current.to_owned(),
        },
        Ok(Some(release)) => CheckResult::Available {
            current: current.to_owned(),
            release,
        },
        Err(e) => error(e),
    }
}

/// The innermost cause of a reqwest error, which says more than its outer "error sending request".
fn short(e: &reqwest::Error) -> String {
    let mut msg = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(s) = src {
        msg = s.to_string();
        src = s.source();
    }
    msg
}

/// `SHA256SUMS.txt`: `<hex>  <path>` per line (path relative to the release, `/` separated) → path → lowercase hex.
pub fn parse_sums(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|l| {
            let (hash, path) = l.trim().split_once(char::is_whitespace)?;
            let path = path.trim().trim_start_matches('*');
            (hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) && !path.is_empty())
                .then(|| (path.to_owned(), hash.to_ascii_lowercase()))
        })
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 of a file, as lowercase hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

/// Progress of [`download`]: bytes done and the total (0 when unknown).
pub type Progress = dyn Fn(u64, u64) + Send + Sync;

/// The temp folder a version is staged in (SPEC §15).
pub fn stage_dir(version: &str) -> PathBuf {
    std::env::temp_dir().join(format!("clip-sequencer-update-{version}"))
}

/// Downloads the release into `dir` (emptied first), verifies the zip against `SHA256SUMS.txt`, unpacks it and
/// verifies every unpacked file that has a line. Returns the unpacked `clip-sequencer` folder.
pub async fn download(
    client: &reqwest::Client,
    release: &Release,
    dir: &Path,
    progress: &Progress,
) -> Result<PathBuf, String> {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let sums = client
        .get(&release.sums_url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("downloading SHA256SUMS.txt failed: {}", short(&e)))?
        .text()
        .await
        .map_err(|e| format!("downloading SHA256SUMS.txt failed: {}", short(&e)))?;
    let sums = parse_sums(&sums);
    let zip_path = dir.join(&release.zip_name);
    let mut resp = client
        .get(&release.zip_url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("downloading the zip failed: {}", short(&e)))?;
    let total = resp.content_length().unwrap_or(release.zip_size);
    let mut file = std::fs::File::create(&zip_path)
        .map_err(|e| format!("cannot write {}: {e}", zip_path.display()))?;
    let mut done = 0u64;
    progress(0, total);
    loop {
        let chunk = resp
            .chunk()
            .await
            .map_err(|e| format!("downloading the zip failed: {}", short(&e)))?;
        let Some(chunk) = chunk else { break };
        std::io::Write::write_all(&mut file, &chunk)
            .map_err(|e| format!("cannot write {}: {e}", zip_path.display()))?;
        done += chunk.len() as u64;
        progress(done, total);
    }
    drop(file);
    let dir = dir.to_owned();
    let zip_name = release.zip_name.clone();
    tokio::task::spawn_blocking(move || verify_and_unpack(&dir, &zip_name, &sums))
        .await
        .map_err(|e| format!("verifying failed: {e}"))?
}

/// The zip in `dir` must match its line; it is unpacked into `dir`, then every unpacked file with a line must match.
pub fn verify_and_unpack(
    dir: &Path,
    zip_name: &str,
    sums: &BTreeMap<String, String>,
) -> Result<PathBuf, String> {
    let zip_path = dir.join(zip_name);
    let want = sums
        .get(zip_name)
        .ok_or_else(|| format!("SHA256SUMS.txt has no line for {zip_name}"))?;
    let got = sha256_file(&zip_path).map_err(|e| format!("cannot read the zip: {e}"))?;
    if &got != want {
        return Err(format!(
            "{zip_name} does not match SHA256SUMS.txt (damaged download?)"
        ));
    }
    let file = std::fs::File::open(&zip_path).map_err(|e| format!("cannot read the zip: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("not a valid zip: {e}"))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("not a valid zip: {e}"))?;
        // `enclosed_name` refuses absolute paths and `..` (no writing outside `dir`).
        let Some(rel) = entry.enclosed_name() else {
            return Err(format!("unsafe path in the zip: {:?}", entry.name()));
        };
        let out = dir.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| format!("unpacking failed: {e}"))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("unpacking failed: {e}"))?;
        }
        let mut f = std::fs::File::create(&out).map_err(|e| format!("unpacking failed: {e}"))?;
        std::io::copy(&mut entry, &mut f).map_err(|e| format!("unpacking failed: {e}"))?;
    }
    for (path, want) in sums {
        if path == zip_name || !path.starts_with(&format!("{ZIP_FOLDER}/")) {
            continue;
        }
        let file = dir.join(path);
        let got = sha256_file(&file).map_err(|_| format!("{path} is missing in the zip"))?;
        if &got != want {
            return Err(format!("{path} does not match SHA256SUMS.txt"));
        }
    }
    let folder = dir.join(ZIP_FOLDER);
    if !folder.join("seq-app-cli.exe").is_file() {
        return Err(format!("the zip has no {ZIP_FOLDER}/seq-app-cli.exe"));
    }
    Ok(folder)
}

/// The program folder can be updated in place: a file can be created and removed in it (SPEC §15).
pub fn folder_writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".update-probe-{}", std::process::id()));
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release_json(tag: &str, assets: &[&str]) -> String {
        let assets: Vec<_> = assets
            .iter()
            .map(|n| {
                serde_json::json!({"name": n, "browser_download_url": format!("http://x/{n}"), "size": 10})
            })
            .collect();
        serde_json::json!({
            "tag_name": tag, "body": " Notes\n", "html_url": "http://x/page",
            "draft": false, "prerelease": false, "assets": assets
        })
        .to_string()
    }

    #[test]
    fn newer_versions_only() {
        let full = |tag: &str, v: &str| {
            release_json(tag, &[&zip_name(v), "SHA256SUMS.txt", "SeqView-x.pdb"])
        };
        let r = parse_release(&full("v0.5.1", "0.5.1"), "0.5.0")
            .unwrap()
            .unwrap();
        assert_eq!(r.version, "0.5.1");
        assert_eq!(r.notes, "Notes");
        assert_eq!(r.zip_url, "http://x/clip-sequencer-0.5.1-windows-x64.zip");
        assert_eq!(r.sums_url, "http://x/SHA256SUMS.txt");
        assert_eq!(r.page, "http://x/page");
        // Semver, not text: 0.10 is newer than 0.9.
        assert!(parse_release(&full("v0.10.0", "0.10.0"), "0.9.3")
            .unwrap()
            .is_some());
        for (tag, cur) in [
            ("v0.5.0", "0.5.0"),
            ("v0.4.0", "0.5.0"),
            ("v0.5.1-rc.1", "0.5.0"),
        ] {
            assert_eq!(
                parse_release(&full(tag, tag.trim_start_matches('v')), cur),
                Ok(None),
                "{tag} vs {cur}"
            );
        }
    }

    #[test]
    fn drafts_prereleases_and_broken_releases() {
        let mut v: serde_json::Value = serde_json::from_str(&release_json(
            "v9.0.0",
            &[&zip_name("9.0.0"), "SHA256SUMS.txt"],
        ))
        .unwrap();
        v["prerelease"] = true.into();
        assert_eq!(parse_release(&v.to_string(), "0.5.0"), Ok(None));
        v["prerelease"] = false.into();
        v["draft"] = true.into();
        assert_eq!(parse_release(&v.to_string(), "0.5.0"), Ok(None));

        let no_zip = release_json("v9.0.0", &["SHA256SUMS.txt"]);
        assert!(parse_release(&no_zip, "0.5.0")
            .unwrap_err()
            .contains("no Windows zip"));
        let no_sums = release_json("v9.0.0", &[&zip_name("9.0.0")]);
        assert!(parse_release(&no_sums, "0.5.0")
            .unwrap_err()
            .contains("SHA256SUMS"));
        assert!(parse_release(&release_json("nightly", &[]), "0.5.0").is_err());
        assert!(parse_release("<html>", "0.5.0").is_err());
    }

    #[test]
    fn sums_lines() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let sums = parse_sums(&format!(
            "{a}  clip-sequencer-0.5.1-windows-x64.zip\n{b} *clip-sequencer/seq-app.exe\nnot a line\n\n"
        ));
        assert_eq!(sums.len(), 2);
        assert_eq!(sums["clip-sequencer-0.5.1-windows-x64.zip"], a);
        assert_eq!(sums["clip-sequencer/seq-app.exe"], "b".repeat(64));
    }

    /// A release zip like `release.ps1` makes, and its `SHA256SUMS.txt` lines.
    pub(crate) fn make_release(dir: &Path, version: &str) -> BTreeMap<String, String> {
        use std::io::Write;
        let files = [
            ("clip-sequencer/seq-app.exe", b"new gui".as_slice()),
            ("clip-sequencer/seq-app-cli.exe", b"new cli".as_slice()),
            ("clip-sequencer/ffgl/SeqView.dll", b"new dll".as_slice()),
            ("clip-sequencer/README.md", b"readme".as_slice()),
        ];
        let name = zip_name(version);
        let mut zw = zip::ZipWriter::new(std::fs::File::create(dir.join(&name)).unwrap());
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (path, data) in files {
            zw.start_file(path, opts).unwrap();
            zw.write_all(data).unwrap();
        }
        zw.finish().unwrap();
        let mut sums = BTreeMap::new();
        sums.insert(name.clone(), sha256_file(&dir.join(&name)).unwrap());
        for (path, data) in files.iter().take(3) {
            sums.insert((*path).to_owned(), hex(&Sha256::digest(data)));
        }
        sums
    }

    #[test]
    fn verify_and_unpack_a_release() {
        let tmp = tempfile::tempdir().unwrap();
        let sums = make_release(tmp.path(), "0.5.1");
        let folder = verify_and_unpack(tmp.path(), &zip_name("0.5.1"), &sums).unwrap();
        assert_eq!(
            std::fs::read(folder.join("seq-app-cli.exe")).unwrap(),
            b"new cli"
        );
        assert!(folder.join("ffgl/SeqView.dll").is_file());

        // A wrong zip hash, a wrong file hash, a missing line: refused.
        let mut bad = sums.clone();
        bad.insert(zip_name("0.5.1"), "0".repeat(64));
        assert!(verify_and_unpack(tmp.path(), &zip_name("0.5.1"), &bad)
            .unwrap_err()
            .contains("does not match"));
        let mut bad = sums.clone();
        bad.insert("clip-sequencer/seq-app.exe".into(), "0".repeat(64));
        assert!(verify_and_unpack(tmp.path(), &zip_name("0.5.1"), &bad)
            .unwrap_err()
            .contains("seq-app.exe does not match"));
        let mut bad = sums.clone();
        bad.remove(&zip_name("0.5.1"));
        assert!(verify_and_unpack(tmp.path(), &zip_name("0.5.1"), &bad)
            .unwrap_err()
            .contains("no line"));
    }

    #[test]
    fn unsafe_zip_paths_are_refused() {
        use std::io::Write;
        let tmp = tempfile::tempdir().unwrap();
        let name = zip_name("0.5.1");
        let mut zw = zip::ZipWriter::new(std::fs::File::create(tmp.path().join(&name)).unwrap());
        zw.start_file("../evil.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zw.write_all(b"x").unwrap();
        zw.finish().unwrap();
        let mut sums = BTreeMap::new();
        sums.insert(name.clone(), sha256_file(&tmp.path().join(&name)).unwrap());
        assert!(verify_and_unpack(tmp.path(), &name, &sums)
            .unwrap_err()
            .contains("unsafe path"));
        assert!(!tmp.path().parent().unwrap().join("evil.txt").exists());
    }

    #[test]
    fn writable_folder_probe() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(folder_writable(tmp.path()));
        assert!(!folder_writable(&tmp.path().join("missing")));
        assert_eq!(
            std::fs::read_dir(tmp.path()).unwrap().count(),
            0,
            "probe removed"
        );
    }
}
