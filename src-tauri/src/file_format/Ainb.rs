use crate::{
    file_format::BinTextFile::OpenedFile,
    parser::ainb::AinbDocument,
    Open_and_Save::SendData,
    Settings::Pathlib,
    Zstd::{TotkFileType, TotkZstd},
};
use std::{io, path::Path, sync::Arc};

pub struct AinbFile;

impl AinbFile {
    pub fn binary_to_text(data: &[u8]) -> io::Result<String> {
        AinbDocument::from_bytes(data)?.to_yaml()
    }

    pub fn text_to_binary(text: &str) -> io::Result<Vec<u8>> {
        AinbDocument::from_yaml(text)?.to_bytes()
    }

    pub fn open_ainb<P: AsRef<Path>>(
        path: P,
        _zstd: Arc<TotkZstd<'_>>,
    ) -> Option<(OpenedFile, SendData)> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).ok()?;
        let text = Self::binary_to_text(&bytes).ok()?;
        let mut opened_file = OpenedFile::default();
        opened_file.path = Pathlib::new(path);
        opened_file.file_type = TotkFileType::AINB;
        let mut data = SendData::default();
        data.status_text = format!("Opened: {}", opened_file.path.full_path);
        data.path = Pathlib::new(path);
        data.text = text;
        data.get_file_label(TotkFileType::AINB, None);
        Some((opened_file, data))
    }
}
