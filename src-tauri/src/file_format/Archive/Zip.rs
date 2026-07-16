use super::{detect_archive_magic, validate_entry_path, ArchiveCodec, ArchiveMagic, ArchiveResult};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

#[derive(Default)]
pub struct ZipFile {
    entries: BTreeMap<String, Vec<u8>>,
}

impl ArchiveCodec for ZipFile {
    fn from_bytes(data: &[u8]) -> ArchiveResult<Self> {
        if detect_archive_magic(data) != Some(ArchiveMagic::Zip) {
            return Err("ZIP magic bytes do not match".into());
        }
        let mut archive =
            ZipArchive::new(Cursor::new(data)).map_err(|e| format!("invalid ZIP: {e}"))?;
        let mut entries = BTreeMap::new();
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).map_err(|e| e.to_string())?;
            if file.encrypted() {
                return Err(format!(
                    "encrypted ZIP entry is unsupported: {}",
                    file.name()
                ));
            }
            if file.is_dir() {
                continue;
            }
            let name = file
                .enclosed_name()
                .ok_or_else(|| format!("unsafe ZIP entry path: {}", file.name()))?
                .to_string_lossy()
                .replace('\\', "/");
            validate_entry_path(&name)?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            entries.insert(name, bytes);
        }
        Ok(Self { entries })
    }
    fn to_bytes(&self) -> ArchiveResult<Vec<u8>> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (name, bytes) in &self.entries {
            validate_entry_path(name)?;
            writer
                .start_file(name, options)
                .map_err(|e| e.to_string())?;
            writer.write_all(bytes).map_err(|e| e.to_string())?;
        }
        Ok(writer.finish().map_err(|e| e.to_string())?.into_inner())
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
    fn zip_roundtrip_and_edit() {
        let mut zip = ZipFile::default();
        zip.entries.insert("folder/a.txt".into(), b"old".to_vec());
        let bytes = zip.to_bytes().unwrap();
        let mut reopened = ZipFile::from_bytes(&bytes).unwrap();
        reopened.replace("folder/a.txt", b"new".to_vec()).unwrap();
        let final_zip = ZipFile::from_bytes(&reopened.to_bytes().unwrap()).unwrap();
        assert_eq!(final_zip.get("folder/a.txt"), Some(&b"new"[..]));
    }
}
