use std::{fs, io::Read, path::Path};

use super::BinTextFile::OpenedFile;
use crate::Open_and_Save::SendData;
use crate::Settings::Pathlib;
use crate::Zstd::{is_aamp, TotkFileType};
use roead::aamp::ParameterIO;

pub struct AampFile;

impl AampFile {
    pub fn open_aamp<P: AsRef<Path>>(path: P) -> Option<(OpenedFile<'static>, SendData)> {
        let mut opened_file = OpenedFile::default();
        let mut data = SendData::default();
        let path_ref = path.as_ref();
        let pathlib_var = Pathlib::new(path_ref);
        print!("Is {} an aamp? ", &pathlib_var.full_path);
        let raw_data = std::fs::read(path_ref).ok()?;
        if is_aamp(&raw_data) {
            let pio = ParameterIO::from_binary(&raw_data).ok()?; // Parse AAMP from binary data
            println!(" yes!");
            opened_file.path = pathlib_var.clone();
            opened_file.file_type = TotkFileType::Aamp;
            data.status_text = format!("Opened {}", &pathlib_var.full_path);
            data.path = pathlib_var;
            data.text = pio.to_text();
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
        let mut opened_file = OpenedFile::default();
        let mut data = SendData::default();
        let path_ref = path.as_ref();
        let pathlib_var = Pathlib::new(path_ref);
        print!("Is {} regular text file? ", &pathlib_var.full_path);
        let mut file = fs::File::open(path_ref).ok()?;
        let mut buffer = Vec::new();
        if let Ok(x) = file.read_to_end(&mut buffer) {
            if let Ok(text) = String::from_utf8(buffer) {
                println!(" yes!");
                opened_file.path = pathlib_var.clone();
                opened_file.file_type = TotkFileType::Text;
                data.status_text = format!("Opened {}", &pathlib_var.full_path);
                data.path = pathlib_var;
                data.text = text;
                data.get_file_label(TotkFileType::Text, None);
                return Some((opened_file, data));
            }
        }
        println!(" no");
        None
    }
}
