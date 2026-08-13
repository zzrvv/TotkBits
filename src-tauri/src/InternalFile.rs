use crate::file_format::BinTextFile::BymlFile;
use crate::file_format::Esetb::Esetb;
use crate::file_format::TagProduct::TagProduct;
use crate::parser::msbt::Msbt;
use crate::Settings::Pathlib;
use crate::Zstd::{TotkFileType, ZstdDictionary};

pub struct InternalFile<'a> {
    pub path: Pathlib,
    pub file_type: TotkFileType,
    pub endian: Option<roead::Endian>,
    pub byml: Option<BymlFile<'a>>,
    pub ainb: Option<crate::parser::ainb::AinbDocument>,
    pub asb: Option<crate::parser::asb::Asb>,
    pub msyt: Option<Msbt>,
    pub text: Option<String>,
    pub aamp: Option<String>,
    pub esetb: Option<Esetb<'a>>,
    pub tag: Option<TagProduct<'a>>,
    pub compression: Option<ZstdDictionary>,
    pub yaz0_alignment: u32,
}

impl Default for InternalFile<'_> {
    fn default() -> Self {
        Self {
            path: Pathlib::default(),
            file_type: TotkFileType::None,
            endian: None,
            byml: None,
            ainb: None,
            asb: None,
            msyt: None,
            text: None,
            aamp: None,
            esetb: None,
            tag: None,
            compression: None,
            yaz0_alignment: 0,
        }
    }
}

impl InternalFile<'_> {
    #[allow(dead_code)]
    pub fn new(path: String) -> Self {
        let path = Pathlib::new(path);
        Self {
            path,
            file_type: TotkFileType::None,
            endian: None,
            byml: None,
            ainb: None,
            asb: None,
            msyt: None,
            text: None,
            aamp: None,
            esetb: None,
            tag: None,
            compression: None,
            yaz0_alignment: 0,
        }
    }

    pub fn get_monaco_lang(&self) -> String {
        match self.file_type {
            TotkFileType::Xlink => "xlink".into(),
            TotkFileType::TagProduct => "json".into(),
            TotkFileType::Evfl => "json".into(),
            _ => "yaml".into(),
        }
    }
}
