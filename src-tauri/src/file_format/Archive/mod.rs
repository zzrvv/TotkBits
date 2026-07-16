use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
};

pub mod Folder;
pub mod Rar;
pub mod SevenZip;
pub mod Zip;

pub type ArchiveResult<T> = Result<T, String>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ArchiveMagic {
    Zip,
    SevenZip,
    Rar,
}

pub fn detect_archive_magic(data: &[u8]) -> Option<ArchiveMagic> {
    if data.starts_with(b"PK\x03\x04")
        || data.starts_with(b"PK\x05\x06")
        || data.starts_with(b"PK\x07\x08")
    {
        Some(ArchiveMagic::Zip)
    } else if data.starts_with(b"7z\xBC\xAF\x27\x1C") {
        Some(ArchiveMagic::SevenZip)
    } else if data.starts_with(b"Rar!\x1A\x07\x00") || data.starts_with(b"Rar!\x1A\x07\x01\x00") {
        Some(ArchiveMagic::Rar)
    } else {
        None
    }
}

pub enum RootArchive {
    Zip(Zip::ZipFile),
    SevenZip(SevenZip::SevenZipFile),
    Rar(Rar::RarFile),
    Folder(Folder::FolderFile),
}

pub struct ArchiveDocument {
    pub archive: RootArchive,
    pub path: String,
    pub added: BTreeSet<String>,
    pub modified: BTreeSet<String>,
}

impl ArchiveDocument {
    pub fn open_folder(path: &Path) -> ArchiveResult<Self> {
        Ok(Self {
            archive: RootArchive::Folder(Folder::FolderFile::from_directory(path)?),
            path: path.to_string_lossy().replace('\\', "/"),
            added: BTreeSet::new(),
            modified: BTreeSet::new(),
        })
    }
    pub fn open(path: &Path) -> ArchiveResult<Option<Self>> {
        let bytes =
            fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        let archive = match detect_archive_magic(&bytes) {
            Some(ArchiveMagic::Zip) => RootArchive::Zip(Zip::ZipFile::from_bytes(&bytes)?),
            Some(ArchiveMagic::SevenZip) => {
                RootArchive::SevenZip(SevenZip::SevenZipFile::from_bytes(&bytes)?)
            }
            Some(ArchiveMagic::Rar) => RootArchive::Rar(Rar::RarFile::from_bytes(&bytes)?),
            None => return Ok(None),
        };
        Ok(Some(Self {
            archive,
            path: path.to_string_lossy().replace('\\', "/"),
            added: BTreeSet::new(),
            modified: BTreeSet::new(),
        }))
    }
    pub fn paths(&self) -> Vec<String> {
        self.archive.entries().keys().cloned().collect()
    }
    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.archive.get(path)
    }
    pub fn set(&mut self, path: &str, bytes: Vec<u8>) -> ArchiveResult<()> {
        validate_entry_path(path)?;
        if self.archive.entries().contains_key(path) {
            self.modified.insert(path.into());
        } else {
            self.added.insert(path.into());
        }
        self.archive.entries_mut().insert(path.into(), bytes);
        Ok(())
    }
    pub fn remove_prefix(&mut self, path: &str) -> ArchiveResult<usize> {
        let prefix = format!("{}/", path.trim_end_matches('/'));
        let keys: Vec<_> = self
            .paths()
            .into_iter()
            .filter(|p| p == path || p.starts_with(&prefix))
            .collect();
        if keys.is_empty() {
            return Err(format!("archive entry not found: {path}"));
        }
        for key in &keys {
            self.archive.entries_mut().remove(key);
            self.added.remove(key);
            self.modified.remove(key);
        }
        Ok(keys.len())
    }
    pub fn rename_prefix(&mut self, from: &str, to: &str) -> ArchiveResult<usize> {
        validate_entry_path(to)?;
        let prefix = format!("{}/", from.trim_end_matches('/'));
        let keys: Vec<_> = self
            .paths()
            .into_iter()
            .filter(|p| p == from || p.starts_with(&prefix))
            .collect();
        if keys.is_empty() {
            return Err(format!("archive entry not found: {from}"));
        }
        let replacements: Vec<_> = keys
            .iter()
            .map(|old| {
                (
                    old.clone(),
                    if old == from {
                        to.into()
                    } else {
                        format!("{}{}", to.trim_end_matches('/'), &old[from.len()..])
                    },
                )
            })
            .collect();
        for (_, new) in &replacements {
            validate_entry_path(new)?;
            if self.archive.entries().contains_key(new) && !keys.contains(new) {
                return Err(format!("archive entry already exists: {new}"));
            }
        }
        for (old, new) in replacements {
            let bytes = self.archive.entries_mut().remove(&old).unwrap();
            self.archive.entries_mut().insert(new.clone(), bytes);
            self.added.remove(&old);
            self.modified.remove(&old);
            self.modified.insert(new);
        }
        Ok(keys.len())
    }
    pub fn to_bytes(&self) -> ArchiveResult<Vec<u8>> {
        self.archive.to_bytes()
    }
    pub fn save_atomic(&mut self, destination: &Path) -> ArchiveResult<()> {
        if let RootArchive::Folder(folder) = &self.archive {
            folder.save_to_directory(destination)?;
            self.path = destination.to_string_lossy().replace('\\', "/");
            self.added.clear();
            self.modified.clear();
            return Ok(());
        }
        let bytes = self.to_bytes()?;
        let parent = destination.parent().unwrap_or_else(|| Path::new("."));
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let tmp = parent.join(format!(
            ".{}.totkbits.tmp",
            destination
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("archive")
        ));
        fs::write(&tmp, bytes).map_err(|e| format!("failed to write temporary archive: {e}"))?;
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            use windows::{
                core::PCWSTR,
                Win32::Storage::FileSystem::{
                    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
                },
            };
            let source: Vec<u16> = tmp.as_os_str().encode_wide().chain(Some(0)).collect();
            let target: Vec<u16> = destination
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect();
            unsafe {
                MoveFileExW(
                    PCWSTR(source.as_ptr()),
                    PCWSTR(target.as_ptr()),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            }
            .map_err(|e| format!("failed to atomically commit archive: {e}"))?;
        }
        #[cfg(not(windows))]
        fs::rename(&tmp, destination).map_err(|e| format!("failed to commit archive: {e}"))?;
        self.path = destination.to_string_lossy().replace('\\', "/");
        self.added.clear();
        self.modified.clear();
        Ok(())
    }
}

impl RootArchive {
    pub(crate) fn entries(&self) -> &BTreeMap<String, Vec<u8>> {
        match self {
            Self::Zip(v) => v.entries(),
            Self::SevenZip(v) => v.entries(),
            Self::Rar(v) => v.entries(),
            Self::Folder(v) => v.entries(),
        }
    }
    pub(crate) fn entries_mut(&mut self) -> &mut BTreeMap<String, Vec<u8>> {
        match self {
            Self::Zip(v) => v.entries_mut(),
            Self::SevenZip(v) => v.entries_mut(),
            Self::Rar(v) => v.entries_mut(),
            Self::Folder(v) => v.entries_mut(),
        }
    }
    pub(crate) fn get(&self, path: &str) -> Option<&[u8]> {
        self.entries().get(path).map(Vec::as_slice)
    }
    pub(crate) fn to_bytes(&self) -> ArchiveResult<Vec<u8>> {
        match self {
            Self::Zip(v) => v.to_bytes(),
            Self::SevenZip(v) => v.to_bytes(),
            Self::Rar(v) => v.to_bytes(),
            Self::Folder(v) => v.to_bytes(),
        }
    }
}

pub trait ArchiveCodec: Sized {
    fn from_bytes(data: &[u8]) -> ArchiveResult<Self>;
    fn to_bytes(&self) -> ArchiveResult<Vec<u8>>;
    fn entries(&self) -> &BTreeMap<String, Vec<u8>>;
    fn entries_mut(&mut self) -> &mut BTreeMap<String, Vec<u8>>;

    fn paths(&self) -> Vec<String> {
        self.entries().keys().cloned().collect()
    }
    fn get(&self, path: &str) -> Option<&[u8]> {
        self.entries().get(path).map(Vec::as_slice)
    }
    fn replace(&mut self, path: &str, data: Vec<u8>) -> ArchiveResult<()> {
        validate_entry_path(path)?;
        if !self.entries().contains_key(path) {
            return Err(format!("archive entry not found: {path}"));
        }
        self.entries_mut().insert(path.to_string(), data);
        Ok(())
    }
    fn remove(&mut self, path: &str) -> ArchiveResult<()> {
        self.entries_mut()
            .remove(path)
            .map(|_| ())
            .ok_or_else(|| format!("archive entry not found: {path}"))
    }
    fn rename(&mut self, from: &str, to: &str) -> ArchiveResult<()> {
        validate_entry_path(to)?;
        if self.entries().contains_key(to) {
            return Err(format!("archive entry already exists: {to}"));
        }
        let data = self
            .entries_mut()
            .remove(from)
            .ok_or_else(|| format!("archive entry not found: {from}"))?;
        self.entries_mut().insert(to.to_string(), data);
        Ok(())
    }
}

pub fn validate_entry_path(path: &str) -> ArchiveResult<()> {
    if path.is_empty() || path.contains('\\') {
        return Err(format!("unsafe archive entry path: {path}"));
    }
    let candidate = Path::new(path);
    if candidate.is_absolute()
        || candidate.components().any(|part| {
            matches!(
                part,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!("unsafe archive entry path: {path}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_detection_uses_magic_only() {
        assert_eq!(
            detect_archive_magic(b"PK\x03\x04payload"),
            Some(ArchiveMagic::Zip)
        );
        assert_eq!(
            detect_archive_magic(b"7z\xBC\xAF\x27\x1Cpayload"),
            Some(ArchiveMagic::SevenZip)
        );
        assert_eq!(
            detect_archive_magic(b"Rar!\x1A\x07\x01\x00payload"),
            Some(ArchiveMagic::Rar)
        );
        assert_eq!(detect_archive_magic(b"example.zip"), None);
    }

    #[test]
    fn rejects_zip_slip_paths() {
        assert!(validate_entry_path("../evil").is_err());
        assert!(validate_entry_path("/evil").is_err());
        assert!(validate_entry_path("safe/file.txt").is_ok());
    }

    fn document_roundtrip(extension: &str, archive: RootArchive) {
        let path = std::env::temp_dir().join(format!(
            "totkbits-archive-document-{}-{}.{}",
            std::process::id(),
            extension,
            extension
        ));
        let mut document = ArchiveDocument {
            archive,
            path: path.to_string_lossy().into_owned(),
            added: BTreeSet::new(),
            modified: BTreeSet::new(),
        };
        document
            .set("folder/original.txt", b"before".to_vec())
            .unwrap();
        document.save_atomic(&path).unwrap();
        let mut reopened = ArchiveDocument::open(&path).unwrap().unwrap();
        assert_eq!(
            reopened.get("folder/original.txt"),
            Some(b"before".as_slice())
        );
        reopened
            .set("folder/original.txt", b"after".to_vec())
            .unwrap();
        reopened.save_atomic(&path).unwrap();
        let final_document = ArchiveDocument::open(&path).unwrap().unwrap();
        assert_eq!(
            final_document.get("folder/original.txt"),
            Some(b"after".as_slice())
        );
        let _ = fs::remove_file(path);
    }

    #[test]
    fn zip_document_open_edit_save() {
        document_roundtrip("zip", RootArchive::Zip(Zip::ZipFile::default()));
    }

    #[test]
    fn seven_zip_document_open_edit_save() {
        document_roundtrip(
            "7z",
            RootArchive::SevenZip(SevenZip::SevenZipFile::default()),
        );
    }

    #[test]
    fn mixed_zip_to_seven_zip_roundtrip() {
        let mut inner = SevenZip::SevenZipFile::default();
        inner
            .entries_mut()
            .insert("deep/value.txt".into(), b"before".to_vec());
        let mut outer = Zip::ZipFile::default();
        outer
            .entries_mut()
            .insert("nested.7z".into(), inner.to_bytes().unwrap());
        let reopened_outer = Zip::ZipFile::from_bytes(&outer.to_bytes().unwrap()).unwrap();
        let mut reopened_inner =
            SevenZip::SevenZipFile::from_bytes(reopened_outer.get("nested.7z").unwrap()).unwrap();
        reopened_inner
            .replace("deep/value.txt", b"after".to_vec())
            .unwrap();
        assert_eq!(
            SevenZip::SevenZipFile::from_bytes(&reopened_inner.to_bytes().unwrap())
                .unwrap()
                .get("deep/value.txt"),
            Some(b"after".as_slice())
        );
    }

    #[test]
    fn mixed_sarc_to_zip_roundtrip() {
        use roead::{
            sarc::{Sarc, SarcWriter},
            Endian,
        };
        let mut zip = Zip::ZipFile::default();
        zip.entries_mut()
            .insert("deep/value.txt".into(), b"value".to_vec());
        let mut sarc_writer = SarcWriter::new(Endian::Little);
        sarc_writer.add_file("nested.zip", zip.to_bytes().unwrap());
        let sarc = Sarc::new(sarc_writer.to_binary()).unwrap();
        let nested = Zip::ZipFile::from_bytes(sarc.get_data("nested.zip").unwrap()).unwrap();
        assert_eq!(nested.get("deep/value.txt"), Some(b"value".as_slice()));
    }

    #[test]
    fn mixed_zip_to_rar_when_runtime_is_installed() {
        if Rar::RarFile::discover_executable().is_err() {
            eprintln!("skipping nested RAR test: rar.exe not installed");
            return;
        }
        let mut rar = Rar::RarFile::default();
        rar.entries_mut()
            .insert("deep/value.txt".into(), b"value".to_vec());
        let mut zip = Zip::ZipFile::default();
        zip.entries_mut()
            .insert("nested.rar".into(), rar.to_bytes().unwrap());
        let reopened_zip = Zip::ZipFile::from_bytes(&zip.to_bytes().unwrap()).unwrap();
        let reopened_rar =
            Rar::RarFile::from_bytes(reopened_zip.get("nested.rar").unwrap()).unwrap();
        assert_eq!(
            reopened_rar.get("deep/value.txt"),
            Some(b"value".as_slice())
        );
    }
}
