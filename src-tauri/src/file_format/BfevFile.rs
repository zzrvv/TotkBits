use crate::{
    file_format::BinTextFile::OpenedFile,
    parser::evfl::BfevDocument,
    Open_and_Save::SendData,
    Settings::Pathlib,
    Zstd::{TotkFileType, TotkZstd},
};
use std::{io, path::Path, sync::Arc};

/// Native application representation of a BFEV flow file.
pub struct BfevFile {
    pub document: BfevDocument,
}

impl BfevFile {
    pub fn from_binary(data: &[u8]) -> io::Result<Self> {
        Ok(Self {
            document: BfevDocument::from_binary(data)?,
        })
    }

    pub fn binary_to_text(data: &[u8]) -> io::Result<String> {
        Self::from_binary(data)?.document.to_json()
    }

    pub fn open_bfev<'a, P: AsRef<Path>>(
        path: P,
        zstd: Arc<TotkZstd<'a>>,
    ) -> Option<(OpenedFile<'a>, SendData)> {
        let path = path.as_ref();
        let source = std::fs::read(path).ok()?;
        let bytes = if source.starts_with(b"BFEVFL") {
            source
        } else {
            zstd.decompressor.decompress_zs(&source).ok()?
        };
        let text = Self::binary_to_text(&bytes).ok()?;
        let mut opened_file = OpenedFile::default();
        opened_file.path = Pathlib::new(path);
        opened_file.endian = Some(roead::Endian::Little);
        opened_file.file_type = TotkFileType::Evfl;
        let mut data = SendData {
            status_text: format!("Opened: {}", opened_file.path.full_path),
            path: Pathlib::new(path),
            text,
            lang: "json".to_owned(),
            // "tab" selects the frontend workspace, not the Monaco language.
            // Structured text editors use the YAML workspace even when their
            // actual language is JSON.
            tab: "YAML".to_owned(),
            ..Default::default()
        };
        data.get_file_label(TotkFileType::Evfl, Some(roead::Endian::Little));
        Some((opened_file, data))
    }
}
