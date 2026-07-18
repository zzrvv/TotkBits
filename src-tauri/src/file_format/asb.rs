use crate::{
    file_format::BinTextFile::OpenedFile,
    parser::asb::{Asb, Baev},
    Open_and_Save::SendData,
    Settings::Pathlib,
    Zstd::{TotkFileType, TotkZstd},
};
use serde::{Deserialize, Serialize};
use std::{io, path::Path, sync::Arc};

/// Native representation passed from the ASB parser to the application layer.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AsbFile {
    #[serde(flatten)]
    pub document: Asb,
    #[serde(rename = "BAEV", skip_serializing_if = "Option::is_none", default)]
    pub baev: Option<Baev>,
}

impl AsbFile {
    pub fn open_asb<'a, P: AsRef<Path>>(
        path: P,
        zstd: Arc<TotkZstd<'a>>,
    ) -> Option<(OpenedFile<'a>, SendData)> {
        let path = path.as_ref();
        let asb_data = read_maybe_compressed(path, &zstd, b"ASB ").ok()?;
        if !asb_data.starts_with(b"ASB ") {
            return None;
        }

        let suggested_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| format!("{}.root.baev", name.split('.').next().unwrap_or(name)))
            .unwrap_or_else(|| "*.baev".to_string());
        let mut dialog = rfd::FileDialog::new()
            .set_title("Select optional BAEV file (Cancel to open without one)")
            .add_filter("Binary Animation Event", &["baev", "zs"])
            .set_file_name(&suggested_name);
        if let Some(parent) = path.parent() {
            dialog = dialog.set_directory(parent);
        }
        let baev_path = dialog.pick_file();
        let baev_data = baev_path
            .as_deref()
            .map(|path| read_maybe_compressed(path, &zstd, b"BFFH"))
            .transpose()
            .ok()?;
        let file = Self::from_binary(&asb_data)
            .and_then(|file| file.with_baev(baev_data.as_deref()))
            .ok()?;
        let text = file.to_yaml().ok()?;
        let mut opened_file = OpenedFile::default();
        opened_file.path = Pathlib::new(path);
        opened_file.file_type = TotkFileType::ASB;
        let mut data = SendData {
            status_text: format!("Opened: {}", opened_file.path.full_path),
            path: Pathlib::new(path),
            text,
            ..Default::default()
        };
        data.get_file_label(TotkFileType::ASB, Some(roead::Endian::Little));
        Some((opened_file, data))
    }

    pub fn from_binary(data: &[u8]) -> io::Result<Self> {
        Ok(Self {
            document: Asb::from_bytes(data)?,
            baev: None,
        })
    }

    pub fn with_baev(mut self, data: Option<&[u8]>) -> io::Result<Self> {
        self.baev = data.map(Baev::from_bytes).transpose()?;
        Ok(self)
    }

    pub fn from_paths(
        asb_path: &Path,
        baev_path: Option<&Path>,
        zstd: Arc<TotkZstd<'_>>,
    ) -> io::Result<Self> {
        let asb = read_maybe_compressed(asb_path, &zstd, b"ASB ")?;
        let baev = baev_path
            .map(|path| read_maybe_compressed(path, &zstd, b"BFFH"))
            .transpose()?;
        Self::from_binary(&asb)?.with_baev(baev.as_deref())
    }

    pub fn binary_to_text(data: &[u8]) -> io::Result<String> {
        Asb::from_bytes(data)?.to_yaml()
    }

    pub fn text_to_binary(text: &str) -> io::Result<Vec<u8>> {
        serde_yaml::from_str::<Self>(text)
            .map_err(io::Error::other)?
            .document
            .to_native_bytes()
    }

    pub fn to_yaml(&self) -> io::Result<String> {
        serde_yaml::to_string(self).map_err(io::Error::other)
    }
}

fn read_maybe_compressed(path: &Path, zstd: &TotkZstd<'_>, magic: &[u8]) -> io::Result<Vec<u8>> {
    let data = std::fs::read(path)?;
    if data.starts_with(magic) {
        Ok(data)
    } else {
        zstd.try_decompress(&data)
    }
}
