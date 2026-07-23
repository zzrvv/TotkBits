use crate::{
    file_format::{
        Ainb::AinbFile,
        Archive::{ArchiveCodec, Rar::RarFile, SevenZip::SevenZipFile, Zip::ZipFile},
        BinTextFile::OpenedFile,
    },
    Open_and_Save::{get_binary_by_filetype, get_string_from_data},
    TotkConfig::TotkConfig,
    Zstd::{TotkFileType, ZstdDictionary},
};
use roead::{
    sarc::{Sarc, SarcWriter},
    Endian,
};
use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug)]
pub struct CliCommand {
    operation: String,
    file_type: String,
    input: PathBuf,
    output: PathBuf,
    replacement_folder: Option<PathBuf>,
}

impl CliCommand {
    pub fn from_env() -> Option<Self> {
        let arguments: Vec<_> = env::args_os().collect();
        if !matches!(
            arguments.get(1).and_then(|v| v.to_str()),
            Some("-c" | "--cli")
        ) {
            return None;
        }
        let operation = arguments
            .get(2)
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let is_public_operation = matches!(
            operation.as_str(),
            "bin_to_text"
                | "text_to_bin"
                | "extract_archive"
                | "dir_to_archive"
                | "decompress"
                | "compress"
                | "replace_bars_from_folder"
        );
        let expected_arguments = if operation == "decompress" { 5 } else { 6 };
        if !is_public_operation || arguments.len() != expected_arguments {
            eprintln!("Usage:\n  Totkbits.exe --cli <bin_to_text|text_to_bin|extract_archive|dir_to_archive> <type> <input> <output>\n  Totkbits.exe --cli decompress <input> <output>\n  Totkbits.exe --cli compress <zs|pack|empty|bcett|yaz0> <input> <output>\n  Totkbits.exe --cli replace_bars_from_folder <input.bars> <audio-folder> <output.bars>\n");
            return Some(Self {
                operation: String::new(),
                file_type: String::new(),
                input: PathBuf::new(),
                output: PathBuf::new(),
                replacement_folder: None,
            });
        }
        let cwd = env::current_dir().ok()?;
        let absolute = |value: &std::ffi::OsStr| {
            let path = PathBuf::from(value);
            if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            }
        };
        if operation == "decompress" {
            Some(Self {
                operation,
                file_type: String::new(),
                input: absolute(&arguments[3]),
                output: absolute(&arguments[4]),
                replacement_folder: None,
            })
        } else if operation == "replace_bars_from_folder" {
            Some(Self {
                operation,
                file_type: "bars".into(),
                input: absolute(&arguments[3]),
                replacement_folder: Some(absolute(&arguments[4])),
                output: absolute(&arguments[5]),
            })
        } else {
            Some(Self {
                operation,
                file_type: arguments[3].to_string_lossy().to_ascii_lowercase(),
                input: absolute(&arguments[4]),
                output: absolute(&arguments[5]),
                replacement_folder: None,
            })
        }
    }

    pub fn execute(&self) -> Result<(), String> {
        if self.operation.is_empty() {
            return Err("missing CLI arguments".into());
        }
        match self.operation.as_str() {
            "bin_to_text" => self.bin_to_text(),
            "text_to_bin" => self.text_to_bin(),
            "extract_archive" => self.extract_archive(),
            "dir_to_archive" => self.dir_to_archive(),
            "decompress" => self.decompress(),
            "compress" => self.compress(),
            "replace_bars_from_folder" => self.replace_bars_from_folder(),
            value => Err(format!("unknown CLI operation: {value}")),
        }
    }

    fn zstd(&self) -> Result<Arc<crate::Zstd::TotkZstd<'static>>, String> {
        let config = Arc::new(TotkConfig::safe_new(false).map_err(|e| e.to_string())?);
        Ok(Arc::new(crate::Zstd::TotkZstd::dictionaryless(
            config,
            crate::Zstd::TOTK_ZSTD_COMPRESSION_LEVEL,
        )))
    }

    fn bin_to_text(&self) -> Result<(), String> {
        let expected = parse_file_type(&self.file_type)?;
        let bytes = fs::read(&self.input).map_err(|e| format!("failed to read input: {e}"))?;
        if expected == TotkFileType::AINB {
            let text = AinbFile::binary_to_text(&bytes)
                .map_err(|e| format!("input could not be converted to AINB text: {e}"))?;
            return write_output(&self.output, text.as_bytes());
        }
        let is_zstandard = bytes.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]);
        let (parsed, text) = get_string_from_data(&self.input, bytes, self.zstd()?).ok_or_else(|| {
            if is_zstandard {
                "input could not be converted to text; it may require a game Zstandard dictionary unavailable in lightweight CLI mode".to_string()
            } else {
                "input could not be converted to text".to_string()
            }
        })?;
        if parsed.file_type != expected
            && !(expected == TotkFileType::Byml && parsed.file_type == TotkFileType::Bcett)
        {
            return Err(format!(
                "input parsed as {:?}, not {:?}",
                parsed.file_type, expected
            ));
        }
        write_output(&self.output, text.as_bytes())
    }

    fn text_to_bin(&self) -> Result<(), String> {
        let file_type = parse_file_type(&self.file_type)?;
        let text = fs::read_to_string(&self.input)
            .map_err(|e| format!("failed to read input text: {e}"))?;
        let mut opened = OpenedFile::default();
        let output_name = self.output.to_string_lossy();
        let bytes = get_binary_by_filetype(
            file_type,
            &text,
            Endian::Little,
            self.zstd()?,
            &output_name,
            &mut opened,
            None,
            None,
            false,
        )
        .filter(|bytes| !bytes.is_empty())
        .ok_or_else(|| format!("conversion to {} produced no data", self.file_type))?;
        write_output(&self.output, &bytes)
    }

    fn extract_archive(&self) -> Result<(), String> {
        let bytes = fs::read(&self.input).map_err(|e| format!("failed to read archive: {e}"))?;
        let entries = archive_entries(&self.file_type, &bytes)?;
        fs::create_dir_all(&self.output).map_err(|e| e.to_string())?;
        for (name, data) in entries {
            let destination = safe_destination(&self.output, &name)?;
            write_output(&destination, &data)?;
        }
        Ok(())
    }

    fn dir_to_archive(&self) -> Result<(), String> {
        if !self.input.is_dir() {
            return Err("input must be a directory".into());
        }
        let mut entries = Vec::new();
        collect_directory(&self.input, &self.input, &mut entries)?;
        let bytes = build_archive(&self.file_type, entries)?;
        write_output(&self.output, &bytes)
    }

    fn decompress(&self) -> Result<(), String> {
        let bytes = fs::read(&self.input).map_err(|e| format!("failed to read input: {e}"))?;
        let decompressed = if crate::compression::meshcodec::MeshCodec::has_magic(&bytes) {
            crate::compression::meshcodec::MeshCodec::decompress(&bytes)
        } else if bytes.starts_with(b"Yaz0") {
            crate::Zstd::TotkZstd::decompress_yaz0(&bytes)
        } else {
            self.zstd()?.try_decompress(&bytes)
        }
        .map_err(|e| format!("failed to decompress input: {e}"))?;
        write_output(&self.output, &decompressed)
    }

    fn compress(&self) -> Result<(), String> {
        let dictionary = parse_dictionary(&self.file_type)?;
        let bytes = fs::read(&self.input).map_err(|e| format!("failed to read input: {e}"))?;
        let compressed = if dictionary == ZstdDictionary::Yaz0 {
            crate::Zstd::TotkZstd::compress_yaz0(&bytes)
        } else {
            self.zstd()?.compress_with_dictionary(&bytes, dictionary)
        }
        .map_err(|e| format!("failed to compress input: {e}"))?;
        write_output(&self.output, &compressed)
    }

    fn replace_bars_from_folder(&self) -> Result<(), String> {
        use std::collections::HashMap;

        let folder = self
            .replacement_folder
            .as_deref()
            .ok_or("missing audio replacement folder")?;
        let zstd = self.zstd()?;
        let mut archive =
            crate::file_format::Archive::ArchiveDocument::open_with_zstd(&self.input, &zstd)?
                .ok_or_else(|| format!("input is not a BARS archive: {}", self.input.display()))?;
        if !matches!(
            archive.archive,
            crate::file_format::Archive::RootArchive::Bars(_)
        ) {
            return Err(format!(
                "input is not a BARS archive: {}",
                self.input.display()
            ));
        }

        if !folder.is_dir() {
            return Err(format!(
                "audio replacement folder does not exist: {}",
                folder.display()
            ));
        }
        let mut sources = HashMap::new();
        for item in fs::read_dir(folder).map_err(|error| error.to_string())? {
            let path = item.map_err(|error| error.to_string())?.path();
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            if !extension.eq_ignore_ascii_case("wav") && !extension.eq_ignore_ascii_case("mp3") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|value| value.to_str()) {
                sources.entry(stem.to_ascii_lowercase()).or_insert(path);
            }
        }
        let targets: Vec<_> = archive
            .archive
            .entries()
            .keys()
            .filter(|path| {
                path.starts_with("Audio/") && (path.ends_with(".bfwav") || path.ends_with(".bwav"))
            })
            .cloned()
            .collect();
        let mut replaced = 0usize;
        let mut skipped = 0usize;
        let mut failures = Vec::new();
        let mut oversized = 0usize;
        for target in targets {
            let stem = Path::new(&target)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase();
            let Some(source_path) = sources.get(&stem) else {
                skipped += 1;
                continue;
            };
            let original = archive
                .get(&target)
                .ok_or_else(|| format!("archive entry disappeared: {target}"))?
                .to_vec();
            let replacement = crate::file_format::Audio::Bfwav::decode_source(source_path)
                .and_then(|source| {
                    crate::file_format::Audio::encode_replacement(&original, &source)
                });
            match replacement {
                Ok(bytes) => {
                    if bytes.len() > original.len() {
                        oversized += 1;
                    }
                    archive.set(&target, bytes)?;
                    replaced += 1;
                }
                Err(error) => failures.push(format!("{target}: {error}")),
            }
        }
        archive.save_atomic_with_zstd(&self.output, &zstd)?;
        println!(
            "Replaced {} audio file(s); skipped {}; failed {}; oversized {}",
            replaced,
            skipped,
            failures.len(),
            oversized
        );
        for failure in failures {
            eprintln!("{failure}");
        }
        Ok(())
    }
}

fn parse_dictionary(value: &str) -> Result<ZstdDictionary, String> {
    match value {
        "zs" => Ok(ZstdDictionary::Zs),
        "pack" => Ok(ZstdDictionary::Pack),
        "empty" => Ok(ZstdDictionary::Empty),
        "bcett" => Ok(ZstdDictionary::Bcett),
        "yaz0" => Ok(ZstdDictionary::Yaz0),
        _ => Err(format!(
            "unsupported compression: {value}; expected zs, pack, empty, bcett, or yaz0"
        )),
    }
}

fn parse_file_type(value: &str) -> Result<TotkFileType, String> {
    match value {
        "ainb" => Ok(TotkFileType::AINB),
        "asb" => Ok(TotkFileType::ASB),
        "byml" => Ok(TotkFileType::Byml),
        "bcett" => Ok(TotkFileType::Bcett),
        "tagproduct" | "tag_product" => Ok(TotkFileType::TagProduct),
        "aamp" => Ok(TotkFileType::Aamp),
        "msbt" | "msyt" => Ok(TotkFileType::Msbt),
        "evfl" | "bfevfl" => Ok(TotkFileType::Evfl),
        "xlink" | "belnk" => Ok(TotkFileType::Xlink),
        "text" => Ok(TotkFileType::Text),
        "smo" => Ok(TotkFileType::SmoSaveFile),
        _ => Err(format!("unsupported file type: {value}")),
    }
}

fn archive_entries(kind: &str, bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    match kind {
        "zip" => Ok(ZipFile::from_bytes(bytes)?
            .entries()
            .iter()
            .map(|(n, d)| (n.clone(), d.clone()))
            .collect()),
        "7z" => Ok(SevenZipFile::from_bytes(bytes)?
            .entries()
            .iter()
            .map(|(n, d)| (n.clone(), d.clone()))
            .collect()),
        "rar" => Ok(RarFile::from_bytes(bytes)?
            .entries()
            .iter()
            .map(|(n, d)| (n.clone(), d.clone()))
            .collect()),
        "sarc" => {
            let sarc = Sarc::new(bytes).map_err(|e| format!("invalid SARC: {e}"))?;
            sarc.files()
                .map(|file| {
                    file.name()
                        .map(|n| (n.to_string(), file.data.to_vec()))
                        .ok_or_else(|| "unnamed SARC entries are unsupported".to_string())
                })
                .collect()
        }
        _ => Err(format!("unsupported archive type: {kind}")),
    }
}

fn build_archive(kind: &str, entries: Vec<(String, Vec<u8>)>) -> Result<Vec<u8>, String> {
    macro_rules! codec {
        ($ty:ty) => {{
            let mut archive = <$ty>::default();
            for (name, data) in entries {
                archive.entries_mut().insert(name, data);
            }
            archive.to_bytes()
        }};
    }
    match kind {
        "zip" => codec!(ZipFile),
        "7z" => codec!(SevenZipFile),
        "rar" => codec!(RarFile),
        "sarc" => {
            let mut writer = SarcWriter::new(Endian::Little);
            for (name, data) in entries {
                writer.add_file(&name, data);
            }
            Ok(writer.to_binary())
        }
        _ => Err(format!("unsupported archive type: {kind}")),
    }
}

fn collect_directory(
    root: &Path,
    current: &Path,
    result: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), String> {
    for entry in fs::read_dir(current).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            collect_directory(root, &path, result)?;
        } else {
            let name = path
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            crate::file_format::Archive::validate_entry_path(&name)?;
            result.push((name, fs::read(path).map_err(|e| e.to_string())?));
        }
    }
    Ok(())
}

fn safe_destination(root: &Path, name: &str) -> Result<PathBuf, String> {
    crate::file_format::Archive::validate_entry_path(name)?;
    Ok(root.join(name))
}

fn write_output(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(path, data).map_err(|e| format!("failed to write {}: {e}", path.display()))
}
