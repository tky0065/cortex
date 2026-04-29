use anyhow::{Context, Result, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const DEFAULT_REPO: &str = "tky0065/cortex";
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateCheck {
    pub current: String,
    pub latest: String,
    pub update_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateOutcome {
    pub previous: String,
    pub installed: String,
    pub binary_path: PathBuf,
    pub restart_required: bool,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
}

pub async fn check_latest() -> Result<UpdateCheck> {
    let latest = latest_release_tag().await?;
    Ok(UpdateCheck {
        current: CURRENT_VERSION.to_string(),
        update_available: is_newer_version(CURRENT_VERSION, &latest),
        latest,
    })
}

pub async fn update(version: Option<&str>) -> Result<UpdateOutcome> {
    let target_version = match version {
        Some(v) if !v.trim().is_empty() => normalize_version(v),
        _ => latest_release_tag().await?,
    };

    let repo = repo();
    let target = target_triple()?;
    let archive = archive_name(&target_version, target);
    let base_url = format!("https://github.com/{repo}/releases/download/{target_version}");
    let tmp_dir = create_tmp_dir()?;
    let archive_path = tmp_dir.join(&archive);

    let result = async {
        download_file(&format!("{base_url}/{archive}"), &archive_path).await?;
        let sums = download_text(&format!("{base_url}/SHA256SUMS")).await?;
        verify_checksum(&archive_path, &archive, &sums)?;
        extract_archive(&archive_path, &tmp_dir)?;

        let binary_name = binary_name();
        let extracted = tmp_dir.join(binary_name);
        if !extracted.exists() {
            bail!("release archive did not contain {binary_name}");
        }

        let current_exe = std::env::current_exe().context("failed to locate current executable")?;
        install_binary(&extracted, &current_exe, &tmp_dir)?;

        Ok(UpdateOutcome {
            previous: CURRENT_VERSION.to_string(),
            installed: target_version,
            binary_path: current_exe,
            restart_required: cfg!(windows),
        })
    }
    .await;

    if !cfg!(windows) {
        let _ = fs::remove_dir_all(&tmp_dir);
    }
    result
}

async fn latest_release_tag() -> Result<String> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", repo());
    let release = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "cortex-updater")
        .send()
        .await
        .context("failed to contact GitHub releases API")?
        .error_for_status()
        .context("GitHub releases API returned an error")?
        .json::<GitHubRelease>()
        .await
        .context("failed to parse GitHub release response")?;

    if release.tag_name.trim().is_empty() {
        bail!("latest release has no tag");
    }

    Ok(normalize_version(&release.tag_name))
}

async fn download_file(url: &str, path: &Path) -> Result<()> {
    let bytes = reqwest::Client::new()
        .get(url)
        .header("User-Agent", "cortex-updater")
        .send()
        .await
        .with_context(|| format!("failed to download {url}"))?
        .error_for_status()
        .with_context(|| format!("download failed: {url}"))?
        .bytes()
        .await
        .with_context(|| format!("failed to read download body: {url}"))?;
    fs::write(path, bytes).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

async fn download_text(url: &str) -> Result<String> {
    reqwest::Client::new()
        .get(url)
        .header("User-Agent", "cortex-updater")
        .send()
        .await
        .with_context(|| format!("failed to download {url}"))?
        .error_for_status()
        .with_context(|| format!("download failed: {url}"))?
        .text()
        .await
        .with_context(|| format!("failed to read text body: {url}"))
}

fn install_binary(source: &Path, destination: &Path, _tmp_dir: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        schedule_windows_replace(source, destination, _tmp_dir)
    }

    #[cfg(not(windows))]
    {
        let tmp_dest = destination.with_extension("new");
        fs::copy(source, &tmp_dest).with_context(|| {
            format!(
                "failed to copy {} to {}",
                source.display(),
                tmp_dest.display()
            )
        })?;
        let mut perms = fs::metadata(&tmp_dest)?.permissions();
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(0o755);
        fs::set_permissions(&tmp_dest, perms)?;
        fs::rename(&tmp_dest, destination).with_context(|| {
            format!(
                "failed to replace current executable at {}",
                destination.display()
            )
        })?;
        Ok(())
    }
}

#[cfg(windows)]
fn schedule_windows_replace(source: &Path, destination: &Path, tmp_dir: &Path) -> Result<()> {
    let script = tmp_dir.join("finish-update.ps1");
    let pid = std::process::id();
    let source = source.display().to_string().replace('\'', "''");
    let destination = destination.display().to_string().replace('\'', "''");
    let tmp_dir = tmp_dir.display().to_string().replace('\'', "''");
    let body = format!(
        "$ErrorActionPreference = 'Stop'\n\
         Wait-Process -Id {pid} -ErrorAction SilentlyContinue\n\
         Copy-Item -Force '{source}' '{destination}'\n\
         Remove-Item -Recurse -Force '{tmp_dir}'\n"
    );
    fs::write(&script, body)?;
    Command::new("powershell")
        .args([
            "-ExecutionPolicy",
            "Bypass",
            "-WindowStyle",
            "Hidden",
            "-File",
        ])
        .arg(&script)
        .spawn()
        .context("failed to schedule Windows executable replacement")?;
    Ok(())
}

fn extract_archive(archive_path: &Path, destination: &Path) -> Result<()> {
    if cfg!(windows) {
        let status = Command::new("powershell")
            .args(["-NoProfile", "-Command"])
            .arg(format!(
                "Expand-Archive -Path '{}' -DestinationPath '{}' -Force",
                archive_path.display(),
                destination.display()
            ))
            .status()
            .context("failed to run PowerShell Expand-Archive")?;
        if !status.success() {
            bail!("PowerShell Expand-Archive failed");
        }
        return Ok(());
    }

    let status = Command::new("tar")
        .arg("-xzf")
        .arg(archive_path)
        .arg("-C")
        .arg(destination)
        .status()
        .context("failed to run tar")?;
    if !status.success() {
        bail!("tar extraction failed");
    }
    Ok(())
}

fn verify_checksum(path: &Path, archive: &str, sums: &str) -> Result<()> {
    let expected = checksum_for_archive(archive, sums)
        .ok_or_else(|| anyhow::anyhow!("SHA256SUMS did not contain {archive}"))?;
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual != expected {
        bail!("checksum verification failed for {archive}");
    }
    Ok(())
}

fn checksum_for_archive(archive: &str, sums: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let checksum = parts.next()?;
        let name = parts.next()?;
        (name == archive).then(|| checksum.to_ascii_lowercase())
    })
}

fn target_triple() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        (os, arch) => bail!("unsupported platform for updater: {os}/{arch}"),
    }
}

fn archive_name(version: &str, target: &str) -> String {
    if target.ends_with("windows-msvc") {
        format!("cortex-{version}-{target}.zip")
    } else {
        format!("cortex-{version}-{target}.tar.gz")
    }
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "cortex.exe"
    } else {
        "cortex"
    }
}

fn repo() -> String {
    std::env::var("CORTEX_REPO").unwrap_or_else(|_| DEFAULT_REPO.to_string())
}

fn create_tmp_dir() -> Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("cortex-update-{nanos}"));
    fs::create_dir_all(&path)?;
    Ok(path)
}

fn normalize_version(version: &str) -> String {
    let trimmed = version.trim();
    if trimmed.starts_with('v') {
        trimmed.to_string()
    } else {
        format!("v{trimmed}")
    }
}

fn is_newer_version(current: &str, candidate: &str) -> bool {
    compare_versions(candidate, current).is_gt()
}

fn compare_versions(left: &str, right: &str) -> Ordering {
    let left = version_parts(left);
    let right = version_parts(right);
    let len = left.len().max(right.len());
    for i in 0..len {
        let l = *left.get(i).unwrap_or(&0);
        let r = *right.get(i).unwrap_or(&0);
        match l.cmp(&r) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    Ordering::Equal
}

fn version_parts(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|part| {
            part.chars()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .parse::<u64>()
                .unwrap_or(0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_versions() {
        assert_eq!(normalize_version("0.1.3"), "v0.1.3");
        assert_eq!(normalize_version("v0.1.3"), "v0.1.3");
    }

    #[test]
    fn compares_versions() {
        assert!(is_newer_version("0.1.2", "v0.1.3"));
        assert!(!is_newer_version("0.1.3", "v0.1.3"));
        assert!(!is_newer_version("0.2.0", "v0.1.9"));
    }

    #[test]
    fn builds_archive_names() {
        assert_eq!(
            archive_name("v0.1.3", "x86_64-apple-darwin"),
            "cortex-v0.1.3-x86_64-apple-darwin.tar.gz"
        );
        assert_eq!(
            archive_name("v0.1.3", "x86_64-pc-windows-msvc"),
            "cortex-v0.1.3-x86_64-pc-windows-msvc.zip"
        );
    }

    #[test]
    fn parses_checksum_file() {
        let sums = "abc123  cortex-v0.1.3-x86_64-apple-darwin.tar.gz\n";
        assert_eq!(
            checksum_for_archive("cortex-v0.1.3-x86_64-apple-darwin.tar.gz", sums),
            Some("abc123".to_string())
        );
        assert_eq!(checksum_for_archive("missing.tar.gz", sums), None);
    }
}
