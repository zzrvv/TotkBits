use crate::{
    file_format::{BinTextFile::BymlFile, Esetb::Esetb},
    parser::msbt::Msbt,
    Settings::Pathlib,
    Zstd::{TotkFileType, TotkZstd, ZstdDictionary},
};
use std::{io, sync::Arc};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FileOrigin {
    Disk,
    Archive {
        parent_document_id: String,
        path: String,
    },
    NestedArchive {
        parent_document_id: String,
        outer_path: String,
        inner_path: String,
    },
}

impl Default for FileOrigin {
    fn default() -> Self {
        Self::Disk
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Compression {
    None,
    Zstandard(ZstdDictionary),
    Yaz0 { alignment: u32 },
}

impl Default for Compression {
    fn default() -> Self {
        Self::None
    }
}

impl Compression {
    pub fn for_new_path(path: &str, file_type: TotkFileType) -> Self {
        if !path.to_ascii_lowercase().ends_with(".zs") {
            return Self::None;
        }
        if file_type == TotkFileType::Bcett || path.to_ascii_lowercase().ends_with(".bcett.byml.zs")
        {
            Self::Zstandard(ZstdDictionary::Bcett)
        } else {
            Self::Zstandard(ZstdDictionary::Zs)
        }
    }

    pub fn detect_and_decode(data: &[u8], zstd: &TotkZstd<'_>) -> io::Result<(Vec<u8>, Self)> {
        if data.starts_with(b"Yaz0") {
            let alignment = TotkZstd::yaz0_alignment(data);
            return TotkZstd::decompress_yaz0(data)
                .map(|decoded| (decoded, Self::Yaz0 { alignment }));
        }
        match zstd.try_decompress_with_dictionary(data) {
            Ok((decoded, ZstdDictionary::Yaz0)) => Ok((
                decoded,
                Self::Yaz0 {
                    alignment: TotkZstd::yaz0_alignment(data),
                },
            )),
            Ok((decoded, dictionary)) => Ok((decoded, Self::Zstandard(dictionary))),
            Err(_) => Ok((data.to_vec(), Self::None)),
        }
    }

    pub fn encode(self, data: &[u8], zstd: &TotkZstd<'_>) -> io::Result<Vec<u8>> {
        match self {
            Self::None => Ok(data.to_vec()),
            Self::Zstandard(dictionary) => zstd.compress_with_dictionary(data, dictionary),
            Self::Yaz0 { alignment } => TotkZstd::compress_yaz0_with_alignment(data, alignment),
        }
    }

    pub fn dictionary(self) -> Option<ZstdDictionary> {
        match self {
            Self::None => None,
            Self::Zstandard(dictionary) => Some(dictionary),
            Self::Yaz0 { .. } => Some(ZstdDictionary::Yaz0),
        }
    }
}

pub enum InternalContent<'a> {
    None,
    Byml(BymlFile<'a>),
    Esetb(Esetb<'a>),
    Msbt(Msbt),
    Text,
    Aamp,
    Structured,
}

impl Default for InternalContent<'_> {
    fn default() -> Self {
        Self::None
    }
}

pub struct InternalFile<'a> {
    pub path: Pathlib,
    pub file_type: TotkFileType,
    pub endian: Option<roead::Endian>,
    pub compression: Compression,
    pub origin: FileOrigin,
    pub content: InternalContent<'a>,
}

impl Default for InternalFile<'_> {
    fn default() -> Self {
        Self {
            path: Pathlib::default(),
            file_type: TotkFileType::None,
            endian: None,
            compression: Compression::None,
            origin: FileOrigin::Disk,
            content: InternalContent::None,
        }
    }
}

impl<'a> InternalFile<'a> {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: Pathlib::new(path.into()),
            ..Self::default()
        }
    }

    pub fn with_origin(mut self, origin: FileOrigin) -> Self {
        self.origin = origin;
        self
    }

    pub fn byml(&self) -> Option<&BymlFile<'a>> {
        match &self.content {
            InternalContent::Byml(file) => Some(file),
            _ => None,
        }
    }

    pub fn byml_mut(&mut self) -> Option<&mut BymlFile<'a>> {
        match &mut self.content {
            InternalContent::Byml(file) => Some(file),
            _ => None,
        }
    }

    pub fn esetb_mut(&mut self) -> Option<&mut Esetb<'a>> {
        match &mut self.content {
            InternalContent::Esetb(file) => Some(file),
            _ => None,
        }
    }

    pub fn msbt(&self) -> Option<&Msbt> {
        match &self.content {
            InternalContent::Msbt(file) => Some(file),
            _ => None,
        }
    }

    pub fn refresh_zstd(&mut self, zstd: Arc<TotkZstd<'a>>) {
        match &mut self.content {
            InternalContent::Byml(file) => file.zstd = zstd,
            InternalContent::Esetb(file) => file.byml.zstd = zstd,
            _ => {}
        }
    }
}
