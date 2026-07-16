use super::{detect_archive_magic, validate_entry_path, ArchiveCodec, ArchiveMagic, ArchiveResult};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};
use winreg::{
    enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE},
    RegKey,
};

#[derive(Default)]
pub struct RarFile {
    entries: BTreeMap<String, Vec<u8>>,
}

impl RarFile {
    pub fn discover_executable() -> ArchiveResult<PathBuf> {
        for candidate in [
            r"C:\Program Files\WinRAR\rar.exe",
            r"C:\Program Files (x86)\WinRAR\rar.exe",
        ] {
            let path = PathBuf::from(candidate);
            if path.is_file() {
                return Ok(path);
            }
        }
        for (hive, key) in [
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\WinRAR"),
            (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\WinRAR"),
            (HKEY_CURRENT_USER, r"SOFTWARE\WinRAR"),
        ] {
            if let Ok(registry) = RegKey::predef(hive).open_subkey(key) {
                if let Ok(folder) = registry
                    .get_value::<String, _>("exe64")
                    .or_else(|_| registry.get_value("exe32"))
                    .or_else(|_| registry.get_value("Path"))
                {
                    let path = PathBuf::from(folder);
                    let candidate = if path.is_dir() {
                        path.join("rar.exe")
                    } else {
                        path
                    };
                    if candidate.is_file() {
                        return Ok(candidate);
                    }
                }
            }
        }
        Err("WinRAR rar.exe was not found. Install licensed WinRAR or configure it in a standard registry/common path; RAR read/write requires the external licensed runtime.".into())
    }

    fn temporary_dir(label: &str) -> ArchiveResult<PathBuf> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("totkbits-{label}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).map_err(|e| e.to_string())?;
        Ok(path)
    }

    fn collect(
        root: &Path,
        current: &Path,
        entries: &mut BTreeMap<String, Vec<u8>>,
    ) -> ArchiveResult<()> {
        for item in fs::read_dir(current).map_err(|e| e.to_string())? {
            let path = item.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                Self::collect(root, &path, entries)?;
                continue;
            }
            let name = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            validate_entry_path(&name)?;
            entries.insert(name, fs::read(path).map_err(|e| e.to_string())?);
        }
        Ok(())
    }
}

impl ArchiveCodec for RarFile {
    fn from_bytes(data: &[u8]) -> ArchiveResult<Self> {
        if detect_archive_magic(data) != Some(ArchiveMagic::Rar) {
            return Err("RAR magic bytes do not match".into());
        }
        let rar = Self::discover_executable()?;
        let workspace = Self::temporary_dir("rar-read")?;
        let archive_path = workspace.join("archive.rar");
        let output = workspace.join("contents");
        fs::create_dir_all(&output).map_err(|e| e.to_string())?;
        fs::write(&archive_path, data).map_err(|e| e.to_string())?;
        let listing = Command::new(&rar)
            .args(["lb", "-p-", archive_path.to_string_lossy().as_ref()])
            .output()
            .map_err(|e| e.to_string())?;
        if !listing.status.success() {
            let _ = fs::remove_dir_all(&workspace);
            return Err("RAR listing failed; encrypted, corrupt, and unsupported RAR archives cannot be opened".into());
        }
        for line in String::from_utf8_lossy(&listing.stdout).lines() {
            let name = line.trim().replace('\\', "/");
            if !name.is_empty() {
                if let Err(error) = validate_entry_path(name.trim_end_matches('/')) {
                    let _ = fs::remove_dir_all(&workspace);
                    return Err(error);
                }
            }
        }
        let status = Command::new(rar)
            .args([
                "x",
                "-idq",
                "-p-",
                archive_path.to_string_lossy().as_ref(),
                output.to_string_lossy().as_ref(),
            ])
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            let _ = fs::remove_dir_all(&workspace);
            return Err("RAR extraction failed; encrypted, corrupt, and unsupported RAR archives cannot be opened".into());
        }
        let mut entries = BTreeMap::new();
        let result = Self::collect(&output, &output, &mut entries).map(|_| Self { entries });
        let _ = fs::remove_dir_all(workspace);
        result
    }
    fn to_bytes(&self) -> ArchiveResult<Vec<u8>> {
        let rar = Self::discover_executable()?;
        let workspace = Self::temporary_dir("rar-write")?;
        let source = workspace.join("contents");
        fs::create_dir_all(&source).map_err(|e| e.to_string())?;
        for (name, bytes) in &self.entries {
            validate_entry_path(name)?;
            let destination = source.join(name);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::write(destination, bytes).map_err(|e| e.to_string())?;
        }
        let archive_path = workspace.join("archive.rar");
        let source_arg = format!("{}\\*", source.display());
        let status = Command::new(rar)
            .current_dir(&source)
            .args([
                "a",
                "-idq",
                "-ep1",
                "-r",
                "-p-",
                archive_path.to_string_lossy().as_ref(),
                &source_arg,
            ])
            .status()
            .map_err(|e| e.to_string())?;
        let result = if status.success() {
            fs::read(&archive_path).map_err(|e| e.to_string())
        } else {
            Err("RAR rebuild failed using rar.exe".into())
        };
        let _ = fs::remove_dir_all(workspace);
        result
    }
    fn entries(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.entries
    }
    fn entries_mut(&mut self) -> &mut BTreeMap<String, Vec<u8>> {
        &mut self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rar_roundtrip_when_runtime_is_installed() {
        if RarFile::discover_executable().is_err() {
            eprintln!("skipping RAR roundtrip: rar.exe not installed");
            return;
        }
        let mut archive = RarFile::default();
        archive
            .entries
            .insert("folder/a.txt".into(), b"rar".to_vec());
        let bytes = archive.to_bytes().unwrap();
        let reopened = RarFile::from_bytes(&bytes).unwrap();
        assert_eq!(reopened.get("folder/a.txt"), Some(&b"rar"[..]));
    }
}
