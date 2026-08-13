use super::{
    BinTextFile::{BymlFile, OpenedFile},
    Wrapper::ExeWrapper,
};
use crate::{
    Open_and_Save::SendData,
    Settings::Pathlib,
    Zstd::{is_gamedatalist, TotkFileType, TotkZstd},
};
use std::{io, path::Path, sync::Arc};

pub struct GameDataList;

impl GameDataList {
    fn converter() -> ExeWrapper {
        ExeWrapper::new("bin/cpp/oead_byml_pipe.exe".to_string(), Vec::new())
    }

    pub fn binary_to_text(data: &[u8], zstd: Arc<TotkZstd<'_>>) -> io::Result<String> {
        let file_data = BymlFile::byml_data_to_bytes(&data.to_vec(), zstd)?;
        Self::converter().binary_to_string(&file_data.data, "byml_binary_to_text".to_string())
    }

    pub fn text_to_binary(text: &str) -> io::Result<Vec<u8>> {
        Self::converter().string_to_binary(text, "byml_text_to_binary".to_string())
    }

    pub fn open<'a, P: AsRef<Path>>(
        path: P,
        zstd: Arc<TotkZstd<'a>>,
    ) -> Option<(OpenedFile<'a>, SendData)> {
        let path = path.as_ref();
        if !is_gamedatalist(path) {
            return None;
        }

        let raw_data = std::fs::read(path).ok()?;
        let file_data = BymlFile::byml_data_to_bytes(&raw_data, zstd.clone()).ok()?;
        let text = Self::converter()
            .binary_to_string(&file_data.data, "byml_binary_to_text".to_string())
            .ok()?;
        let endian = BymlFile::get_endiannes(&file_data.data);
        let pathlib = Pathlib::new(path);

        let mut opened_file = OpenedFile::default();
        opened_file.path = pathlib.clone();
        opened_file.endian = endian;
        opened_file.file_type = TotkFileType::Byml;

        let mut send_data = SendData::default();
        send_data.status_text = format!("Opened {}", pathlib.full_path);
        send_data.path = pathlib;
        send_data.text = text;
        send_data.get_file_label(TotkFileType::Byml, endian);
        Some((opened_file, send_data))
    }
}
