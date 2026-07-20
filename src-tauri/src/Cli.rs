use crate::{
    file_format::{
        Ainb::AinbFile,
        Archive::{ArchiveCodec, Rar::RarFile, SevenZip::SevenZipFile, Zip::ZipFile},
        BinTextFile::OpenedFile,
    },
    parser::rstb::ResourceSizeTable,
    Open_and_Save::{get_binary_by_filetype, get_string_from_data},
    TotkConfig::TotkConfig,
    Zstd::{global_totk_zstd, TotkFileType, ZstdDictionary},
};
use roead::{
    sarc::{Sarc, SarcWriter},
    Endian,
};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
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
                | "bphcl_nodes"
                // | "bphcl_validate"
                | "compress" // | "msbt_dump"
                             // | "msbt_verify"
                             // | "rstb_validate"
        );
        let expected_arguments = if matches!(
            operation.as_str(),
            "decompress"
                | "bphcl_nodes"
                | "bphcl_validate"
                | "msbt_dump"
                | "msbt_verify"
                | "rstb_validate"
        ) {
            5
        } else {
            6
        };
        if !is_public_operation || arguments.len() != expected_arguments {
            eprintln!("Usage:\n  Totkbits.exe --cli <bin_to_text|text_to_bin|extract_archive|dir_to_archive> <type> <input> <output>\n  Totkbits.exe --cli <decompress|bphcl_nodes> <input> <output>\n  Totkbits.exe --cli compress <zs|pack|empty|bcett|yaz0> <input> <output>\n");
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
        if matches!(
            operation.as_str(),
            "decompress"
                | "bphcl_nodes"
                | "bphcl_validate"
                | "msbt_dump"
                | "msbt_verify"
                | "rstb_validate"
        ) {
            Some(Self {
                operation,
                file_type: String::new(),
                input: absolute(&arguments[3]),
                output: absolute(&arguments[4]),
            })
        } else {
            Some(Self {
                operation,
                file_type: arguments[3].to_string_lossy().to_ascii_lowercase(),
                input: absolute(&arguments[4]),
                output: absolute(&arguments[5]),
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
            "bphcl_nodes" => self.bphcl_nodes(),
            // "bphcl_validate" => self.bphcl_validate(),
            "compress" => self.compress(),
            // "msbt_dump" => self.msbt_dump(),
            // "msbt_verify" => self.msbt_verify(),
            // "rstb_validate" => self.rstb_validate(),
            value => Err(format!("unknown CLI operation: {value}")),
        }
    }

    fn zstd(&self) -> Result<Arc<crate::Zstd::TotkZstd<'static>>, String> {
        let config = TotkConfig::safe_new().map_err(|e| e.to_string())?;
        global_totk_zstd(Arc::new(config), 16).map_err(|e| e.to_string())
    }

    fn rstb_validate(&self) -> Result<(), String> {
        let source = fs::read(&self.input).map_err(|e| format!("failed to read RSTB: {e}"))?;
        let bytes = if crate::Zstd::is_restbl(&source) {
            source
        } else {
            zstd::decode_all(source.as_slice())
                .map_err(|e| format!("failed to decompress RSTB: {e}"))?
        };
        let table = ResourceSizeTable::from_bytes(&bytes).map_err(|e| e.to_string())?;
        let rebuilt = table.to_bytes().map_err(|e| e.to_string())?;
        println!("RSTB valid: version={:?}, endian={:?}, hashes={}, overflow={}, bytes={}, byte_exact={}", table.version, table.endian, table.hash_table.len(), table.overflow_table.len(), bytes.len(), bytes == rebuilt);
        write_output(&self.output, &rebuilt)
    }

    /// Parse every BPHCL below `input`, validate every exposed YAML leaf, and
    /// write a stable SHA-256 manifest to `output`.
    fn bphcl_validate(&self) -> Result<(), String> {
        let mut rows = Vec::new();
        for entry in walkdir::WalkDir::new(&self.input) {
            let entry = entry.map_err(|e| e.to_string())?;
            if !entry.file_type().is_file() {
                continue;
            }
            let bytes = fs::read(entry.path()).map_err(|e| e.to_string())?;
            let file =
                crate::file_format::bphcl::BphclFile::from_binary(&bytes, Some(entry.path()))
                    .map_err(|e| format!("{}: {e}", entry.path().display()))?;
            file.document.validate().map_err(|e| e.to_string())?;
            file.document
                .type_table
                .validate_rebuild()
                .map_err(|e| format!("{} TYPE: {e}", entry.path().display()))?;
            for leaf in file.leaves().map_err(|e| e.to_string())? {
                if leaf.yaml.to_ascii_lowercase().contains("base64") {
                    return Err(format!(
                        "{} {} contains base64",
                        entry.path().display(),
                        leaf.path
                    ));
                }
                let _: serde_yaml::Value = serde_yaml::from_str(&leaf.yaml)
                    .map_err(|e| format!("{} {}: {e}", entry.path().display(), leaf.path))?;
            }
            if file.raw_binary() != bytes {
                return Err(format!(
                    "{} raw hash roundtrip mismatch",
                    entry.path().display()
                ));
            }
            let relative = entry
                .path()
                .strip_prefix(&self.input)
                .unwrap_or(entry.path());
            rows.push(format!(
                "{}\t{:x}",
                relative.display(),
                Sha256::digest(&bytes)
            ));
        }
        rows.sort();
        write_output(&self.output, rows.join("\n").as_bytes())?;
        println!(
            "BPHCL valid: {} files; hashes written to {}",
            rows.len(),
            self.output.display()
        );
        Ok(())
    }

    fn bphcl_nodes(&self) -> Result<(), String> {
        #[derive(serde::Serialize)]
        struct Nodes {
            cloth: Vec<String>,
            collidables: Vec<String>,
        }

        let mut files = walkdir::WalkDir::new(&self.input)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.file_type().is_file()
                    && entry
                        .path()
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("bphcl"))
            })
            .map(|entry| entry.into_path())
            .collect::<Vec<_>>();
        files.sort();

        let mut manifest = BTreeMap::new();
        for path in files {
            let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let document = crate::parser::bphcl::BphclDocument::parse(&bytes)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let relative = path.strip_prefix(&self.input).unwrap_or(&path);
            let key = relative.to_string_lossy().replace('\\', "/");
            if manifest.contains_key(&key) {
                return Err(format!("duplicate BPHCL corpus name: {key}"));
            }
            manifest.insert(
                key,
                Nodes {
                    cloth: document.cloth.into_iter().map(|node| node.name).collect(),
                    collidables: document
                        .collidables
                        .into_iter()
                        .map(|node| node.name)
                        .collect(),
                },
            );
        }
        let json = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
        write_output(&self.output, &json)?;
        println!(
            "BPHCL nodes: {} files written to {}",
            manifest.len(),
            self.output.display()
        );
        Ok(())
    }

    fn bin_to_text(&self) -> Result<(), String> {
        let expected = parse_file_type(&self.file_type)?;
        let bytes = fs::read(&self.input).map_err(|e| format!("failed to read input: {e}"))?;
        if expected == TotkFileType::AINB {
            let text = AinbFile::binary_to_text(&bytes)
                .map_err(|e| format!("input could not be converted to AINB text: {e}"))?;
            return write_output(&self.output, text.as_bytes());
        }
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
        let decompressed = if bytes.starts_with(b"Yaz0") {
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

    fn msbt_files(&self) -> Result<Vec<PathBuf>, String> {
        if !self.input.is_dir() {
            return Err("MSBT corpus input must be a directory".into());
        }
        let mut files = Vec::new();
        collect_files(&self.input, &mut files)?;
        files.retain(|p| {
            p.extension()
                .and_then(|x| x.to_str())
                .is_some_and(|x| x.eq_ignore_ascii_case("msbt"))
        });
        files.sort();
        Ok(files)
    }

    fn msbt_dump(&self) -> Result<(), String> {
        for path in self.msbt_files()? {
            let data = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let parsed = crate::parser::msbt::Msbt::from_bytes(&data)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let mut relative = path
                .strip_prefix(&self.input)
                .map_err(|e| e.to_string())?
                .to_path_buf();
            relative.set_extension("txt");
            write_output(
                &self.output.join(relative),
                crate::parser::msbt::editable::serialize(&parsed).as_bytes(),
            )?;
        }
        Ok(())
    }

    fn msbt_verify(&self) -> Result<(), String> {
        let files = self.msbt_files()?;
        let mut report = String::from("file\toriginal_sha256\trebuilt_sha256\tmatch\n");
        let mut changed = 0;
        for path in &files {
            let data = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let parsed = crate::parser::msbt::Msbt::from_bytes(&data)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let text = crate::parser::msbt::editable::serialize(&parsed);
            if text.to_ascii_lowercase().contains("base64") {
                return Err(format!("base64 node in {}", path.display()));
            }
            let reparsed = crate::parser::msbt::editable::deserialize(&parsed, &text)
                .map_err(|e| format!("{} text: {e}", path.display()))?;
            let rebuilt = reparsed
                .to_bytes()
                .map_err(|e| format!("{} rebuild: {e}", path.display()))?;
            let original_hash = hex_sha256(&data);
            let rebuilt_hash = hex_sha256(&rebuilt);
            let matches = original_hash == rebuilt_hash;
            if !matches {
                changed += 1;
            }
            report.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                path.strip_prefix(&self.input).unwrap_or(path).display(),
                original_hash,
                rebuilt_hash,
                matches
            ));
        }
        report.push_str(&format!(
            "\n{} MSBT files checked; {} byte-identical, {} changed\n",
            files.len(),
            files.len() - changed,
            changed
        ));
        write_output(&self.output, report.as_bytes())?;
        if changed > 0 {
            return Err(format!(
                "{changed} MSBT round trips changed; see {}",
                self.output.display()
            ));
        }
        Ok(())
    }

    fn ainb_roundtrip(&self) -> Result<(), String> {
        if !self.input.is_dir() {
            return Err("AINB round-trip input must be a directory".into());
        }
        let mut files = Vec::new();
        collect_files(&self.input, &mut files)?;
        files.sort();
        let mut report = String::from("file\toriginal_sha256\trebuilt_sha256\tmatch\n");
        let mut tested = 0usize;
        for path in files {
            let bytes =
                fs::read(&path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
            if !bytes.starts_with(b"AIB ") {
                continue;
            }
            let document = crate::parser::ainb::AinbDocument::from_bytes(&bytes)
                .map_err(|e| format!("failed to parse {}: {e}", path.display()))?;
            let yaml = document
                .to_yaml()
                .map_err(|e| format!("failed to encode {}: {e}", path.display()))?;
            if yaml.contains("data_base64") || yaml.contains("original_data_base64") {
                return Err(format!(
                    "base64 payload leaked into YAML for {}",
                    path.display()
                ));
            }
            crate::parser::ainb::AinbDocument::from_yaml(&yaml).map_err(|e| {
                format!("failed to parse generated YAML for {}: {e}", path.display())
            })?;
            let rebuilt = document
                .to_bytes()
                .map_err(|e| format!("failed to preserve {}: {e}", path.display()))?;
            let original_hash = hex_sha256(&bytes);
            let rebuilt_hash = hex_sha256(&rebuilt);
            let matches = original_hash == rebuilt_hash;
            report.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                path.strip_prefix(&self.input).unwrap_or(&path).display(),
                original_hash,
                rebuilt_hash,
                matches
            ));
            if !matches {
                return Err(format!("AINB round trip changed {}", path.display()));
            }
            tested += 1;
        }
        report.push_str(&format!("\n{} AINB files passed\n", tested));
        write_output(&self.output, report.as_bytes())
    }

    fn asb_validate(&self) -> Result<(), String> {
        if !self.input.is_dir() {
            return Err("asb_validate requires an ASB directory".into());
        }
        let mut files = Vec::new();
        collect_files(&self.input, &mut files)?;
        files.retain(|p| p.to_string_lossy().to_ascii_lowercase().contains(".asb"));
        files.sort();
        let mut report = String::from("file\toriginal_sha256\trebuilt_sha256\tmatch\n");
        for path in &files {
            let packed = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
            if !packed.starts_with(b"ASB ") {
                return Err(format!("{} is not a decompressed ASB file", path.display()));
            }
            let bytes = packed;
            let parsed = crate::parser::asb::Asb::from_bytes(&bytes)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let rebuilt = parsed.to_bytes();
            let original_hash = hex_sha256(&bytes);
            let rebuilt_hash = hex_sha256(&rebuilt);
            report.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                path.file_name().unwrap_or_default().to_string_lossy(),
                original_hash,
                rebuilt_hash,
                original_hash == rebuilt_hash
            ));
            if original_hash != rebuilt_hash {
                return Err(format!("ASB round trip changed {}", path.display()));
            }
        }
        report.push_str(&format!("\n{} ASB files passed\n", files.len()));
        write_output(&self.output, report.as_bytes())
    }

    fn asb_native_yaml(&self) -> Result<(), String> {
        let bytes = fs::read(&self.input)
            .map_err(|e| format!("failed to read {}: {e}", self.input.display()))?;
        let yaml = crate::parser::asb::Asb::from_bytes(&bytes)
            .and_then(|document| document.to_yaml())
            .map_err(|e| format!("failed to parse {}: {e}", self.input.display()))?;
        write_output(&self.output, yaml.as_bytes())
    }

    fn baev_native_yaml(&self) -> Result<(), String> {
        let bytes = fs::read(&self.input)
            .map_err(|e| format!("failed to read {}: {e}", self.input.display()))?;
        let yaml = crate::parser::asb::Baev::from_bytes(&bytes)
            .and_then(|document| document.to_yaml())
            .map_err(|e| format!("failed to parse {}: {e}", self.input.display()))?;
        write_output(&self.output, yaml.as_bytes())
    }

    fn asb_yaml_roundtrip(&self) -> Result<(), String> {
        let yaml = fs::read_to_string(&self.input)
            .map_err(|e| format!("failed to read {}: {e}", self.input.display()))?;
        let output = crate::parser::asb::Asb::from_yaml(&yaml)
            .and_then(|document| document.to_yaml())
            .map_err(|e| format!("failed to deserialize {}: {e}", self.input.display()))?;
        write_output(&self.output, output.as_bytes())
    }

    fn asb_yaml_to_binary(&self) -> Result<(), String> {
        let yaml = fs::read_to_string(&self.input)
            .map_err(|e| format!("failed to read {}: {e}", self.input.display()))?;
        let output = crate::parser::asb::Asb::from_yaml(&yaml)
            .and_then(|document| document.to_native_bytes())
            .map_err(|e| format!("failed to write {}: {e}", self.input.display()))?;
        write_output(&self.output, &output)
    }

    fn asb_native_events(&self) -> Result<(), String> {
        let bytes = fs::read(&self.input)
            .map_err(|e| format!("failed to read {}: {e}", self.input.display()))?;
        let yaml = crate::parser::asb::Asb::from_bytes(&bytes)
            .and_then(|document| document.events_yaml())
            .map_err(|e| format!("failed to parse {}: {e}", self.input.display()))?;
        write_output(&self.output, yaml.as_bytes())
    }

    fn asb_native_connections(&self) -> Result<(), String> {
        let bytes = fs::read(&self.input)
            .map_err(|e| format!("failed to read {}: {e}", self.input.display()))?;
        let yaml = crate::parser::asb::Asb::from_bytes(&bytes)
            .and_then(|document| document.connections_yaml())
            .map_err(|e| format!("failed to parse {}: {e}", self.input.display()))?;
        write_output(&self.output, yaml.as_bytes())
    }
}

fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.is_dir() {
            collect_files(&path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

fn hex_sha256(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
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
