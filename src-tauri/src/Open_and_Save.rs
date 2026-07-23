use crate::{
    file_format::{
        asb::AsbFile,
        msbt::MsbtFile,
        Ainb::AinbFile,
        BfevFile::BfevFile,
        BinTextFile::{is_banc_path, replace_rotate_deg_to_rad, BymlFile, OpenedFile},
        Esetb::Esetb,
        GameDataList::GameDataList,
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
        is_aamp, is_ainb, is_asb, is_byml, is_esetb, is_evfl, is_gamedatalist, is_tagproduct,
        TotkFileType, TotkZstd, ZstdDictionary,
    },
};
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
    let (data, dictionary) = if data.starts_with(b"Yaz0") || crate::Zstd::is_zstd(&data) {
        let (decoded, dictionary) = zstd.try_decompress_for_path(&path, &data).ok()?;
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
    // if is_gamedatalist(&path) {
    //     if let Ok(text) = GameDataList::binary_to_text(&data, zstd.clone()) {
    //         internal_file.endian = BymlFile::get_endiannes(&data);
    //         internal_file.path = Pathlib::new(path.clone());
    //         internal_file.file_type = TotkFileType::Byml;
    //         return Some((internal_file, text));
    //     }
    //     println!("Unable to parse GameDataList archive entry {path}");
    //     return None;
    // }

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
    if let Some(xlink) = Xlink_rs::open_internal(&path, &data, zstd.clone()) {
        return Some(xlink);
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
        let pio = ParameterIO::from_binary(&data).ok()?;
        let text = crate::file_format::bphcl::safe_aamp_yaml(&pio).ok()?;
        internal_file.endian = None;
        internal_file.path = Pathlib::new(path.clone());
        internal_file.file_type = TotkFileType::Aamp;
        return Some((internal_file, text));
    }
    if let Some(msbt) = MsbtFile::open_internal(&path, &data) {
        return Some(msbt);
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
        if !&zstd.totk_config.romfs.is_empty() && dest_file.starts_with(&zstd.totk_config.romfs) {
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
    internal_msbt: Option<&crate::parser::msbt::Msbt>,
    is_internal: bool,
) -> Option<Vec<u8>> {
    let mut rawdata: Vec<u8> = Vec::new();
    let is_zs = file_path.to_lowercase().ends_with(".zs") && zstd_dictionary.is_none();
    let is_bcett = file_path.to_lowercase().ends_with(".bcett.byml.zs");
    match file_type {
        TotkFileType::Bphcl => {
            rawdata = opened_file.bphcl.as_ref()?.raw_binary();
        }
        TotkFileType::Hkcl => return None,
        TotkFileType::Bphhb => return None,
        TotkFileType::Xlink => {
            rawdata = Xlink_rs::text_to_binary(text, file_path, zstd.clone(), zstd_dictionary)?;
        }
        TotkFileType::Evfl => {
            rawdata = match opened_file.bfev.as_ref() {
                Some(file) => file.save_text(text).ok()?,
                None => BfevFile::text_to_binary(text).ok()?,
            };
            if is_zs {
                rawdata = zstd.compress_zs(&rawdata).ok()?;
            }
        }
        TotkFileType::Esetb => {
            if let Some(esetb) = &mut opened_file.esetb {
                esetb.update_from_text(text).ok()?;
                rawdata = esetb.to_binary();
                if file_path.to_lowercase().ends_with(".zs") {
                    rawdata = zstd.compress_zs(&rawdata).ok()?;
                }
            }
        }
        TotkFileType::ASB => {
            if let Ok(some_data) = AsbFile::text_to_binary(text, Some(opened_file)) {
                rawdata = some_data;
                if is_zs {
                    rawdata = zstd.compress_zs(&rawdata).ok()?;
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
                    rawdata = zstd.compress_zs(&rawdata).ok()?;
                }
            }
        }
        TotkFileType::Byml => {
            if is_gamedatalist(file_path) {
                rawdata = GameDataList::text_to_binary(text)
                    .map_err(|e| println!("Unable to encode GameDataList: {e}"))
                    .ok()?;
            }
            if rawdata.is_empty() {
                let processed_text = if is_banc_path(&file_path) && zstd.totk_config.rotation_deg {
                    &replace_rotate_deg_to_rad(&text)
                } else {
                    text
                };

                let pio = Byml::from_text(processed_text).ok()?;
                rawdata = pio.to_binary(endian);
            }
            if !rawdata.is_empty() {
                if is_bcett {
                    rawdata = zstd.compress_bcett(&rawdata).ok()?;
                } else if is_zs {
                    rawdata = zstd.compress_zs(&rawdata).ok()?;
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
                rawdata = zstd.compress_bcett(&rawdata).ok()?;
            }
        }
        TotkFileType::Msbt => {
            rawdata = MsbtFile::text_to_binary(
                text,
                file_path,
                internal_msbt.or(opened_file.msyt.as_ref()),
                is_internal,
            )?;
        }
        TotkFileType::Aamp => {
            let pio = crate::file_format::bphcl::aamp_from_yaml(text).ok()?;
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
                if self.opened_file.bphcl.is_some() {
                    self.name = Some(self.opened_file.path.name.clone());
                } else if let Some(pack) = self.pack {
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
                if self.opened_file.bphcl.is_some() {
                    filters.insert("BPHCL".to_string(), vec!["bphcl".to_string()]);
                } else {
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
    pub read_only: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_modded_path: Option<String>,
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
            read_only: false,
            parent_modded_path: None,
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
            Some(ZstdDictionary::Yaz0) => format!("[{filetype:?}] [Yaz0]"),
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

pub fn open_file_from_disk_name_guess<P: AsRef<Path>>(
    path: P,
    zstd: Arc<TotkZstd>,
) -> Option<(OpenedFile, SendData)> {
    let path = path.as_ref();
    let file_name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    let uncompressed_name = file_name.strip_suffix(".zs").unwrap_or(&file_name);

    let result = if file_name.starts_with("tag.product") {
        TagProduct::open_tag(path, zstd)
    } else if file_name.ends_with(".belnk") || file_name.ends_with(".belnk.zs") {
        Xlink_rs::open_xlink(path, zstd)
    } else if file_name.ends_with(".esetb.byml") || file_name.ends_with(".esetb.byml.zs") {
        Esetb::open_esetb(path, zstd)
    } else if uncompressed_name.ends_with(".rsizetable")
        || uncompressed_name.ends_with(".restbl")
        || uncompressed_name.ends_with(".rstb")
        || uncompressed_name.ends_with(".rsizetable")
        || uncompressed_name.ends_with(".rstbl.byml")
    {
        Restbl::open_restbl(path, zstd)
    } else if uncompressed_name.ends_with(".asb") {
        AsbFile::open_asb(path, zstd)
    } else if uncompressed_name.ends_with(".ainb") {
        AinbFile::open_ainb(path, zstd)
    } else if uncompressed_name.ends_with(".bfevfl") {
        BfevFile::open_bfev(path, zstd)
    } else if uncompressed_name.ends_with(".msbt") || uncompressed_name.ends_with(".msyt") {
        MsbtFile::open_mstb(path)
    } else if uncompressed_name.ends_with(".bphcl") {
        crate::file_format::bphcl::BphclFile::open(path)
    } else if uncompressed_name.ends_with(".hkcl") {
        crate::file_format::hkcl::HkclFile::open(path)
    } else if uncompressed_name.ends_with(".bphhb") {
        crate::file_format::bphhb::BphhbFile::open(path)
    } else if uncompressed_name.ends_with(".bfres") || file_name.ends_with(".bfres.mc") {
        crate::file_format::Model3D::bfres::BfresFile::open(path)
    } else if uncompressed_name.ends_with(".fbx") {
        crate::parser::fbx::FbxFile::open(path)
    } else if uncompressed_name.ends_with(".bntx")
        || uncompressed_name.ends_with(".dds")
        || uncompressed_name.ends_with(".png")
        || uncompressed_name.ends_with(".jpg")
        || uncompressed_name.ends_with(".jpeg")
        || uncompressed_name.ends_with(".tga")
        || uncompressed_name.ends_with(".bmp")
    {
        crate::file_format::Image::ImageDocument::open(path, &zstd)
    } else if uncompressed_name.ends_with(".baamp")
        || uncompressed_name.ends_with(".bparam")
        || uncompressed_name.ends_with(".aamp")
    {
        crate::file_format::SimpleOpeners::AampFile::open_aamp(path)
    } else if uncompressed_name.ends_with(".byml")
        || uncompressed_name.ends_with(".bgyml")
        || uncompressed_name.ends_with(".sbyml")
        || uncompressed_name.starts_with("gamedatalist")
    {
        BymlFile::open_byml(path, zstd)
    } else if uncompressed_name.ends_with(".yaml")
        || uncompressed_name.ends_with(".yml")
        || uncompressed_name.ends_with(".json")
        || uncompressed_name.ends_with(".txt")
    {
        crate::file_format::SimpleOpeners::TextFile::open_text(path)
    } else {
        None
    };

    if let Some((opened_file, _)) = &result {
        println!(
            "[GUESS] [OPEN] {} type: {:?}",
            opened_file.path.full_path, opened_file.file_type
        );
    } else {
        println!("[GUESS] [OPEN] FAILED TO OPEN {}", path.display());
    }

    result
}

pub fn file_from_disk_to_senddata<P: AsRef<Path>>(
    path: P,
    zstd: Arc<TotkZstd>,
) -> Option<(OpenedFile, SendData)> {
    let file_name = path.as_ref(); //.to_string_lossy().to_string().replace("\\", "/");
    println!(
        "[OPEN ROUTER] beginning disk opener chain for {} (RSTB follows Xlink, TagProduct, and Esetb)",
        file_name.display()
    );
    open_file_from_disk_name_guess(file_name, zstd.clone())
        .or_else(|| Xlink_rs::open_xlink(file_name, zstd.clone()))
        // .or_else(|| GameDataList::open(&file_name, zstd.clone()))
        .or_else(|| TagProduct::open_tag(&file_name, zstd.clone()))
        .or_else(|| Esetb::open_esetb(&file_name, zstd.clone()))
        .or_else(|| Restbl::open_restbl(&file_name, zstd.clone()))
        .or_else(|| AsbFile::open_asb(&file_name, zstd.clone()))
        .or_else(|| AinbFile::open_ainb(&file_name, zstd.clone()))
        .or_else(|| BymlFile::open_byml(&file_name, zstd.clone()))
        .or_else(|| MsbtFile::open_mstb(file_name))
        .or_else(|| crate::file_format::Model3D::bfres::BfresFile::open(file_name))
        .or_else(|| crate::parser::fbx::FbxFile::open(file_name))
        // Structured formats must run before the generic image detector. In
        // particular, many BFEVFL files use the shared `.zs` compression suffix.
        .or_else(|| BfevFile::open_bfev(&file_name, zstd.clone()))
        .or_else(|| crate::file_format::Image::ImageDocument::open(file_name, &zstd))
        .or_else(|| crate::file_format::bphcl::BphclFile::open(file_name))
        .or_else(|| crate::file_format::hkcl::HkclFile::open(file_name))
        .or_else(|| crate::file_format::bphhb::BphhbFile::open(file_name))
        .or_else(|| crate::file_format::SimpleOpeners::AampFile::open_aamp(&file_name))
        .or_else(|| SmoSaveFile::open_smo_save_file(&file_name, zstd.clone()))
        .or_else(|| crate::file_format::SimpleOpeners::TextFile::open_text(&file_name))
        .map(|(opened_file, data)| {
            println!(
                "[OPEN ROUTER] opener chain accepted {} as {:?}",
                file_name.display(),
                opened_file.file_type
            );
            // self.opened_file = opened_file;
            // self.internal_file = None;
            (opened_file, data)
        })
}
