//! Shared watch-channel validation and stop-only path checks.
//! The configured home is trusted and may be a symlink.
//! Active mutation by a malicious same-UID/privileged local process is outside
//! this boundary; these checks do not promise protection against that race.

use anyhow::{bail, Context, Result};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use super::types::WatchConfig;

pub fn validate_id(id: &str) -> Result<()> {
    if id.len() != 9
        || !id.starts_with("ws_")
        || !id.as_bytes()[3..]
            .iter()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        bail!("invalid session ID: expected ws_ followed by six lowercase hexadecimal digits");
    }
    Ok(())
}

pub fn validate_channel(value: &str) -> Result<()> {
    let mut components = Path::new(value).components();
    if value.is_empty()
        || value.contains(['/', '\\', '\0', ':'])
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
        || value.ends_with(['.', ' '])
    {
        bail!(
            "invalid channel {:?}: expected a safe session path component",
            value
        );
    }
    Ok(())
}

fn is_link(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Includes junctions and other reparse points, not just symlinks.
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}

fn require_directory(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path).with_context(|| {
        format!(
            "session directory '{}' not found or inaccessible",
            path.display()
        )
    })?;
    if is_link(&meta) || !meta.is_dir() {
        bail!(
            "unsafe session directory '{}': expected a real directory",
            path.display()
        );
    }
    Ok(())
}

pub struct StopRoot {
    dir: PathBuf,
}

pub struct StopSession {
    pub dir: PathBuf,
    pub config: WatchConfig,
    pub pid: Option<u32>,
}

impl StopRoot {
    pub fn open() -> Result<Option<Self>> {
        let dir = super::store::watch_root()?;
        match fs::symlink_metadata(&dir) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e).context("unsafe or inaccessible watch root"),
            Ok(_) => {}
        }
        require_directory(&dir).context("unsafe watch root")?;
        Ok(Some(Self { dir }))
    }

    /// Enumerate names only, so batch stop never reads an unvalidated session.
    pub fn ids(&self) -> Result<(Vec<String>, Vec<String>)> {
        let mut ids = Vec::new();
        let mut unsafe_names = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let name = entry?.file_name();
            let display = name.to_string_lossy();
            // Retain the existing batch entry selection; legacy IDs are errors.
            if !display.starts_with("ws_") && !display.starts_with("watch_") {
                continue;
            }
            match name.into_string() {
                Ok(name) => ids.push(name),
                Err(name) => unsafe_names.push(name.to_string_lossy().into_owned()),
            }
        }
        ids.sort();
        Ok((ids, unsafe_names))
    }

    pub fn session(&self, id: &str) -> Result<StopSession> {
        validate_id(id)?;
        let dir = self.dir.join(id);
        require_directory(&dir)?;
        // Check every existing file stop can read or overwrite, including the
        // store's predictable temporary names, before config/PID reads or flush.
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let name = entry.file_name();
            // On case-insensitive filesystems CONFIG.JSON and PID can be the
            // files later opened through lowercase names. Check both spellings.
            let name = name.to_string_lossy().to_ascii_lowercase();
            if matches!(
                name.as_str(),
                "config.json" | "pid" | "status" | ".status.tmp"
            ) || name.starts_with("cursor.")
                || name.starts_with(".cursor.")
                || name.starts_with("events.")
            {
                let meta = fs::symlink_metadata(entry.path())?;
                if is_link(&meta) || !meta.is_file() {
                    bail!("unsafe session file '{}'", name);
                }
            }
        }
        let config: WatchConfig = serde_json::from_slice(
            &fs::read(dir.join("config.json")).context("session config missing or inaccessible")?,
        )
        .context("invalid session config")?;
        if config.channels.is_empty() {
            bail!("session config has no channels");
        }
        for channel in &config.channels {
            validate_channel(channel).context("unsafe channel in session config")?;
        }
        let pid = match fs::read_to_string(dir.join("pid")) {
            Ok(raw) => {
                let pid: u32 = raw.trim().parse().context("invalid session PID")?;
                if pid == 0 || pid > i32::MAX as u32 {
                    bail!("invalid session PID; refusing to send signal");
                }
                Some(pid)
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e).context("cannot read session PID"),
        };
        Ok(StopSession { dir, config, pid })
    }
}

impl StopSession {
    pub fn remove(self) -> Result<()> {
        require_directory(&self.dir)?;
        // std::fs::remove_dir_all removes child links themselves without
        // following them. The session entry itself was required to be real.
        fs::remove_dir_all(&self.dir).context("failed to clean up session")
    }
}

#[cfg(test)]
mod channel_tests {
    use super::*;

    #[test]
    fn rejects_unsafe_channel_components() {
        for channel in [
            "",
            ".",
            "..",
            "foo/bar",
            "foo\\bar",
            "/absolute",
            "C:channel",
            "foo\0bar",
            "trailing.",
            "trailing ",
        ] {
            assert!(validate_channel(channel).is_err(), "{channel:?}");
        }
    }

    #[test]
    fn accepts_registered_default_and_safe_unknown_channels() {
        for channel in super::super::types::ALL_CHANNELS {
            validate_channel(channel.name).unwrap();
        }
        for channel in super::super::types::DEFAULT_CHANNELS {
            validate_channel(channel).unwrap();
        }
        for channel in [
            "dex-token-candle1m",
            "dex-token-candle1H",
            "unknown-safe-channel",
        ] {
            validate_channel(channel).unwrap();
        }
    }
}
