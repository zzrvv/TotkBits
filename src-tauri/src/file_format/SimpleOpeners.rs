use std::{fs, io::Read, path::Path, sync::Arc};

use super::BinTextFile::OpenedFile;
use crate::Open_and_Save::SendData;
use crate::Settings::Magic;
use crate::Settings::Pathlib;
use crate::Zstd::{TotkFileType, TotkZstd};
use roead::aamp::ParameterIO;

use super::bphcl::safe_aamp_yaml;

pub struct AampFile;

impl AampFile {
    pub fn open_aamp<P: AsRef<Path>>(path: P) -> Option<(OpenedFile<'static>, SendData)> {
        let path_ref = path.as_ref();
        let pathlib_var = Pathlib::new(path_ref);
        print!("Is {} an aamp? ", &pathlib_var.full_path);
        let raw_data = std::fs::read(path_ref).ok()?;
        Self::open_aamp_data(&raw_data, path_ref)
    }

    pub fn open_aamp_binary<P: AsRef<Path>>(
        raw_data: &[u8],
        path: P,
        _zstd: Arc<TotkZstd<'_>>,
    ) -> Option<(OpenedFile<'static>, SendData)> {
        Self::open_aamp_data(raw_data, path.as_ref())
    }

    fn open_aamp_data(raw_data: &[u8], path_ref: &Path) -> Option<(OpenedFile<'static>, SendData)> {
        let pathlib_var = Pathlib::new(path_ref);
        let mut opened_file = OpenedFile::default();
        let mut data = SendData::default();
        if Magic::is_aamp(&raw_data) {
            let pio = ParameterIO::from_binary(&raw_data).ok()?; // Parse AAMP from binary data
            println!(" yes!");
            opened_file.path = pathlib_var.clone();
            opened_file.file_type = TotkFileType::Aamp;
            data.status_text = format!("Opened {}", &pathlib_var.full_path);
            data.path = pathlib_var;
            data.text = safe_aamp_yaml(&pio).ok()?;
            data.get_file_label(TotkFileType::Aamp, None);
            return Some((opened_file, data));
        }
        println!(" no");
        None
    }
}

pub struct TextFile;

impl TextFile {
    pub fn open_text<P: AsRef<Path>>(path: P) -> Option<(OpenedFile<'static>, SendData)> {
        let path_ref = path.as_ref();
        let pathlib_var = Pathlib::new(path_ref);
        print!("Is {} regular text file? ", &pathlib_var.full_path);
        let mut file = fs::File::open(path_ref).ok()?;
        let mut buffer = Vec::new();
        if file.read_to_end(&mut buffer).is_ok() {
            return Self::open_text_data(&buffer, path_ref);
        }
        None
    }

    pub fn open_text_binary<P: AsRef<Path>>(
        buffer: &[u8],
        path: P,
        _zstd: Arc<TotkZstd<'_>>,
    ) -> Option<(OpenedFile<'static>, SendData)> {
        Self::open_text_data(buffer, path.as_ref())
    }

    fn open_text_data(buffer: &[u8], path_ref: &Path) -> Option<(OpenedFile<'static>, SendData)> {
        let pathlib_var = Pathlib::new(path_ref);
        let mut opened_file = OpenedFile::default();
        let mut data = SendData::default();
        if let Ok(text) = String::from_utf8(buffer.to_vec()) {
            println!(" yes!");
            opened_file.path = pathlib_var.clone();
            opened_file.file_type = TotkFileType::Text;
            data.status_text = format!("Opened {}", &pathlib_var.full_path);
            data.path = pathlib_var;
            data.text = text;
            data.get_file_label(TotkFileType::Text, None);
            return Some((opened_file, data));
        }
        println!(" no");
        None
    }
}

#[cfg(test)]
mod tests {
    use super::safe_aamp_yaml;
    use crate::file_format::bphcl::aamp_from_yaml;
    use roead::aamp::ParameterIO;

    #[test]
    fn gamescene_aamp_entries_render_without_aborting() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/GameScene");
        let mut count = 0;
        for entry in walkdir::WalkDir::new(root) {
            let entry = entry.expect("read GameScene fixture path");
            if !entry.file_type().is_file() {
                continue;
            }
            let bytes = std::fs::read(entry.path()).expect("read GameScene AAMP fixture");
            if !crate::Settings::Magic::is_aamp(&bytes) {
                continue;
            }
            let parameter_io = ParameterIO::from_binary(&bytes).unwrap_or_else(|error| {
                panic!("failed to parse {}: {error}", entry.path().display())
            });
            let yaml = safe_aamp_yaml(&parameter_io).unwrap_or_else(|error| {
                panic!("failed to serialize {}: {error}", entry.path().display())
            });
            let from_yaml = aamp_from_yaml(&yaml).unwrap_or_else(|error| {
                panic!(
                    "failed to deserialize {}: {error}\n{yaml}",
                    entry.path().display()
                )
            });
            assert_eq!(
                parameter_io,
                from_yaml,
                "YAML changed {}",
                entry.path().display()
            );
            let rebuilt = from_yaml.to_binary();
            let reparsed = ParameterIO::from_binary(&rebuilt).unwrap_or_else(|error| {
                panic!(
                    "failed to parse rebuilt {}: {error}",
                    entry.path().display()
                )
            });
            assert_eq!(
                parameter_io,
                reparsed,
                "binary changed {}",
                entry.path().display()
            );
            count += 1;
        }
        assert_eq!(count, 67, "not every GameScene AAMP fixture was tested");
    }
}
