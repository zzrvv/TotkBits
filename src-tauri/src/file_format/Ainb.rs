use crate::{
    file_format::BinTextFile::OpenedFile,
    parser::ainb::AinbDocument,
    Open_and_Save::SendData,
    Settings::Pathlib,
    Zstd::{TotkFileType, TotkZstd, ZstdDictionary},
};
use std::{io, path::Path, sync::Arc};

pub struct AinbFile;

impl AinbFile {
    pub fn binary_to_text(data: &[u8]) -> io::Result<String> {
        AinbDocument::from_bytes(data)?.to_yaml()
    }

    pub fn text_to_binary(text: &str) -> io::Result<Vec<u8>> {
        AinbDocument::from_yaml(text)?.to_bytes()
    }

    pub fn open_ainb<P: AsRef<Path>>(
        path: P,
        zstd: Arc<TotkZstd<'_>>,
    ) -> Option<(OpenedFile, SendData)> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).ok()?;
        Self::open_ainb_binary(&bytes, path, zstd)
    }

    pub fn open_ainb_binary<'a, P: AsRef<Path>>(
        bytes: &[u8],
        path: P,
        zstd: Arc<TotkZstd<'a>>,
    ) -> Option<(OpenedFile<'a>, SendData)> {
        let path = path.as_ref();
        let (source, compression) = zstd.try_decompress_all_ordered_safe(bytes, path);
        let document = AinbDocument::from_bytes(&source).ok()?;
        let text = document.to_yaml().ok()?;
        let mut opened_file = OpenedFile::default();
        opened_file.path = Pathlib::new(path);
        opened_file.file_type = TotkFileType::AINB;
        opened_file.compression = (compression != ZstdDictionary::None).then_some(compression);
        // opened_file.ainb = Some(document);
        let mut data = SendData::default();
        data.status_text = format!("Opened: {}", opened_file.path.full_path);
        data.path = Pathlib::new(path);
        data.text = text;
        data.get_file_label(TotkFileType::AINB, None);
        Some((opened_file, data))
    }
}

#[cfg(test)]
mod tests {
    use super::AinbFile;
    use sha2::{Digest, Sha256};
    use std::{fs, path::Path};

    #[test]
    fn corpus_yaml_roundtrip_sha256() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/AI");
        let mut paths = fs::read_dir(&root)
            .unwrap_or_else(|error| panic!("unable to read {}: {error}", root.display()))
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("ainb"))
            })
            .collect::<Vec<_>>();
        paths.sort();
        assert!(
            !paths.is_empty(),
            "no AINB files found in {}",
            root.display()
        );

        let mut failures = Vec::new();
        for path in paths {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("<invalid filename>");
            let original = fs::read(&path).expect("read corpus AINB");
            let before = format!("{:x}", Sha256::digest(&original));
            let result = AinbFile::binary_to_text(&original)
                .and_then(|yaml| AinbFile::text_to_binary(&yaml));
            match result {
                Ok(rebuilt) => {
                    let after = format!("{:x}", Sha256::digest(&rebuilt));
                    let first_difference = original
                        .iter()
                        .zip(&rebuilt)
                        .position(|(left, right)| left != right)
                        .map(|offset| format!("{offset:#x}"))
                        .unwrap_or_else(|| "length".to_owned());
                    println!(
                        "{name}\t{before}\t{after}\t{}\t{} -> {}\tfirst={first_difference}",
                        original == rebuilt,
                        original.len(),
                        rebuilt.len()
                    );
                    if original != rebuilt {
                        failures.push(format!("{name}: SHA-256 mismatch"));
                    }
                }
                Err(error) => {
                    println!("{name}\t{before}\tERROR\t{error}");
                    failures.push(format!("{name}: {error}"));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "AINB YAML round-trip failures:\n{}",
            failures.join("\n")
        );
    }
}
