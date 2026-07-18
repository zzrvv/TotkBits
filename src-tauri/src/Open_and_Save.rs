use crate::{
    file_format::{
        asb::AsbFile,
        Ainb::AinbFile,
        BfevFile::BfevFile,
        BinTextFile::{is_banc_path, replace_rotate_deg_to_rad, BymlFile, OpenedFile},
        Esetb::Esetb,
        Evfl_cs::Evfl,
        GameDataList::GameDataList,
        Msbt::str_endian_to_roead,
        Pack::{PackComparer, SarcPaths},
        Rstb::Restbl,
        TagProduct::TagProduct,
        Xlink::Xlink_rs,
        SMO::SmoSaveFile::SmoSaveFile,
    },
    Comparer::DiffComparer,
    Settings::Pathlib,
    TotkApp::InternalFile,
    Zstd::{
        is_aamp, is_ainb, is_asb, is_byml, is_esetb, is_evfl, is_gamedatalist, is_msyt,
        is_tagproduct, is_xlink, is_xlink_path, TotkFileType, TotkZstd, ZstdDictionary,
    },
};
use msbt_bindings_rs::MsbtCpp::MsbtCpp;
use rfd::{FileDialog, MessageDialog};
use roead::{aamp::ParameterIO, byml::Byml};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Write},
    path::Path,
    sync::Arc,
};
pub fn get_string_from_data<P: AsRef<Path>>(
    filepath: P,
    data: Vec<u8>,
    zstd: Arc<TotkZstd>,
) -> Option<(InternalFile, String)> {
    let path = filepath.as_ref().to_string_lossy().into_owned();
    let (data, dictionary) = if path.to_ascii_lowercase().ends_with(".zs") {
        let (decoded, dictionary) = zstd.try_decompress_with_dictionary(&data).ok()?;
        (decoded, Some(dictionary))
    } else {
        (data, None)
    };
    let (mut internal_file, text) = get_string_from_decoded_data(&path, data, zstd)?;
    internal_file.zstd_dictionary = dictionary;
    Some((internal_file, text))
}

fn get_string_from_decoded_data<P: AsRef<Path>>(
    filepath: P,
    data: Vec<u8>,
    zstd: Arc<TotkZstd>,
) -> Option<(InternalFile, String)> {
    let mut internal_file = InternalFile::default();
    if data.is_empty() {
        return None;
    }
    let path = filepath.as_ref().to_string_lossy().into_owned();
    let lower_path = path.to_ascii_lowercase();

    // Archive entries must be dispatched by their full, lower-cased name before
    // generic magic checks. Several specialized TOTK formats are BYML containers.
    if is_tagproduct(&path) {
        if let Some(mut tag) = TagProduct::from_binary(&data, &path, zstd.clone()) {
            internal_file.endian = Some(roead::Endian::Little);
            internal_file.path = Pathlib::new(path.clone());
            internal_file.file_type = TotkFileType::TagProduct;
            return Some((internal_file, tag.to_text()));
        }
        println!("Unable to parse TagProduct archive entry {path}");
        return None;
    }

    if is_esetb(&filepath) {
        if let Ok(esetb) = Esetb::from_binary(&data, zstd.clone()) {
            internal_file.endian = Some(roead::Endian::Little);
            internal_file.path = Pathlib::new(path.clone());
            internal_file.file_type = TotkFileType::Esetb;
            let text = esetb.to_string();
            internal_file.esetb = Some(esetb);
            return Some((internal_file, text));
        }
    }

    let byml_suffix = lower_path.ends_with(".byml") || lower_path.ends_with(".byml.zs");
    if is_gamedatalist(&path) {
        if let Ok(text) = GameDataList::binary_to_text(&data, zstd.clone()) {
            internal_file.endian = BymlFile::get_endiannes(&data);
            internal_file.path = Pathlib::new(path.clone());
            internal_file.file_type = TotkFileType::Byml;
            return Some((internal_file, text));
        }
        println!("Unable to parse GameDataList archive entry {path}");
        return None;
    }

    if is_banc_path(&path) || byml_suffix {
        if let Ok(file_data) = BymlFile::byml_data_to_bytes(&data, zstd.clone()) {
            if let Ok(byml_file) = BymlFile::from_binary(file_data, zstd.clone(), path.clone()) {
                let text = byml_file.to_string();
                internal_file.endian = byml_file.endian;
                internal_file.path = Pathlib::new(path.clone());
                internal_file.file_type = byml_file.file_data.file_type.clone();
                internal_file.byml = Some(byml_file);
                return Some((internal_file, text));
            }
        }
        println!("Unable to parse named BYML archive entry {path}");
        return None;
    }

    let asb_suffix = lower_path.ends_with(".asb") || lower_path.ends_with(".asb.zs");
    if asb_suffix || is_asb(&data) {
        if let Ok(text) = AsbFile::binary_to_text(&data) {
            internal_file.endian = Some(roead::Endian::Little);
            internal_file.path = Pathlib::new(path.clone());
            internal_file.file_type = TotkFileType::ASB;
            return Some((internal_file, text));
        }
    }

    let ainb_suffix = lower_path.ends_with(".ainb") || lower_path.ends_with(".ainb.zs");
    if ainb_suffix || is_ainb(&data) {
        if let Ok(text) = AinbFile::binary_to_text(&data) {
            internal_file.endian = Some(roead::Endian::Little);
            internal_file.path = Pathlib::new(path.clone());
            internal_file.file_type = TotkFileType::AINB;
            return Some((internal_file, text));
        }
    }
    let evfl_suffix = lower_path.ends_with(".bfevfl") || lower_path.ends_with(".bfevfl.zs");
    if evfl_suffix || is_evfl(&data) {
        if let Ok(text) = BfevFile::binary_to_text(&data) {
            internal_file.endian = Some(roead::Endian::Little);
            internal_file.path = Pathlib::new(path.clone());
            internal_file.file_type = TotkFileType::Evfl;
            return Some((internal_file, text));
        }
    }
    if is_xlink(&data) || is_xlink_path(&path) {
        match Xlink_rs::new(zstd.clone()).and_then(|xlink| xlink.binary_to_yaml(&data)) {
            Ok(text) => {
                internal_file.endian = Some(roead::Endian::Little);
                internal_file.path = Pathlib::new(path.clone());
                internal_file.file_type = TotkFileType::Xlink;
                return Some((internal_file, text));
            }
            Err(error) => println!("Unable to parse XLink entry {path}: {error}"),
        }
    }
    if is_byml(&data) {
        if let Ok(file_data) = BymlFile::byml_data_to_bytes(&data, zstd.clone()) {
            if let Ok(byml_file) = BymlFile::from_binary(file_data, zstd.clone(), path.clone()) {
                // let text = Byml::to_text(&byml_file.pio);
                let text = byml_file.to_string();
                internal_file.endian = byml_file.endian;
                internal_file.file_type = byml_file.file_data.file_type;
                internal_file.byml = Some(byml_file);
                internal_file.path = Pathlib::new(path);
                return Some((internal_file, text));
            }
        }
    }

    if is_aamp(&data) {
        let text = ParameterIO::from_binary(&data).ok()?.to_text();
        internal_file.endian = None;
        internal_file.path = Pathlib::new(path.clone());
        internal_file.file_type = TotkFileType::Aamp;
        return Some((internal_file, text));
    }
    if is_msyt(&data) {
        // let msbt = MsbtFile::from_binary(data, Some(path.clone()))?;
        // internal_file.endian = Some(msbt.endian.clone());
        // internal_file.path = Pathlib::new(path.clone());
        // internal_file.file_type = TotkFileType::Msbt;
        let msbt = MsbtCpp::from_binary(&data).ok()?;

        internal_file.endian = Some(str_endian_to_roead(
            &msbt.endian.unwrap_or("LE".to_string()),
        ));
        internal_file.path = Pathlib::new(path.clone());
        internal_file.file_type = TotkFileType::Msbt;
        return Some((internal_file, msbt.text));
    }
    if let Ok(text) = String::from_utf8(data) {
        internal_file.endian = None;
        internal_file.path = Pathlib::new(path.clone());
        internal_file.file_type = TotkFileType::Text;
        return Some((internal_file, text));
    }

    None
}

#[allow(dead_code)]
fn write_data_to_file<P: AsRef<Path>>(path: P, data: Vec<u8>) -> io::Result<()> {
    let path = path.as_ref();

    // Ensure the parent directory exists.
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Open the file in write mode, creating it if it doesn't exist.
    let mut file = File::create(path)?;

    // Write the data to the file.
    file.write_all(&data)?;

    Ok(())
}

#[allow(dead_code)]
pub fn save_file_dialog(file_name: Option<String>) -> String {
    let name = file_name.unwrap_or_default();
    let file = FileDialog::new().set_file_name(name).save_file();
    match file {
        Some(res) => {
            return res.to_string_lossy().into_owned();
        }
        None => {
            return "".to_string();
        }
    }
}

pub fn check_if_save_in_romfs(dest_file: &str, zstd: Arc<TotkZstd>) -> bool {
    if !dest_file.is_empty() {
        //check if file is saved in romfs
        // if dest_file.starts_with(&zstd.totk_config.romfs.to_string_lossy().to_string()) {
        if dest_file.starts_with(&zstd.totk_config.romfs) {
            let m = format!(
                "About to save file:\n{}\nin romfs dump. Continue?",
                &dest_file
            );
            if MessageDialog::new()
                .set_title("Warning")
                .set_description(m)
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                == rfd::MessageDialogResult::Yes
            {
                return true;
            }
        }
    }
    false
}

pub fn get_binary_by_filetype(
    file_type: TotkFileType,
    text: &str,
    endian: roead::Endian,
    zstd: Arc<TotkZstd>,
    file_path: &str,
    opened_file: &mut OpenedFile<'_>,
    zstd_dictionary: Option<ZstdDictionary>,
) -> Option<Vec<u8>> {
    let mut rawdata: Vec<u8> = Vec::new();
    let endian_str = match endian {
        roead::Endian::Big => "BE",
        roead::Endian::Little => "LE",
    };
    let is_zs = file_path.to_lowercase().ends_with(".zs") && zstd_dictionary.is_none();
    let is_bcett = file_path.to_lowercase().ends_with(".bcett.byml.zs");
    match file_type {
        TotkFileType::Xlink => {
            match Xlink_rs::new(zstd.clone()).and_then(|xlink| xlink.yaml_to_binary(text)) {
                Ok(data) => {
                    rawdata = if is_zs {
                        zstd.cpp_compressor.compress_zs(&data).ok()?
                    } else {
                        data
                    };
                }
                Err(error) => {
                    println!("Unable to save XLink YAML for {file_path}: {error}");
                    return None;
                }
            }
        }
        TotkFileType::Evfl => {
            let evfl = Evfl::new(zstd.clone());
            if let Ok(new_data) = evfl.string_to_binary(text) {
                if is_zs {
                    if let Ok(compressed_data) = zstd.cpp_compressor.compress_zs(&new_data) {
                        rawdata = compressed_data;
                    }
                } else {
                    rawdata = new_data;
                }
            }
        }
        TotkFileType::Esetb => {
            if let Some(esetb) = &mut opened_file.esetb {
                esetb.update_from_text(text).ok()?;
                rawdata = esetb.to_binary();
                if file_path.to_lowercase().ends_with(".zs") {
                    rawdata = zstd.cpp_compressor.compress_zs(&rawdata).ok()?;
                }
            }
        }
        TotkFileType::ASB => {
            if let Ok(some_data) = AsbFile::text_to_binary(text) {
                rawdata = some_data;
                if is_zs {
                    rawdata = zstd.cpp_compressor.compress_zs(&rawdata).ok()?;
                }
            }
        }
        TotkFileType::AINB => {
            if let Ok(some_data) = AinbFile::text_to_binary(text) {
                rawdata = some_data;
            }
        }
        TotkFileType::TagProduct => {
            if let Ok(some_data) = TagProduct::to_binary(text) {
                rawdata = some_data;
                if is_zs {
                    rawdata = zstd.cpp_compressor.compress_zs(&rawdata).ok()?;
                }
            }
        }
        TotkFileType::Byml => {
            if is_gamedatalist(file_path) {
                rawdata = GameDataList::text_to_binary(text)
                    .map_err(|e| println!("Unable to encode GameDataList: {e}"))
                    .ok()?;
            }
            if (rawdata.is_empty()) {
                let processed_text = if is_banc_path(&file_path) && zstd.totk_config.rotation_deg {
                    &replace_rotate_deg_to_rad(&text)
                } else {
                    text
                };

                let pio = Byml::from_text(processed_text).ok()?;
                rawdata = pio.to_binary(endian);
            }
            if (!rawdata.is_empty()) {
                if is_bcett {
                    rawdata = zstd.cpp_compressor.compress_bcett(&rawdata).ok()?;
                } else if is_zs {
                    rawdata = zstd.cpp_compressor.compress_zs(&rawdata).ok()?;
                }
            }
        }
        TotkFileType::Bcett => {
            let processed_text = if zstd.totk_config.rotation_deg {
                &replace_rotate_deg_to_rad(&text)
            } else {
                text
            };
            let pio = Byml::from_text(processed_text).ok()?;
            rawdata = pio.to_binary(endian);
            if is_zs {
                rawdata = zstd.cpp_compressor.compress_bcett(&rawdata).ok()?;
            }
        }
        TotkFileType::Msbt => {
            let result = MsbtCpp::from_text(text, endian_str.to_string());
            if let Ok(msbt) = result {
                rawdata = msbt.binary;
            }
        }
        TotkFileType::Aamp => {
            let pio = ParameterIO::from_text(text).ok()?;
            rawdata = pio.to_binary();
        }
        TotkFileType::SmoSaveFile => {
            let mut smo_file = SmoSaveFile::from_string(text, zstd.clone()).ok()?;
            smo_file.endian = endian;
            rawdata = smo_file.to_binary().ok()?;
        }
        TotkFileType::Text => {
            rawdata = text.as_bytes().to_vec();
        }
        _ => {}
    }

    if let Some(dictionary) = zstd_dictionary {
        rawdata = zstd.compress_with_dictionary(&rawdata, dictionary).ok()?;
    }
    Some(rawdata)
}

pub struct SaveFileDialog<'a> {
    pub tab: String,
    pub pack: &'a Option<PackComparer<'a>>,
    pub opened_file: &'a OpenedFile<'a>,
    pub title: String,
    pub name: Option<String>,
    pub filters: BTreeMap<String, Vec<String>>,
    pub isText: bool,
}
impl SaveFileDialog<'_> {
    pub fn new<'a>(
        tab: String,
        pack: &'a Option<PackComparer<'a>>,
        opened_file: &'a OpenedFile<'a>,
        title: String,
    ) -> SaveFileDialog<'a> {
        SaveFileDialog {
            tab: tab,
            pack: pack,
            opened_file: opened_file,
            title: title,
            name: None,
            filters: Default::default(),
            isText: false,
        }
    }
    pub fn process_name(&mut self) {
        self.name = None;
        match self.tab.as_str() {
            "SARC" => {
                if let Some(pack) = self.pack {
                    if let Some(opened) = &pack.opened {
                        self.name = Some(opened.path.name.clone());
                    }
                }
            }
            "YAML" => {
                self.name = Some(self.opened_file.path.name.clone());
            }
            _ => {}
        }
    }

    pub fn filters_from_path(&mut self, file_path: &str) {
        let path = Pathlib::new(file_path.to_string());
        let x = if path.ext_last.is_empty() {
            vec![path.extension.clone()]
        } else {
            vec![path.extension.clone(), path.ext_last.clone()]
        };
        let y = if path.ext_last.is_empty() {
            path.extension.clone().to_uppercase()
        } else {
            path.ext_last.clone().to_uppercase()
        };

        self.filters.insert(y, x);
    }

    pub fn generate_filters(&mut self) {
        let mut filters: BTreeMap<String, Vec<String>> = BTreeMap::new();
        match self.tab.as_str() {
            "SARC" => {
                filters.insert(
                    "SARC".to_string(),
                    vec![
                        "pack".to_string(),
                        "sarc".to_string(),
                        "pack.zs".to_string(),
                        "sarc.zs".to_string(),
                    ],
                );
            }
            "YAML" => {
                let exts = if self.opened_file.path.ext_last.is_empty() {
                    vec![self.opened_file.path.extension.clone()]
                } else {
                    vec![
                        self.opened_file.path.extension.clone(),
                        self.opened_file.path.ext_last.clone(),
                    ]
                };
                filters.insert(
                    //own extension
                    format!("{:?}", self.opened_file.file_type),
                    exts,
                );
                filters.insert(
                    "Text Files".to_string(),
                    vec![
                        "yaml".to_string(),
                        "json".to_string(),
                        "yml".to_string(),
                        "txt".to_string(),
                    ],
                );
            }
            _ => {} // Add a wildcard pattern to cover all other cases
        }
        // filters.insert("All Files".to_string(), vec!["*".to_string()]);
        self.filters = filters;
    }
    pub fn generate_filters_and_name(&mut self) {
        self.generate_filters();
        self.process_name();
    }

    pub fn show(&mut self) -> String {
        // self.generate_filters();
        // self.process_name();
        let mut result = String::new();
        let mut dialog = FileDialog::new()
            .set_file_name(self.name.clone().unwrap_or_default())
            .set_title(&self.title);
        for (key, value) in &self.filters {
            dialog = dialog.add_filter(key, value);
        }
        let file = dialog
            .add_filter("All files", &vec!["*".to_string()])
            .save_file();
        if let Some(res) = file {
            result = res.to_string_lossy().into_owned();
        }
        self.isText = vec![".txt", ".yaml", ".json", ".yml"]
            .iter()
            .any(|ext| result.to_lowercase().ends_with(ext));
        result.replace("\\", "/")
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SendData {
    pub text: String,
    pub path: Pathlib,
    pub file_label: String,
    pub file_metadata: String,
    pub status_text: String,
    pub tab: String,
    pub rstb_paths: Vec<serde_json::Value>,
    pub sarc_paths: SarcPaths,
    pub lang: String,
    pub compare_data: DiffComparer,
}

impl Default for SendData {
    fn default() -> Self {
        Self {
            text: "".to_string(),
            path: Pathlib::default(),
            file_label: "".to_string(),
            file_metadata: "".to_string(),
            status_text: "".to_string(),
            tab: "YAML".to_string(),
            rstb_paths: Vec::default(),
            sarc_paths: SarcPaths::default(),
            lang: "yaml".to_string(),
            compare_data: DiffComparer::default(),
        }
    }
}
impl SendData {
    pub fn get_file_label(&mut self, filetype: TotkFileType, endian: Option<roead::Endian>) {
        self.set_file_metadata(filetype, None);
        let mut e = String::new();
        if let Some(endian) = endian {
            e = match endian {
                roead::Endian::Big => "BE".to_string(),
                roead::Endian::Little => "LE".to_string(),
            };
        }
        if !e.is_empty() {
            self.file_label = format!("{} [{:?}] [{}]", self.path.name, filetype, e)
        } else {
            self.file_label = format!("{} [{:?}]", self.path.name, filetype)
        }
    }

    pub fn set_file_metadata(
        &mut self,
        filetype: TotkFileType,
        dictionary: Option<ZstdDictionary>,
    ) {
        self.file_metadata = match dictionary {
            Some(dictionary) => format!("[{filetype:?}] [ZSTD: {dictionary:?}]"),
            None => format!("[{filetype:?}]"),
        };
    }
    pub fn get_sarc_paths(&mut self, pack: &PackComparer<'_>) {
        if let Some(opened) = &pack.opened {
            for file in opened.sarc.files() {
                if let Some(name) = file.name {
                    self.sarc_paths.paths.push(name.into());
                }
            }
            for (path, _) in pack.added.iter() {
                self.sarc_paths.added_paths.push(path.into());
            }
            for (path, _) in pack.modded.iter() {
                self.sarc_paths.modded_paths.push(path.into());
            }
            self.sarc_paths
                .paths
                .sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));

            if self.sarc_paths.paths.len() == self.sarc_paths.added_paths.len() {
                self.sarc_paths.added_paths.clear(); //avoid all files be lit blue as added
                self.sarc_paths.modded_paths.clear(); //redundant
                return; //skip sorting empty lists
            }

            self.sarc_paths
                .added_paths
                .sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
            self.sarc_paths
                .modded_paths
                .sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
        }
        //println!("Sarc paths: {:?}", self.sarc_paths);
    }
}

pub fn file_from_disk_to_senddata<P: AsRef<Path>>(
    path: P,
    zstd: Arc<TotkZstd>,
) -> Option<(OpenedFile, SendData)> {
    let file_name = path.as_ref(); //.to_string_lossy().to_string().replace("\\", "/");
    let is_xlink_file = is_xlink_path(file_name)
        || fs::read(file_name)
            .map(|contents| is_xlink(&contents))
            .unwrap_or(false);
    let res = if is_xlink_file {
        Xlink_rs::open_xlink(file_name, zstd.clone())
    } else {
        None
    }
    // .or_else(|| GameDataList::open(&file_name, zstd.clone()))
    .or_else(|| TagProduct::open_tag(&file_name, zstd.clone()))
    .or_else(|| Esetb::open_esetb(&file_name, zstd.clone()))
    .or_else(|| Restbl::open_restbl(&file_name, zstd.clone()))
    .or_else(|| AsbFile::open_asb(&file_name, zstd.clone()))
    .or_else(|| AinbFile::open_ainb(&file_name, zstd.clone()))
    .or_else(|| BymlFile::open_byml(&file_name, zstd.clone()))
    .or_else(|| crate::file_format::Msbt::MsbtFile::open_msbt(&file_name))
    .or_else(|| crate::file_format::SimpleOpeners::AampFile::open_aamp(&file_name))
    .or_else(|| BfevFile::open_bfev(&file_name, zstd.clone()))
    .or_else(|| SmoSaveFile::open_smo_save_file(&file_name, zstd.clone()))
    .or_else(|| crate::file_format::SimpleOpeners::TextFile::open_text(&file_name))
    .map(|(opened_file, data)| {
        // self.opened_file = opened_file;
        // self.internal_file = None;
        (opened_file, data)
    });
    res
}
