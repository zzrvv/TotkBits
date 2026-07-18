use crate::{
    parser::asb::{Asb, Baev},
    Zstd::TotkZstd,
};
use std::{io, path::Path, sync::Arc};

/// Native representation passed from the ASB parser to the application layer.
pub struct AsbFile {
    pub document: Asb,
    pub baev: Option<Baev>,
}

impl AsbFile {
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
}

fn read_maybe_compressed(path: &Path, zstd: &TotkZstd<'_>, magic: &[u8]) -> io::Result<Vec<u8>> {
    let data = std::fs::read(path)?;
    if data.starts_with(magic) {
        Ok(data)
    } else {
        zstd.try_decompress(&data)
    }
}
