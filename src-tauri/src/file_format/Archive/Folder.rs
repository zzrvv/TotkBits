use super::{validate_entry_path, ArchiveCodec, ArchiveResult};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Default)]
pub struct FolderFile {
    entries: BTreeMap<String, Vec<u8>>,
}

impl FolderFile {
    pub fn from_directory(path: &Path) -> ArchiveResult<Self> {
        if !path.is_dir() {
            return Err(format!("folder does not exist: {}", path.display()));
        }
        let mut entries = BTreeMap::new();
        collect(path, path, &mut entries)?;
        Ok(Self { entries })
    }

    pub fn save_to_directory(&self, path: &Path) -> ArchiveResult<()> {
        fs::create_dir_all(path).map_err(|e| e.to_string())?;
        let mut existing = BTreeMap::new();
        collect(path, path, &mut existing)?;
        for name in existing
            .keys()
            .filter(|name| !self.entries.contains_key(*name))
        {
            fs::remove_file(path.join(name))
                .map_err(|e| format!("failed to remove {name}: {e}"))?;
        }
        for (name, data) in &self.entries {
            validate_entry_path(name)?;
            let destination = path.join(name);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::write(&destination, data)
                .map_err(|e| format!("failed to write {}: {e}", destination.display()))?;
        }
        remove_empty_directories(path, path)?;
        Ok(())
    }
}

impl ArchiveCodec for FolderFile {
    fn from_bytes(_data: &[u8]) -> ArchiveResult<Self> {
        Err("folders must be opened from a directory path".into())
    }

    fn to_bytes(&self) -> ArchiveResult<Vec<u8>> {
        Err("folders cannot be encoded as a single byte stream".into())
    }

    fn entries(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.entries
    }
    fn entries_mut(&mut self) -> &mut BTreeMap<String, Vec<u8>> {
        &mut self.entries
    }
}

fn collect(
    root: &Path,
    current: &Path,
    entries: &mut BTreeMap<String, Vec<u8>>,
) -> ArchiveResult<()> {
    for item in fs::read_dir(current).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let file_type = item.file_type().map_err(|e| e.to_string())?;
        if file_type.is_symlink() {
            continue;
        }
        let path = item.path();
        if file_type.is_dir() {
            collect(root, &path, entries)?;
        } else if file_type.is_file() {
            let name = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            validate_entry_path(&name)?;
            entries.insert(name, fs::read(path).map_err(|e| e.to_string())?);
        }
    }
    Ok(())
}

fn remove_empty_directories(root: &Path, current: &Path) -> ArchiveResult<bool> {
    for item in fs::read_dir(current).map_err(|e| e.to_string())? {
        let path = item.map_err(|e| e.to_string())?.path();
        if path.is_dir() && remove_empty_directories(root, &path)? {
            fs::remove_dir(&path).map_err(|e| e.to_string())?;
        }
    }
    Ok(current != root
        && fs::read_dir(current)
            .map_err(|e| e.to_string())?
            .next()
            .is_none())
}
