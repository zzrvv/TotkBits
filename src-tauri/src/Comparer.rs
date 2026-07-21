use std::{io, path::Path, sync::Arc};

use crate::{
    file_format::{BinTextFile::OpenedFile, Pack::PackComparer},
    Open_and_Save::{file_from_disk_to_senddata, get_string_from_data, SendData},
    Settings::Pathlib,
    TotkApp::InternalFile,
    Zstd::TotkZstd,
};
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompareDecision {
    FilesFromDisk,
    RegularFileWithOriginal,
    InternalFileWithFileFromDisk,
    InternalFileWithOriginal,
}
impl CompareDecision {
    #[allow(dead_code)]
    pub fn as_str(&self) -> &str {
        match self {
            CompareDecision::FilesFromDisk => "FilesFromDisk",
            CompareDecision::RegularFileWithOriginal => "RegularFileWithOriginal",
            CompareDecision::InternalFileWithFileFromDisk => "InternalFileWithFileFromDisk",
            CompareDecision::InternalFileWithOriginal => "InternalFileWithOriginal",
        }
    }
}

pub fn str_to_compare_decision(s: &str) -> CompareDecision {
    match s {
        "FilesFromDisk" => CompareDecision::FilesFromDisk,
        "InternalFileWithFileFromDisk" => CompareDecision::InternalFileWithFileFromDisk,
        "InternalFileWithOriginal" => CompareDecision::InternalFileWithOriginal,
        "RegularFileWithOriginal" => CompareDecision::RegularFileWithOriginal,
        _ => CompareDecision::FilesFromDisk,
    }
}

pub struct FileComparer<'a> {
    zstd: Arc<TotkZstd<'a>>,
    _opened_file: &'a OpenedFile<'a>,
    _internal_file: Option<&'a InternalFile<'a>>,
    pack: Option<&'a PackComparer<'a>>,
}

impl<'a> FileComparer<'a> {
    pub fn new(
        zstd: Arc<TotkZstd<'a>>,
        opened_file: &'a OpenedFile<'a>,
        internal_file: Option<&'a InternalFile<'a>>,
        pack: Option<&'a PackComparer<'a>>,
    ) -> Self {
        Self {
            zstd,
            _opened_file: opened_file,
            _internal_file: internal_file,
            pack,
        }
    }

    pub fn fetch_vanilla_path(&self, path: &str) -> Result<String, String> {
        self.zstd
            .totk_config
            .find_vanila_file_in_romfs(path)
            .map_err(|error| format!("Error: {error}"))
    }

    pub fn fetch_file_data(&self, path: &str, is_from_sarc: bool) -> Result<String, String> {
        if is_from_sarc {
            if let Some(rawdata) = self
                .pack
                .and_then(|pack| pack.opened.as_ref())
                .and_then(|opened| opened.sarc.get_data(path))
            {
                return get_string_from_data(path, rawdata.to_vec(), Arc::clone(&self.zstd))
                    .map(|(_, text)| text)
                    .ok_or_else(|| format!("Error parsing data from SARC: {path}"));
            }
        }
        Err(format!("Error: Data not found in SARC for {}", path))
    }

    pub fn fetch_vanilla_data(&self, path: &str) -> Result<String, String> {
        self.zstd
            .find_vanila_internal_file_data_in_romfs(path, Arc::clone(&self.zstd))
            .map_err(|error| format!("Error fetching vanilla data: {error}"))
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct FileToCompare {
    pub path: Pathlib,
    pub label: String,
    pub text: String,
    pub is_internal: bool,
}

impl FileToCompare {
    pub fn new<P: AsRef<Path>>(path: P, text: String) -> Self {
        Self {
            path: Pathlib::new(&path),
            label: String::new(),
            text,
            is_internal: false,
        }
    }

    pub fn get_path_from_dialog(&mut self, title: Option<String>) {
        if let Some(file) = rfd::FileDialog::new()
            .set_title(title.unwrap_or_else(|| "Select file".to_owned()))
            .pick_file()
        {
            self.path = Pathlib::new(file);
        }
    }
    pub fn get_text_from_file_on_disk(&mut self, zstd: Arc<TotkZstd>) -> io::Result<()> {
        if let Some((_, send_data)) = file_from_disk_to_senddata(&self.path.full_path, zstd) {
            self.text = send_data.text;
        }
        Ok(())
    }
    pub fn from_path<P: AsRef<Path>>(&mut self, path: P, zstd: Arc<TotkZstd>) -> io::Result<()> {
        self.path = Pathlib::new(&path);
        if self.path.exists() && self.path.is_file() {
            self.get_text_from_file_on_disk(zstd)?;
            if !self.text.is_empty() {
                self.is_internal = false;
            }
        }
        Ok(())
    }

    pub fn from_disk(&mut self, title: Option<String>, zstd: Arc<TotkZstd>) -> io::Result<()> {
        self.get_path_from_dialog(title);
        if !self.path.full_path.is_empty() {
            self.get_text_from_file_on_disk(zstd)?;
            if !self.text.is_empty() {
                self.is_internal = false;
            }
        }
        Ok(())
    }
    pub fn from_opened_pack(
        &mut self,
        pack: &PackComparer,
        internal_path: String,
        zstd: Arc<TotkZstd>,
    ) -> io::Result<()> {
        if let Some(raw_data) = pack
            .opened
            .as_ref()
            .and_then(|opened| opened.sarc.get_data(&internal_path))
        {
            if let Some((_, text)) = get_string_from_data(&internal_path, raw_data.to_vec(), zstd) {
                self.text = text;
                self.is_internal = true;
            }
        }
        Ok(())
    }
}
const MAX_COMPARE_SIZE: usize = 999 * 1024 * 1024;
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct DiffComparer {
    pub file1: FileToCompare,
    pub file2: FileToCompare,
    pub MAX_COMPARE_SIZE: usize,
    //#[serde(skip)]
    //pub zstd: Arc<TotkZstd>
}

impl Default for DiffComparer {
    fn default() -> Self {
        Self {
            file1: FileToCompare::default(),
            file2: FileToCompare::default(),
            MAX_COMPARE_SIZE: MAX_COMPARE_SIZE,
        }
    }
}

impl DiffComparer {
    pub fn new(file1: FileToCompare, file2: FileToCompare) -> Self {
        Self {
            file1,
            file2,
            MAX_COMPARE_SIZE,
        }
    }
    pub fn new_default() -> Self {
        Self::default()
    }

    fn is_too_large(&self, text: &str) -> bool {
        let size = text.len();
        if size <= self.MAX_COMPARE_SIZE {
            return false;
        }
        self.msgbox_max_size_exceeded(size);
        true
    }

    fn show_error(description: impl AsRef<str>) {
        rfd::MessageDialog::new()
            .set_title("Error")
            .set_description(description.as_ref())
            .show();
    }

    fn same_disk_path(&self) -> bool {
        !self.file1.path.full_path.is_empty()
            && self.file1.path.full_path == self.file2.path.full_path
    }

    fn show_same_path_error(&self) {
        Self::show_error(format!(
            "Paths to both files are the same, skipping comparison\n{}",
            self.file1.path.full_path
        ));
    }

    pub fn msgbox_max_size_exceeded(&self, size: usize) {
        rfd::MessageDialog::new()
            .set_title("Error")
            .set_description(format!(
                "File  too large to compare\nFile text size: {:.2} MB, max size: {} MB",
                (size as f64) / 1024.0 / 1024.0,
                self.MAX_COMPARE_SIZE / 1024 / 1024
            ))
            .show();
    }

    pub fn files_from_disk(zstd: Arc<TotkZstd>, is_from_disk: bool) -> Option<SendData> {
        let mut data: SendData = SendData::default();
        let comp = &mut data.compare_data;
        //File1
        if is_from_disk {
            //load file from disk
            println!("Loading file from disk");
            if let Err(err) = comp
                .file1
                .from_disk(Some("Select first file".to_string()), zstd.clone())
            {
                data.status_text = format!("ERROR: {}", err);
                return Some(data);
            }
            if comp.file1.path.full_path.is_empty() {
                return None;
            }
            if comp.file1.text.is_empty() {
                data.status_text = format!("ERROR: Unable to parse: {}", comp.file1.path.full_path);
                return Some(data);
            }
            comp.file1.label = comp.file1.path.full_path.clone();
        } else {
            println!("Skipping, ill get from Monaco ");
            comp.file1.label = "YAML editor".to_string();
        }
        if comp.is_too_large(&comp.file1.text) {
            return None;
        }
        //File2
        if let Err(err) = comp
            .file2
            .from_disk(Some("Select Second file".to_string()), zstd.clone())
        {
            data.status_text = format!("ERROR: {}", err); //unreachable
            return Some(data);
        }
        if comp.file2.path.full_path.is_empty() {
            return None;
        }
        if comp.same_disk_path() {
            comp.show_same_path_error();
            return None;
        }
        if comp.file2.text.is_empty() {
            data.status_text = format!("ERROR: Unable to parse: {}", comp.file2.path.full_path);
            return Some(data);
        }
        if comp.is_too_large(&comp.file2.text) {
            return None;
        }

        comp.file1.is_internal = false;
        comp.file2.is_internal = false;
        comp.file2.label = comp.file2.path.full_path.clone();
        data.status_text = "Files loaded successfully".to_owned();
        data.file_label = if !comp.file1.path.name.is_empty() {
            comp.file1.path.name.clone()
        } else {
            comp.file2.label.clone()
        };
        // data.compare_data = comp.clone();
        Some(data)
    }

    pub fn regular_file_with_original<P: AsRef<Path>>(
        path: P,
        zstd: Arc<TotkZstd>,
        is_from_disk: bool,
    ) -> Option<SendData> {
        let mut data: SendData = SendData::default();
        let comp = &mut data.compare_data;
        //if is_from_disk then it will ask the dialog box, otherwise the text will be loaded from monaco editor
        //at the frontend; no point in passing the text here; path is passed in order to identify the vanilla
        //file path

        //File1
        if is_from_disk {
            if let Err(err) = comp
                .file1
                .from_disk(Some("Select first file".to_string()), zstd.clone())
            {
                data.status_text = format!("ERROR: {}", err);
                return Some(data);
            }
            if comp.file1.path.full_path.is_empty() {
                return None;
            }
            if !comp.file1.path.exists() {
                data.status_text =
                    format!("ERROR: File does not exist: {}", comp.file1.path.full_path);
                return Some(data);
            }
            if !comp.file1.path.is_file() {
                data.status_text =
                    format!("ERROR: Path is not a file: {}", comp.file1.path.full_path);
                return Some(data);
            }
        }
        //File2
        match zstd.totk_config.find_vanila_file_in_romfs(&path) {
            Ok(vanila_path) => {
                comp.file2.path = Pathlib::new(&vanila_path);
            }
            Err(err) => {
                Self::show_error(format!(
                    "ERROR:\n{err:?}\nUnable to find original path for file:\n{:?}",
                    path.as_ref()
                ));
                data.status_text = format!("ERROR: {:?}", err);
                return Some(data);
            }
        }
        if comp.same_disk_path() {
            comp.show_same_path_error();
            return None;
        }

        if let Err(err) = comp.file2.get_text_from_file_on_disk(zstd.clone()) {
            data.status_text = format!("ERROR: {:?}", err);
            return Some(data);
        }
        if comp.file2.text.is_empty() {
            Self::show_error(format!(
                "ERROR: Could not get text from original file: {}",
                comp.file2.path.full_path
            ));
            data.status_text = format!("ERROR: failed to parse: {}", comp.file2.path.full_path);
            return Some(data);
        }

        Some(data)
    }

    pub fn internal_file_with_disk_file<P: AsRef<Path>>(
        pack: &PackComparer,
        internal_path: P,
        zstd: Arc<TotkZstd>,
        is_from_sarc: bool,
    ) -> Option<SendData> {
        let mut data: SendData = SendData::default();
        let comp = &mut data.compare_data;
        //File1
        comp.file1.path = Pathlib::new(&internal_path);
        if is_from_sarc {
            if let Err(err) = comp.file1.from_opened_pack(
                pack,
                internal_path.as_ref().to_string_lossy().into_owned(),
                Arc::clone(&zstd),
            ) {
                data.status_text = format!("ERROR: {:?}", err);
                return Some(data);
            }
            if comp.file1.text.is_empty() {
                Self::show_error(format!(
                    "ERROR: Could not get text from internal file: {}",
                    comp.file1.path.full_path
                ));
                data.status_text = format!("ERROR: failed to parse: {}", comp.file1.path.full_path);
                return Some(data);
            }
        }
        comp.file1.is_internal = true;
        //File2
        comp.file2
            .get_path_from_dialog(Some("Select second file to compare with".to_string()));
        if comp.file2.path.full_path.is_empty() {
            return None;
        }
        if !comp.file2.path.exists() {
            data.status_text = format!("ERROR: File does not exist: {}", comp.file2.path.full_path);
            return Some(data);
        }
        if !comp.file2.path.is_file() {
            data.status_text = format!("ERROR: Path {} is not a file", comp.file2.path.full_path);
            return Some(data);
        }
        if let Err(err) = comp.file2.get_text_from_file_on_disk(zstd) {
            data.status_text = format!("ERROR: {:?}", err);
            return Some(data);
        }
        if comp.file2.text.is_empty() {
            Self::show_error(format!(
                "ERROR: Could not get text from file:\n{}",
                comp.file2.path.full_path
            ));
            data.status_text = format!("ERROR: failed to parse: {}", comp.file2.path.full_path);
            return Some(data);
        }
        comp.file2.is_internal = false;
        comp.file1.label = comp.file1.path.name.clone();
        comp.file2.label = comp.file2.path.full_path.clone();
        data.status_text = "Files loaded successfully".to_owned();
        // data.compare_data = comp.clone();
        Some(data)
    }

    pub fn internal_file_with_original<P: AsRef<Path>>(
        internal_path: P,
        pack: &PackComparer,
        zstd: Arc<TotkZstd>,
        is_from_sarc: bool,
    ) -> Option<SendData> {
        let mut data: SendData = SendData::default();
        let comp = &mut data.compare_data;
        //File1
        comp.file1.path = Pathlib::new(&internal_path);
        if is_from_sarc {
            if let Err(err) = comp.file1.from_opened_pack(
                pack,
                internal_path.as_ref().to_string_lossy().into_owned(),
                Arc::clone(&zstd),
            ) {
                data.status_text = format!("ERROR: {:?}", err);
                return Some(data);
            }
            if comp.file1.text.is_empty() {
                Self::show_error(format!(
                    "ERROR: Could not get text from internal file: {}",
                    comp.file1.path.full_path
                ));
                data.status_text = format!("ERROR: Failed to parse: {}", comp.file1.path.full_path);
                return Some(data);
            }
        }
        comp.file1.is_internal = true;
        //File2
        comp.file2.path = Pathlib::new(&internal_path);
        match zstd.find_vanila_internal_file_data_in_romfs(&internal_path, Arc::clone(&zstd)) {
            Ok(text) => {
                comp.file2.text = text;
            }
            Err(err) => {
                Self::show_error(format!("ERROR:\n{err:?}"));
                data.status_text = format!("ERROR: {:?}", err);
                return Some(data);
            }
        }
        comp.file2.is_internal = true;
        comp.file1.label = comp.file1.path.name.clone();
        comp.file2.label = "Original".to_string();
        data.status_text = "Files loaded successfully".to_owned();
        // data.compare_data = comp.clone();
        Some(data)
    }

    pub fn compare_by_choice<P: AsRef<Path>>(
        &mut self,
        decision: String,
        pack: &Option<PackComparer>,
        int_or_regular_path: P,
        zstd: Arc<TotkZstd>,
        is_from_disk: bool,
    ) -> Option<SendData> {
        let decision = str_to_compare_decision(&decision);
        let is_from_sarc = is_from_disk;
        println!("Decision: {:?} is from disk: {}", decision, is_from_disk);
        let res = match decision {
            CompareDecision::FilesFromDisk => Self::files_from_disk(zstd.clone(), is_from_disk),
            CompareDecision::RegularFileWithOriginal => {
                Self::regular_file_with_original(&int_or_regular_path, zstd.clone(), is_from_disk)
            }
            CompareDecision::InternalFileWithFileFromDisk => {
                if let Some(pack) = pack {
                    Self::internal_file_with_disk_file(
                        pack,
                        &int_or_regular_path,
                        zstd.clone(),
                        is_from_sarc,
                    )
                } else {
                    None
                }
            }
            CompareDecision::InternalFileWithOriginal => {
                if let Some(pack) = pack {
                    Self::internal_file_with_original(
                        &int_or_regular_path,
                        pack,
                        zstd.clone(),
                        is_from_sarc,
                    )
                } else {
                    None
                }
            }
        };
        if let Some(x) = &res {
            if x.compare_data.file1.text == x.compare_data.file2.text {
                rfd::MessageDialog::new()
                    .set_title("Files are identical")
                    .set_description(format!(
                        "Files:\n{}\nand\n{}\nare identical",
                        &x.compare_data.file1.path.full_path, &x.compare_data.file2.path.full_path
                    ))
                    .show();
                return None;
            }
        }

        res //should not reach here
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comparison_decisions_preserve_legacy_names_and_fallback() {
        let cases = [
            ("FilesFromDisk", CompareDecision::FilesFromDisk),
            (
                "RegularFileWithOriginal",
                CompareDecision::RegularFileWithOriginal,
            ),
            (
                "InternalFileWithFileFromDisk",
                CompareDecision::InternalFileWithFileFromDisk,
            ),
            (
                "InternalFileWithOriginal",
                CompareDecision::InternalFileWithOriginal,
            ),
            ("unknown", CompareDecision::FilesFromDisk),
        ];

        for (name, expected) in cases {
            let actual = str_to_compare_decision(name);
            assert_eq!(actual, expected);
            if name != "unknown" {
                assert_eq!(actual.as_str(), name);
            }
        }
    }

    #[test]
    fn constructors_preserve_data_and_size_limit() {
        let first = FileToCompare::new("first.byml", "first".to_owned());
        let second = FileToCompare::new("second.byml", "second".to_owned());
        let comparer = DiffComparer::new(first.clone(), second.clone());

        assert_eq!(comparer.file1.path.full_path, first.path.full_path);
        assert_eq!(comparer.file1.text, first.text);
        assert_eq!(comparer.file2.path.full_path, second.path.full_path);
        assert_eq!(comparer.file2.text, second.text);
        assert_eq!(comparer.MAX_COMPARE_SIZE, MAX_COMPARE_SIZE);
        assert_eq!(
            DiffComparer::new_default().MAX_COMPARE_SIZE,
            MAX_COMPARE_SIZE
        );
    }
}
