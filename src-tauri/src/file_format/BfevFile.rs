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
    original_text: String,
}

impl BfevFile {
    pub fn from_binary(data: &[u8]) -> io::Result<Self> {
        let document = BfevDocument::from_binary(data)?;
        let original_text = document.to_json()?;
        Ok(Self {
            document,
            original_text,
        })
    }

    pub fn binary_to_text(data: &[u8]) -> io::Result<String> {
        Self::from_binary(data)?.document.to_json()
    }

    pub fn text_to_binary(text: &str) -> io::Result<Vec<u8>> {
        let document: BfevDocument = serde_json::from_str(text)
            .or_else(|_| serde_yaml::from_str(text))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        document.to_binary()
    }

    pub fn save_text(&self, text: &str) -> io::Result<Vec<u8>> {
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
    use sha2::{Digest, Sha256};
    use std::fs;

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

    #[test]
    fn corpus_json_roundtrip_sha256() {
        let root = std::path::Path::new(r"W:\coding\TotkBits\tmp\event");
        if !root.is_dir() {
            return;
        }
        let mut paths = fs::read_dir(root)
            .expect("read event corpus")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("bfevfl"))
            })
            .collect::<Vec<_>>();
        paths.sort();
        let mut failures = Vec::new();
        for path in paths {
            let name = path.file_name().unwrap().to_string_lossy();
            let original = fs::read(&path).expect("read BFEVFL");
            let result = BfevFile::binary_to_text(&original)
                .and_then(|json| BfevFile::text_to_binary(&json));
            match result {
                Ok(rebuilt) => {
                    let before = format!("{:x}", Sha256::digest(&original));
                    let after = format!("{:x}", Sha256::digest(&rebuilt));
                    let first = original
                        .iter()
                        .zip(&rebuilt)
                        .position(|(left, right)| left != right)
                        .map(|offset| format!("{offset:#x}"))
                        .unwrap_or_else(|| "length".to_owned());
                    println!(
                        "{name}\t{before}\t{after}\t{}\t{} -> {}\tfirst={first}",
                        original == rebuilt,
                        original.len(),
                        rebuilt.len()
                    );
                    if original != rebuilt {
                        failures.push(name.into_owned());
                    }
                }
                Err(error) => {
                    println!("{name}\tERROR\t{error}");
                    failures.push(format!("{name}: {error}"));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "BFEVFL JSON round-trip failures:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn timeline_json_roundtrip_sha256() {
        let root = std::path::Path::new(r"W:\coding\TotkBits\tmp\BfevLibrary\tests\Data");
        if !root.is_dir() {
            return;
        }
        let mut failures = Vec::new();
        for path in fs::read_dir(root)
            .expect("read timeline corpus")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("bfevtm"))
            })
        {
            let original = fs::read(&path).expect("read BFEVTM");
            let rebuilt = BfevFile::binary_to_text(&original)
                .and_then(|json| BfevFile::text_to_binary(&json))
                .expect("timeline JSON round-trip");
            let name = path.file_name().unwrap().to_string_lossy();
            println!(
                "{name}\t{:x}\t{:x}\t{}\t{} -> {}\tfirst={:?}",
                Sha256::digest(&original),
                Sha256::digest(&rebuilt),
                original == rebuilt,
                original.len(),
                rebuilt.len(),
                original.iter().zip(&rebuilt).position(|(a, b)| a != b)
            );
            if original != rebuilt {
                failures.push(name.into_owned());
            }
        }
        assert!(
            failures.is_empty(),
            "BFEVTM JSON round-trip failures:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn edited_json_rebuilds_without_backing_binary() {
        for path in [
            r"W:\coding\TotkBits\tmp\event\BallBring_MiniGame.bfevfl",
            r"W:\coding\TotkBits\tmp\BfevLibrary\tests\Data\Demo161_0.bfevtm",
        ] {
            let path = std::path::Path::new(path);
            if !path.is_file() {
                continue;
            }
            let original = fs::read(path).expect("read EVFL edit fixture");
            let text = BfevFile::binary_to_text(&original).expect("EVFL to JSON");
            let mut value: serde_json::Value =
                serde_json::from_str(&text).expect("parse EVFL JSON");
            value["FileName"] = serde_json::Value::String("EditedFromJson".to_owned());
            let edited = serde_json::to_string_pretty(&value).expect("serialize edited EVFL JSON");
            let rebuilt = BfevFile::text_to_binary(&edited).expect("fresh edited EVFL rebuild");
            assert_ne!(rebuilt, original);
            let parsed = crate::parser::evfl::BfevDocument::from_binary(&rebuilt)
                .expect("parse edited EVFL rebuild");
            assert_eq!(parsed.file_name, "EditedFromJson");
        }
    }
}
