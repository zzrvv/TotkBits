use crate::{
    file_format::{
        Archive::{ArchiveCodec, Rar::RarFile, SevenZip::SevenZipFile, Zip::ZipFile},
        BinTextFile::OpenedFile,
    },
    Open_and_Save::{get_binary_by_filetype, get_string_from_data},
    TotkConfig::TotkConfig,
    Zstd::{global_totk_zstd, TotkFileType},
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
        if arguments.len() != 6 {
            eprintln!("Usage: Totkbits.exe --cli <bin_to_text|text_to_bin|extract_archive|dir_to_archive> <file_type> <input> <output>");
            return Some(Self {
                operation: String::new(),
                file_type: String::new(),
                input: PathBuf::new(),
                output: PathBuf::new(),
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
        Some(Self {
            operation: arguments[2].to_string_lossy().to_ascii_lowercase(),
            file_type: arguments[3].to_string_lossy().to_ascii_lowercase(),
            input: absolute(&arguments[4]),
            output: absolute(&arguments[5]),
        })
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
            value => Err(format!("unknown CLI operation: {value}")),
        }
    }

    fn zstd(&self) -> Result<Arc<crate::Zstd::TotkZstd<'static>>, String> {
        let config = TotkConfig::safe_new().map_err(|e| e.to_string())?;
        global_totk_zstd(Arc::new(config), 16).map_err(|e| e.to_string())
    }

    fn bin_to_text(&self) -> Result<(), String> {
        let expected = parse_file_type(&self.file_type)?;
        let bytes = fs::read(&self.input).map_err(|e| format!("failed to read input: {e}"))?;
        let (parsed, text) = get_string_from_data(&self.input, bytes, self.zstd()?)
            .ok_or_else(|| "input could not be converted to text".to_string())?;
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
