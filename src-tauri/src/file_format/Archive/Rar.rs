use super::{detect_archive_magic, ArchiveCodec, ArchiveMagic, ArchiveResult};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default)]
pub struct RarFile {
    entries: BTreeMap<String, Vec<u8>>,
}

impl RarFile {
    pub fn discover_executable() -> ArchiveResult<std::path::PathBuf> {
        Err("RAR is unavailable: TotkBits does not invoke external archive executables".into())
    }
}

impl ArchiveCodec for RarFile {
    fn from_bytes(data: &[u8]) -> ArchiveResult<Self> {
        if detect_archive_magic(data) != Some(ArchiveMagic::Rar) {
            return Err("input is not a RAR archive".into());
        }
        Err("RAR is unsupported without an in-process reader".into())
    }

    fn to_bytes(&self) -> ArchiveResult<Vec<u8>> {
        Err("RAR is unsupported without an in-process writer".into())
    }

    fn entries(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.entries
    }

    fn entries_mut(&mut self) -> &mut BTreeMap<String, Vec<u8>> {
        &mut self.entries
    }
}
