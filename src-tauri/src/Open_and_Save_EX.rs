use crate::{
    file_format::{
        asb::AsbFile,
        msbt::MsbtFile,
        Ainb::AinbFile,
        BfevFile::BfevFile,
        BinTextFile::{is_banc_path, replace_rotate_deg_to_rad, BymlFile, OpenedFile},
        Esetb::Esetb,
        GameDataList::GameDataList,
        Pack::PackComparer,
        Rstb::Restbl,
        TagProduct::TagProduct,
        Xlink::Xlink_rs,
        SMO::SmoSaveFile::SmoSaveFile,
    },
    InternalFile_EX::{Compression, FileOrigin, InternalContent, InternalFile},
    Open_and_Save::SendData,
    Zstd::{
        is_aamp, is_ainb, is_asb, is_byml, is_esetb, is_evfl, is_gamedatalist, is_tagproduct,
        TotkFileType, TotkZstd, ZstdDictionary,
    },
};
use rfd::{FileDialog, MessageDialog};
use roead::{aamp::ParameterIO, byml::Byml};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

pub struct OpenResult<'a> {
    pub file: InternalFile<'a>,
    pub text: String,
}

pub struct EncodeRequest<'a> {
    pub file_type: TotkFileType,
    pub text: &'a str,
    pub path: &'a str,
    pub endian: roead::Endian,
    pub compression: Compression,
    pub internal_msbt: Option<&'a crate::parser::msbt::Msbt>,
    pub is_internal: bool,
}

pub fn open_internal_file<'a>(
    path: impl AsRef<Path>,
    bytes: &[u8],
    zstd: Arc<TotkZstd<'a>>,
) -> io::Result<OpenResult<'a>> {
    let path = path.as_ref().to_string_lossy().into_owned();
    let forced_dictionary = is_banc_path(&path).then_some(ZstdDictionary::Bcett);
    let is_zstandard = bytes.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]);
    let (decoded, compression) =
        if let Some(dictionary) = forced_dictionary.filter(|_| is_zstandard) {
            (
                zstd.try_decompress_using(bytes, dictionary)?,
                Compression::Zstandard(dictionary),
            )
        } else {
            Compression::detect_and_decode(bytes, &zstd)?
        };
    let mut result = dispatch_internal(&path, decoded, zstd)?;
    result.file.compression = compression;
    Ok(result)
}

pub fn open_archive_entry<'a>(
    parent_document_id: impl Into<String>,
    path: impl AsRef<Path>,
    bytes: &[u8],
    zstd: Arc<TotkZstd<'a>>,
) -> io::Result<OpenResult<'a>> {
    let path_text = path.as_ref().to_string_lossy().into_owned();
    let mut result = open_internal_file(&path_text, bytes, zstd)?;
    result.file.origin = FileOrigin::Archive {
        parent_document_id: parent_document_id.into(),
        path: path_text,
    };
    Ok(result)
}

pub fn open_nested_archive_entry<'a>(
    parent_document_id: impl Into<String>,
    outer_path: impl Into<String>,
    inner_path: impl AsRef<Path>,
    bytes: &[u8],
    zstd: Arc<TotkZstd<'a>>,
) -> io::Result<OpenResult<'a>> {
    let inner_path_text = inner_path.as_ref().to_string_lossy().into_owned();
    let mut result = open_internal_file(&inner_path_text, bytes, zstd)?;
    result.file.origin = FileOrigin::NestedArchive {
        parent_document_id: parent_document_id.into(),
        outer_path: outer_path.into(),
        inner_path: inner_path_text,
    };
    Ok(result)
}

fn dispatch_internal<'a>(
    path: &str,
    bytes: Vec<u8>,
    zstd: Arc<TotkZstd<'a>>,
) -> io::Result<OpenResult<'a>> {
    if bytes.is_empty() {
        return Err(invalid("file is empty"));
    }
    let lower = path.to_ascii_lowercase();
    let mut file = InternalFile::new(path);

    if is_tagproduct(path) {
        let mut tag = TagProduct::from_binary(&bytes, path, zstd)
            .ok_or_else(|| invalid("invalid TagProduct"))?;
        file.file_type = TotkFileType::TagProduct;
        file.endian = Some(roead::Endian::Little);
        return Ok(OpenResult {
            file,
            text: tag.to_text(),
        });
    }
    if is_esetb(path) {
        let esetb = Esetb::from_binary(&bytes, zstd)?;
        let text = esetb.to_string();
        file.file_type = TotkFileType::Esetb;
        file.endian = Some(roead::Endian::Little);
        file.content = InternalContent::Esetb(esetb);
        return Ok(OpenResult { file, text });
    }
    if is_banc_path(path) || lower.ends_with(".byml") || lower.ends_with(".byml.zs") {
        return open_internal_byml(file, bytes, zstd);
    }
    if lower.ends_with(".asb") || lower.ends_with(".asb.zs") || is_asb(&bytes) {
        file.file_type = TotkFileType::ASB;
        file.endian = Some(roead::Endian::Little);
        file.content = InternalContent::Structured;
        return Ok(OpenResult {
            file,
            text: AsbFile::binary_to_text(&bytes)?,
        });
    }
    if lower.ends_with(".ainb") || lower.ends_with(".ainb.zs") || is_ainb(&bytes) {
        file.file_type = TotkFileType::AINB;
        file.endian = Some(roead::Endian::Little);
        file.content = InternalContent::Structured;
        return Ok(OpenResult {
            file,
            text: AinbFile::binary_to_text(&bytes)?,
        });
    }
    if lower.ends_with(".bfevfl") || lower.ends_with(".bfevfl.zs") || is_evfl(&bytes) {
        file.file_type = TotkFileType::Evfl;
        file.endian = Some(roead::Endian::Little);
        file.content = InternalContent::Structured;
        return Ok(OpenResult {
            file,
            text: BfevFile::binary_to_text(&bytes)?,
        });
    }
    if let Some((legacy, text)) = Xlink_rs::open_internal(path, &bytes, zstd.clone()) {
        file.file_type = legacy.file_type;
        file.endian = legacy.endian;
        file.content = InternalContent::Structured;
        return Ok(OpenResult { file, text });
    }
    if is_byml(&bytes) {
        return open_internal_byml(file, bytes, zstd);
    }
    if is_aamp(&bytes) {
        let pio = ParameterIO::from_binary(&bytes).map_err(io::Error::other)?;
        file.file_type = TotkFileType::Aamp;
        file.content = InternalContent::Aamp;
        return Ok(OpenResult {
            file,
            text: crate::file_format::bphcl::safe_aamp_yaml(&pio)?,
        });
    }
    if let Some((legacy, text)) = MsbtFile::open_internal(path, &bytes) {
        file.file_type = legacy.file_type;
        file.endian = legacy.endian;
        file.content = legacy
            .msyt
            .map(InternalContent::Msbt)
            .unwrap_or(InternalContent::Structured);
        return Ok(OpenResult { file, text });
    }
    let text = String::from_utf8(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    file.file_type = TotkFileType::Text;
    file.content = InternalContent::Text;
    Ok(OpenResult { file, text })
}

fn open_internal_byml<'a>(
    mut file: InternalFile<'a>,
    bytes: Vec<u8>,
    zstd: Arc<TotkZstd<'a>>,
) -> io::Result<OpenResult<'a>> {
    let data = BymlFile::byml_data_to_bytes(&bytes, zstd.clone())?;
    let byml = BymlFile::from_binary(data, zstd, file.path.full_path.clone())?;
    let text = byml.to_string();
    file.endian = byml.endian;
    file.file_type = byml.file_data.file_type;
    file.content = InternalContent::Byml(byml);
    Ok(OpenResult { file, text })
}

pub fn encode_internal_file(
    file: &mut InternalFile<'_>,
    text: &str,
    zstd: Arc<TotkZstd<'_>>,
) -> io::Result<Vec<u8>> {
    if let InternalContent::Msbt(msbt) = &file.content {
        let raw = encode_uncompressed(
            EncodeRequest {
                file_type: file.file_type,
                text,
                path: &file.path.full_path,
                endian: file.endian.unwrap_or(roead::Endian::Little),
                compression: Compression::None,
                internal_msbt: Some(msbt),
                is_internal: true,
            },
            None,
            zstd.clone(),
        )?;
        return file.compression.encode(&raw, &zstd);
    }
    let raw = encode_uncompressed(
        EncodeRequest {
            file_type: file.file_type,
            text,
            path: &file.path.full_path,
            endian: file.endian.unwrap_or(roead::Endian::Little),
            compression: Compression::None,
            internal_msbt: None,
            is_internal: true,
        },
        Some(&mut file.content),
        zstd.clone(),
    )?;
    file.compression.encode(&raw, &zstd)
}

pub fn encode_uncompressed(
    request: EncodeRequest<'_>,
    mut content: Option<&mut InternalContent<'_>>,
    zstd: Arc<TotkZstd<'_>>,
) -> io::Result<Vec<u8>> {
    let data = match request.file_type {
        TotkFileType::Bphcl => return Err(unsupported("BPHCL requires its opened document")),
        TotkFileType::Hkcl => return Err(unsupported("HKCL writing is not implemented")),
        TotkFileType::Bphhb => return Err(unsupported("BPHHB writing is not implemented")),
        TotkFileType::Xlink => Xlink_rs::text_to_binary(
            request.text,
            request.path,
            zstd.clone(),
            Some(ZstdDictionary::Zs),
        )
        .ok_or_else(|| invalid("failed to encode Xlink"))?,
        TotkFileType::Evfl => BfevFile::text_to_binary(request.text)?,
        TotkFileType::Esetb => match content.as_deref_mut() {
            Some(InternalContent::Esetb(file)) => file.text_to_binary(request.text)?,
            _ => return Err(invalid("ESETB state is unavailable")),
        },
        TotkFileType::ASB => AsbFile::text_to_binary(request.text, None)?,
        TotkFileType::AINB => AinbFile::text_to_binary(request.text)?,
        TotkFileType::TagProduct => TagProduct::to_binary(request.text)?,
        TotkFileType::Byml | TotkFileType::Bcett => {
            let processed;
            let text = if is_banc_path(request.path) && zstd.totk_config.rotation_deg {
                processed = replace_rotate_deg_to_rad(request.text);
                processed.as_str()
            } else {
                request.text
            };
            if is_gamedatalist(request.path) {
                GameDataList::text_to_binary(text)?
            } else {
                Byml::from_text(text)
                    .map_err(io::Error::other)?
                    .to_binary(request.endian)
            }
        }
        TotkFileType::Msbt => MsbtFile::text_to_binary(
            request.text,
            request.path,
            request.internal_msbt,
            request.is_internal,
        )
        .ok_or_else(|| invalid("failed to encode MSBT"))?,
        TotkFileType::Aamp => crate::file_format::bphcl::aamp_from_yaml(request.text)?.to_binary(),
        TotkFileType::SmoSaveFile => {
            let mut file = SmoSaveFile::from_string(request.text, zstd.clone())?;
            file.endian = request.endian;
            file.to_binary()?
        }
        TotkFileType::Text => request.text.as_bytes().to_vec(),
        other => return Err(unsupported(&format!("{other:?} writing is not supported"))),
    };
    request.compression.encode(&data, &zstd)
}

pub fn encode_opened_document(
    request: EncodeRequest<'_>,
    opened: &mut OpenedFile<'_>,
    zstd: Arc<TotkZstd<'_>>,
) -> io::Result<Vec<u8>> {
    let data = match request.file_type {
        TotkFileType::Bphcl => opened
            .bphcl
            .as_ref()
            .map(|file| file.raw_binary())
            .ok_or_else(|| invalid("BPHCL state is unavailable"))?,
        TotkFileType::Hkcl => return Err(unsupported("HKCL writing is not implemented")),
        TotkFileType::Bphhb => return Err(unsupported("BPHHB writing is not implemented")),
        TotkFileType::Esetb => opened
            .esetb
            .as_mut()
            .ok_or_else(|| invalid("ESETB state is unavailable"))?
            .text_to_binary(request.text)?,
        TotkFileType::ASB => AsbFile::text_to_binary(request.text, Some(opened))?,
        _ => return encode_uncompressed(request, None, zstd),
    };
    request.compression.encode(&data, &zstd)
}

pub fn open_file_from_disk<'a>(
    path: impl AsRef<Path>,
    zstd: Arc<TotkZstd<'a>>,
) -> Option<(OpenedFile<'a>, SendData)> {
    let path = path.as_ref();
    Xlink_rs::open_xlink(path, zstd.clone())
        .or_else(|| TagProduct::open_tag(path, zstd.clone()))
        .or_else(|| Esetb::open_esetb(path, zstd.clone()))
        .or_else(|| Restbl::open_restbl(path, zstd.clone()))
        .or_else(|| AsbFile::open_asb(path, zstd.clone()))
        .or_else(|| AinbFile::open_ainb(path, zstd.clone()))
        .or_else(|| BymlFile::open_byml(path, zstd.clone()))
        .or_else(|| MsbtFile::open_mstb(path))
        .or_else(|| {
            #[cfg(debug_assertions)]
            {
                crate::file_format::Model3D::bfres::BfresFile::open(path)
            }
            #[cfg(not(debug_assertions))]
            {
                None
            }
        })
        .or_else(|| {
            #[cfg(debug_assertions)]
            {
                crate::parser::fbx::FbxFile::open(path)
            }
            #[cfg(not(debug_assertions))]
            {
                None
            }
        })
        .or_else(|| BfevFile::open_bfev(path, zstd.clone()))
        .or_else(|| {
            #[cfg(debug_assertions)]
            {
                crate::file_format::Image::ImageDocument::open(path, &zstd)
            }
            #[cfg(not(debug_assertions))]
            {
                None
            }
        })
        .or_else(|| crate::file_format::bphcl::BphclFile::open(path))
        .or_else(|| crate::file_format::hkcl::HkclFile::open(path))
        .or_else(|| crate::file_format::bphhb::BphhbFile::open(path))
        .or_else(|| crate::file_format::SimpleOpeners::AampFile::open_aamp(path))
        .or_else(|| SmoSaveFile::open_smo_save_file(path, zstd))
        .or_else(|| crate::file_format::SimpleOpeners::TextFile::open_text(path))
}

pub fn save_bytes(path: impl AsRef<Path>, bytes: &[u8]) -> io::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = temporary_path(path);
    fs::write(&temporary, bytes)?;
    match fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error)
        }
    }
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".totkbits.tmp");
    PathBuf::from(temporary)
}

pub fn confirm_romfs_write(path: &Path, zstd: &TotkZstd<'_>) -> bool {
    let romfs = Path::new(&zstd.totk_config.romfs);
    if zstd.totk_config.romfs.is_empty() || !path.starts_with(romfs) {
        return true;
    }
    MessageDialog::new()
        .set_title("Warning")
        .set_description(format!(
            "About to save file:\n{}\nin the RomFS dump. Continue?",
            path.display()
        ))
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes
}

pub struct SaveDialog {
    title: String,
    name: Option<String>,
    filters: BTreeMap<String, Vec<String>>,
}

impl SaveDialog {
    pub fn for_document(
        tab: &str,
        pack: Option<&PackComparer<'_>>,
        opened: &OpenedFile<'_>,
        title: impl Into<String>,
    ) -> Self {
        let mut dialog = Self {
            title: title.into(),
            name: None,
            filters: BTreeMap::new(),
        };
        match tab {
            "SARC" if opened.bphcl.is_some() => {
                dialog.name = Some(opened.path.name.clone());
                dialog.filters.insert("BPHCL".into(), vec!["bphcl".into()]);
            }
            "SARC" => {
                dialog.name = pack
                    .and_then(|pack| pack.opened.as_ref())
                    .map(|pack| pack.path.name.clone());
                dialog.filters.insert(
                    "SARC".into(),
                    vec![
                        "pack".into(),
                        "sarc".into(),
                        "pack.zs".into(),
                        "sarc.zs".into(),
                    ],
                );
            }
            "YAML" => {
                dialog.name = Some(opened.path.name.clone());
                let extensions = if opened.path.ext_last.is_empty() {
                    vec![opened.path.extension.clone()]
                } else {
                    vec![opened.path.extension.clone(), opened.path.ext_last.clone()]
                };
                dialog
                    .filters
                    .insert(format!("{:?}", opened.file_type), extensions);
                dialog.filters.insert(
                    "Text Files".into(),
                    vec!["yaml".into(), "json".into(), "yml".into(), "txt".into()],
                );
            }
            _ => {}
        }
        dialog
    }

    pub fn show(self) -> Option<PathBuf> {
        let mut dialog = FileDialog::new()
            .set_title(&self.title)
            .set_file_name(self.name.unwrap_or_default());
        for (name, extensions) in self.filters {
            dialog = dialog.add_filter(name, &extensions);
        }
        dialog.add_filter("All files", &["*"]).save_file()
    }
}

pub fn is_valid_file(path: impl AsRef<Path>) -> bool {
    path.as_ref().is_file()
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn unsupported(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::Unsupported, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TotkConfig::TotkConfig;

    fn zstd() -> Arc<TotkZstd<'static>> {
        Arc::new(TotkZstd::dictionaryless(
            Arc::new(TotkConfig::default()),
            crate::Zstd::TOTK_ZSTD_COMPRESSION_LEVEL,
        ))
    }

    #[test]
    fn internal_text_roundtrip_preserves_empty_zstd() {
        let zstd = zstd();
        let original = zstd.compress_empty(b"hello").unwrap();
        let mut opened =
            open_archive_entry("parent", "notes.txt.zs", &original, zstd.clone()).unwrap();
        assert_eq!(opened.text, "hello");
        assert_eq!(
            opened.file.compression,
            Compression::Zstandard(ZstdDictionary::Empty)
        );
        assert!(matches!(opened.file.origin, FileOrigin::Archive { .. }));

        let saved = encode_internal_file(&mut opened.file, "updated", zstd.clone()).unwrap();
        assert_eq!(TotkZstd::decompress_empty(&saved).unwrap(), b"updated");
    }

    #[test]
    fn nested_text_roundtrip_preserves_yaz0_alignment() {
        let zstd = zstd();
        let original = TotkZstd::compress_yaz0_with_alignment(b"nested", 0x80).unwrap();
        let mut opened =
            open_nested_archive_entry("parent", "outer.pack", "inner.txt", &original, zstd.clone())
                .unwrap();
        assert_eq!(
            opened.file.compression,
            Compression::Yaz0 { alignment: 0x80 }
        );
        assert!(matches!(
            opened.file.origin,
            FileOrigin::NestedArchive { .. }
        ));

        let saved = encode_internal_file(&mut opened.file, "changed", zstd).unwrap();
        assert_eq!(TotkZstd::yaz0_alignment(&saved), 0x80);
        assert_eq!(TotkZstd::decompress_yaz0(&saved).unwrap(), b"changed");
    }

    #[test]
    fn raw_bcett_path_does_not_force_decompression() {
        let zstd = zstd();
        let byml = Byml::Map(Default::default()).to_binary(roead::Endian::Little);
        let opened = open_internal_file("test.bcett.byml", &byml, zstd).unwrap();
        assert_eq!(opened.file.compression, Compression::None);
        assert_eq!(opened.file.file_type, TotkFileType::Byml);
    }
}
