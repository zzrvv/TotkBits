use std::{io, path::Path, sync::Arc};

use roead::aamp::ParameterIO;

use crate::{
    file_format::{
        asb::AsbFile,
        msbt::MsbtFile,
        Ainb::AinbFile,
        Archive::ArchiveDocument,
        BfevFile::BfevFile,
        BinTextFile::{BymlFile, FileData},
        Esetb::Esetb,
        Image::ImageDocument,
        Model3D::bfres::BfresFile,
        Pack::PackComparer,
        Rstb::Restbl,
        TagProduct::TagProduct,
        Xlink::Xlink_rs,
    },
    parser::{fbx::FbxFile, AOC::g1m::G1mFile},
    Settings::{Magic, Pathlib},
    Zstd::{is_esetb, is_tagproduct, TotkFileType, TotkZstd, ZstdDictionary},
};

pub enum FileGenesis {
    Disk,
    Archive,
    None,
}

pub enum TotkEndian {
    LE,
    BE,
    None,
}

pub struct CacheText<'a> {
    pub byml: Option<BymlFile<'a>>,
    pub esetb: Option<Esetb<'a>>,
    pub tag: Option<TagProduct<'a>>,
    pub msbt: Option<MsbtFile>,
}

impl Default for CacheText<'_> {
    fn default() -> Self {
        CacheText {
            byml: None,
            esetb: None,
            tag: None,
            msbt: None,
        }
    }
}

pub struct CacheArchive<'a> {
    pub sarc: PackComparer<'a>,
    pub archive: Option<ArchiveDocument>,
}

impl<'a> CacheArchive<'a> {
    pub fn default_new(zstd: Arc<TotkZstd<'a>>) -> Self {
        let sarc = PackComparer::default_new(zstd.clone());
        CacheArchive {
            sarc: sarc,
            archive: None,
        }
    }
}

pub struct CacheOther<'a> {
    pub rstb: Option<Restbl<'a>>,
    pub bfew: Option<BfevFile>,
}

impl Default for CacheOther<'_> {
    fn default() -> Self {
        CacheOther {
            rstb: None,
            bfew: None,
        }
    }
}
pub struct Cache3D {
    pub g1m: Option<G1mFile>,
    pub bfres: Option<BfresFile>,
    pub fbx: Option<FbxFile>,
    pub image: Option<ImageDocument>,
}

impl Default for Cache3D {
    fn default() -> Self {
        Cache3D {
            g1m: None,
            bfres: None,
            fbx: None,
            image: None,
        }
    }
}

pub struct TotkFile<'a> {
    pub zstd: Arc<TotkZstd<'a>>,
    pub file_type: TotkFileType,
    pub endian: TotkEndian,
    pub compression: ZstdDictionary,
    pub genesis: FileGenesis,
    pub path: Pathlib,
    pub binary_raw: Vec<u8>,
    pub text: String,
    //cache
    pub cache_arc: CacheArchive<'a>,
    pub cache_text: CacheText<'a>,
    pub cache_3d: Cache3D,
    pub cache_misc: CacheOther<'a>,
}

impl<'a> TotkFile<'a> {
    pub fn default(zstd: Arc<TotkZstd<'a>>) -> Self {
        Self {
            zstd: zstd.clone(),
            file_type: TotkFileType::None,
            endian: TotkEndian::None,
            compression: ZstdDictionary::None,
            genesis: FileGenesis::None,
            path: Pathlib::default(),
            binary_raw: Default::default(),
            text: Default::default(),
            cache_arc: CacheArchive::default_new(zstd),
            cache_text: Default::default(),
            cache_3d: Default::default(),
            cache_misc: Default::default(),
        }
    }

    pub fn filetype_from_path(path: impl AsRef<Path>) -> TotkFileType {
        let path = path.as_ref();
        if is_tagproduct(path) {
            return TotkFileType::TagProduct;
        }
        if is_esetb(path) {
            return TotkFileType::Esetb;
        }
        if Pathlib::is_rstb_path(path) {
            return TotkFileType::Restbl;
        }
        TotkFileType::None
    }

    pub fn raise_err_template<T>(&self, msg: String) -> io::Result<T> {
        let _ = rfd::MessageDialog::new()
            .set_title("Error")
            .set_description(msg.as_str())
            .show();
        Err(io::Error::new(io::ErrorKind::InvalidData, msg))
    }

    pub fn raise_err_path_filetype<T>(&self) -> io::Result<T> {
        let msg = format!(
            "ERROR: Unable to parse path based file {} of type {:?}!",
            &self.path.full_path, &self.file_type
        );
        self.raise_err_template(msg)
    }

    pub fn raise_err_magic_filetype<T>(&self) -> io::Result<T> {
        let msg = format!(
            "ERROR: Unable to parse magic based file {:?} of type {:?}!",
            Magic::magic_to_str(&self.binary_raw),
            &self.file_type
        );
        self.raise_err_template(msg)
    }

    pub fn update_endian_from_byml(&mut self, byml: &BymlFile) {
        match byml.endian {
            Some(roead::Endian::Big) => self.endian = TotkEndian::BE,
            Some(roead::Endian::Little) => self.endian = TotkEndian::LE,
            None => self.endian = TotkEndian::None,
        }
    }

    pub fn raise_not_implemented<T>(&self) -> io::Result<T> {
        self.raise_err_template("NOT implemented feature! FIX!".to_string())
    }

    pub fn from_binary(
        data: &[u8],
        zstd: Arc<TotkZstd<'a>>,
        path: impl AsRef<Path>,
    ) -> io::Result<Self> {
        let mut res = Self::default(zstd.clone());
        let path = path.as_ref();
        (res.binary_raw, res.compression) = zstd.try_decompress_all_ordered_safe(data, path);
        res.path = Pathlib::new(path);
        let filetype_path = Self::filetype_from_path(path);
        let filetype_magic = Magic::from_binary(&res.binary_raw);
        if filetype_magic == TotkFileType::None {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Invalid magic: {}", Magic::magic_to_str(&res.binary_raw)),
            ));
        }
        res.file_type = filetype_path;
        match filetype_path {
            TotkFileType::TagProduct => {
                if let Some(mut tag) = TagProduct::from_binary(&res.binary_raw, path, zstd.clone())
                {
                    res.endian = TotkEndian::LE;
                    res.file_type = TotkFileType::TagProduct;
                    res.text = tag.to_text();
                    res.cache_text.tag = Some(tag);
                    return Ok(res);
                }
                return res.raise_err_path_filetype();
            }
            TotkFileType::Esetb => {
                if let Ok(esetb) = Esetb::from_binary(&res.binary_raw, zstd.clone()) {
                    res.update_endian_from_byml(&esetb.byml);
                    res.file_type = TotkFileType::Esetb;
                    res.text = esetb.to_string();
                    res.cache_text.esetb = Some(esetb);
                    return Ok(res);
                }
                return res.raise_err_path_filetype();
            }
            TotkFileType::Restbl => {
                if let Some(restbl) = Restbl::from_binary(data, zstd.clone(), path) {
                    res.endian = TotkEndian::LE;
                    res.file_type = TotkFileType::Restbl;
                    res.path = Pathlib::new(path);
                    if zstd.totk_config.rstb_view == "json" {
                        res.text = match restbl.to_json() {
                            Ok(text) => text,
                            Err(error) => {
                                return res.raise_err_template(format!(
                                    "Detected Restbl for {}, but parsing failed: {error}",
                                    path.display()
                                ));
                            }
                        };
                    }
                    res.cache_misc.rstb = Some(restbl);
                    return Ok(res);
                }
                return res.raise_err_path_filetype();
            }
            TotkFileType::None => {}
            _ => {
                // return res.raise_not_implemented();
            }
        }
        res.file_type = filetype_magic;
        macro_rules! magic_result {
            ($value:expr) => {
                match $value {
                    Ok(value) => value,
                    Err(_) => return res.raise_err_magic_filetype(),
                }
            };
        }
        macro_rules! magic_option {
            ($value:expr) => {
                match $value {
                    Some(value) => value,
                    None => return res.raise_err_magic_filetype(),
                }
            };
        }
        match filetype_magic {
            TotkFileType::Byml => {
                let file_type = if res.compression == ZstdDictionary::Bcett {
                    TotkFileType::Bcett
                } else {
                    TotkFileType::Byml
                };
                let file_data = FileData {
                    file_type,
                    data: res.binary_raw.clone(),
                    compression: (res.compression != ZstdDictionary::None)
                        .then_some(res.compression),
                    yaz0_alignment: 0,
                };
                let mut byml =
                    magic_result!(BymlFile::from_binary(&res.binary_raw, zstd.clone(), path));
                byml.file_type = file_type;
                byml.file_data = file_data;
                res.update_endian_from_byml(&byml);
                res.file_type = file_type;
                res.text = byml.to_string();
                res.cache_text.byml = Some(byml);
            }
            TotkFileType::ASB => {
                res.text = magic_result!(AsbFile::binary_to_text(&res.binary_raw));
                res.endian = TotkEndian::LE;
                res.file_type = TotkFileType::ASB;
            }
            TotkFileType::AINB => {
                res.text = magic_result!(AinbFile::binary_to_text(&res.binary_raw));
                res.endian = TotkEndian::LE;
                res.file_type = TotkFileType::AINB;
            }
            TotkFileType::Evfl => {
                let bfev = magic_result!(BfevFile::from_binary(&res.binary_raw));
                res.text = magic_result!(bfev.document.to_json());
                res.endian = TotkEndian::LE;
                res.file_type = TotkFileType::Evfl;
                res.cache_misc.bfew = Some(bfev);
            }
            TotkFileType::Xlink => {
                let compression =
                    (res.compression != ZstdDictionary::None).then_some(res.compression);
                let (_, text) = magic_option!(Xlink_rs::open_internal(
                    path,
                    &res.binary_raw,
                    zstd.clone(),
                    compression
                ));
                res.text = text;
                res.endian = TotkEndian::LE;
                res.file_type = TotkFileType::Xlink;
            }
            TotkFileType::Aamp => {
                let pio = magic_result!(ParameterIO::from_binary(&res.binary_raw));
                res.text = magic_result!(crate::file_format::bphcl::safe_aamp_yaml(&pio));
                res.endian = TotkEndian::None;
                res.file_type = TotkFileType::Aamp;
            }
            TotkFileType::Msbt => {
                let msbt = magic_option!(MsbtFile::from_binary(
                    res.binary_raw.clone(),
                    Some(path.to_string_lossy().into_owned()),
                ));
                res.endian = match msbt.endian {
                    roead::Endian::Little => TotkEndian::LE,
                    roead::Endian::Big => TotkEndian::BE,
                };
                res.text = msbt.text.clone();
                res.file_type = TotkFileType::Msbt;
                res.cache_text.msbt = Some(msbt);
            }
            TotkFileType::Text => {
                res.text = magic_result!(String::from_utf8(res.binary_raw.clone()));
                res.endian = TotkEndian::None;
                res.file_type = TotkFileType::Text;
            }
            TotkFileType::Restbl => {
                let restbl = magic_option!(Restbl::from_binary(data, zstd.clone(), path));
                if zstd.totk_config.rstb_view == "json" {
                    res.text = magic_result!(restbl.to_json());
                }
                res.endian = TotkEndian::LE;
                res.file_type = TotkFileType::Restbl;
                res.cache_misc.rstb = Some(restbl);
            }
            _ => return res.raise_err_magic_filetype(),
        }

        Ok(res)
    }
}
