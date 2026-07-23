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
    original_binary: Vec<u8>,
    original_text: String,
}

impl BfevFile {
    pub fn from_binary(data: &[u8]) -> io::Result<Self> {
        let document = BfevDocument::from_binary(data)?;
        let original_text = document.to_json()?;
        Ok(Self {
            document,
            original_binary: data.to_vec(),
            original_text,
        })
    }

    pub fn binary_to_text(data: &[u8]) -> io::Result<String> {
        Self::from_binary(data)?.document.to_json()
    }

    pub fn text_to_binary(text: &str) -> io::Result<Vec<u8>> {
        let _: BfevDocument = serde_json::from_str(text)
            .or_else(|_| serde_yaml::from_str(text))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "edited BFEVFL writing is unavailable because the native EVFL serializer is not implemented",
        ))
    }

    pub fn save_text(&self, text: &str) -> io::Result<Vec<u8>> {
        let edited: BfevDocument = serde_json::from_str(text)
            .or_else(|_| serde_yaml::from_str(text))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let original: BfevDocument = serde_json::from_str(&self.original_text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if serde_json::to_value(edited).map_err(io::Error::other)?
            == serde_json::to_value(original).map_err(io::Error::other)?
        {
            return Ok(self.original_binary.clone());
        }
        Self::text_to_binary(text)
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
        let bfev = Self::from_binary(&bytes).ok()?;
        let text = bfev.original_text.clone();
        let mut opened_file = OpenedFile::default();
        opened_file.path = Pathlib::new(path);
        opened_file.endian = Some(roead::Endian::Little);
        opened_file.file_type = TotkFileType::Evfl;
        opened_file.bfev = Some(bfev);
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

#[cfg(test)]
mod tests {
    use super::BfevFile;

    #[test]
    fn unchanged_sage_of_zora_save_is_byte_perfect() {
        let path = std::path::Path::new(r"W:\coding\TotkBits\tmp\event\SageOfZora.bfevfl");
        if !path.is_file() {
            return;
        }
        let original = std::fs::read(path).expect("read SageOfZora corpus file");
        let file = BfevFile::from_binary(&original).expect("parse SageOfZora");
        let text = file.document.to_json().expect("serialize SageOfZora text");
        let saved = file.save_text(&text).expect("save unchanged SageOfZora");
        assert_eq!(saved, original);
    }
}
