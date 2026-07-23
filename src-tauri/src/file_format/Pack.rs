#![allow(non_snake_case, non_camel_case_types)]
use flate2::read::ZlibDecoder;
use roead;
use roead::sarc::{Sarc, SarcWriter};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

//mod Zstd;

use crate::Open_and_Save::SendData;
use crate::Settings::{exe_relative_path, makedirs, Pathlib};
use crate::TotkConfig::TotkConfig;
use crate::Zstd::{is_sarc, sha256, TotkFileType, TotkZstd};

// use super::SarcEntriesData::get_sarc_entries_data;

pub fn get_sarc_entries_data() -> io::Result<HashMap<String, String>> {
    //parse json
    println!("Getting global sarc data ");
    let json_zlibdata = fs::read(exe_relative_path("bin/totk_sarc_sha256.bin"))?;
    let mut decoder = ZlibDecoder::new(&json_zlibdata[..]);
    let mut json_str = String::new();
    decoder.read_to_string(&mut json_str)?;
    let res: HashMap<String, String> = serde_json::from_str(&json_str)?;
    Ok(res)
}

pub struct PackComparer<'a> {
    pub opened: Option<PackFile<'a>>,
    pub vanila: Option<PackFile<'a>>,
    // pub totk_config: Arc<TotkConfig>,
    pub zstd: Arc<TotkZstd<'a>>,
    pub added: HashMap<String, String>,
    pub modded: HashMap<String, String>,
    pub global_sarc_data: HashMap<String, String>,
}

#[allow(dead_code)]
impl<'a> PackComparer<'a> {
    pub fn open_sarc<P: AsRef<Path>>(
        path: P,
        zstd: Arc<TotkZstd<'a>>,
    ) -> Option<(Self, crate::Open_and_Save::SendData)> {
        let mut data = SendData::default();
        let path_ref = path.as_ref();
        print!("Is {:?} a sarc? ", &path_ref.display());
        let pathlib_var = Pathlib::new(path_ref);
        if let Ok(sarc) = PackFile::new(path_ref, zstd.clone()) {
            let e_s = if sarc.endian == roead::Endian::Little {
                " [LE]"
            } else {
                " [BE]"
            };
            let yaz0_s = if sarc.is_yaz0 { " [Yaz0] " } else { " " };
            if let Some(pack) = PackComparer::from_pack(sarc, zstd.clone()) {
                println!(" yes!");
                data.get_sarc_paths(&pack);
                data.status_text =
                    format!("Opened {}", &path_ref.to_string_lossy().replace("\\", "/"));
                data.path = pathlib_var.clone();
                data.tab = "SARC".to_string();
                data.file_label = format!("{}{}[SARC]{}", &pathlib_var.name, yaz0_s, e_s);
                return Some((pack, data));
            }
        }
        println!(" no");

        None
    }
    pub fn from_pack(pack: PackFile<'a>, zstd: Arc<TotkZstd<'a>>) -> Option<Self> {
        let config = zstd.clone().totk_config.clone();
        // let vanila = PackComparer::get_vanila_sarc(&pack.path, zstd.clone());
        //Set to None - rely only on global_sarc_data (except for mals files)
        let mut vanila = PackComparer::get_vanila_mals(&pack.path, zstd.clone());
        if vanila.is_none() {
            if let Some(vanila_path) = config.get_pack_path(&pack.path.stem) {
                if let Ok(van) = PackFile::new(vanila_path, zstd.clone()) {
                    vanila = Some(van);
                }
            }
        }
        let mut pack = Self {
            opened: Some(pack),
            vanila: vanila,
            // totk_config: config,
            zstd: zstd.clone(),
            added: HashMap::default(),
            modded: HashMap::default(),
            global_sarc_data: HashMap::default(),
        };
        println!("Comparing and reloading");
        if let Err(error) = pack.compare_and_reload() {
            eprintln!("Failed to initialize SARC comparison: {error}");
            return None;
        }
        Some(pack)
    }

    pub fn extract_all_to_folder<P: AsRef<Path>>(&self, dest_path: P) -> io::Result<String> {
        let mut p = PathBuf::from(dest_path.as_ref());
        if let Some(pack) = &self.opened {
            let mut i: i32 = 0;
            let name = pack.path.stem.as_str();
            p.push(name);
            fs::create_dir_all(&p)?;
            for file in pack.sarc.files() {
                if let Some(file_name) = file.name() {
                    let mut file_path = p.clone();
                    file_path.push(file_name);
                    makedirs(&file_path)?;
                    fs::write(&file_path, file.data)?;
                    i += 1;
                }
            }

            return Ok(format!("Extracted {} files to {}", i, p.display()));
        }

        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "No opened pack",
        ));
    }

    pub fn extract_folder_to<P: AsRef<Path>>(
        &mut self,
        source_folder: &str,
        dest_path: P,
    ) -> io::Result<String> {
        let paths = self.get_sarc_paths().paths;
        let opened = self
            .opened
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "No opened pack"))?;
        let prefix = source_folder.trim_matches(['/', '\\']);
        let prefix_with_separator = if prefix.is_empty() {
            String::new()
        } else {
            format!("{}/", prefix.replace('\\', "/"))
        };
        // When extracting a selected directory, make that directory the root of
        // the result instead of recreating the SARC name and its full parent path.
        // Full-archive extraction has its own path and intentionally keeps the
        // existing `<destination>/<sarc name>/...` layout.
        let output_root = if prefix.is_empty() {
            dest_path.as_ref().join(&opened.path.stem)
        } else {
            let folder_name = prefix
                .rsplit('/')
                .next()
                .filter(|name| !name.is_empty())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "Invalid SARC folder path")
                })?;
            dest_path.as_ref().join(folder_name)
        };
        let mut count = 0;

        for path in paths {
            let normalized = path.replace('\\', "/");
            if !prefix_with_separator.is_empty()
                && normalized != prefix
                && !normalized.starts_with(&prefix_with_separator)
            {
                continue;
            }
            if let Some(bytes) = opened.writer.get_file(&path) {
                let relative_path = if prefix_with_separator.is_empty() {
                    normalized.as_str()
                } else {
                    normalized
                        .strip_prefix(&prefix_with_separator)
                        .unwrap_or(normalized.as_str())
                };
                let output_path = output_root.join(relative_path);
                makedirs(&output_path)?;
                fs::write(output_path, bytes)?;
                count += 1;
            }
        }
        Ok(format!(
            "Extracted {} files to {}",
            count,
            output_root.display()
        ))
    }

    pub fn get_sarc_paths(&self) -> SarcPaths {
        let mut paths = SarcPaths::default();
        if let Some(opened) = &self.opened {
            for file in opened.sarc.files() {
                if let Some(name) = file.name {
                    paths.paths.push(name.to_string());
                }
            }
            for (path, _) in self.added.iter() {
                paths.added_paths.push(path.to_string());
            }
            for (path, _) in self.modded.iter() {
                paths.modded_paths.push(path.to_string());
            }
            let size = paths.paths.len();
            if size == paths.added_paths.len() {
                paths.added_paths.clear();
                paths.modded_paths.clear();
            }
        }
        // println!("Paths: {:?}", paths);
        paths
    }

    pub fn compare_and_reload(&mut self) -> io::Result<()> {
        if let Some(opened) = &mut self.opened {
            opened.reload()?;
            opened.self_populate_hashes();
            // println!("hasehs {:?}\nNow to compare", opened.hashes);
        }
        // println!("Comparing, ready...");
        self.compare();
        Ok(())
    }

    pub fn compare(&mut self) {
        println!("Comparing");
        if let Some(opened) = &self.opened {
            let mut is_compared = false;
            if let Some(vanila) = &mut self.vanila {
                //unreachable unless mals
                println!("Comparing vanila actor");
                let mut added: HashMap<String, String> = HashMap::default();
                let mut modded: HashMap<String, String> = HashMap::default();
                vanila.self_populate_hashes();
                for (file, hash) in opened.hashes.iter() {
                    let van_hash = vanila.hashes.get(file);
                    match van_hash {
                        Some(h) => {
                            if h != hash {
                                modded.insert(file.to_string(), hash.to_string());
                            }
                        }
                        None => {
                            added.insert(file.to_string(), hash.to_string());
                        }
                    }
                }
                self.added = added;
                self.modded = modded;
                is_compared = self.added.len() != opened.hashes.keys().len();
                // println!("Added {:?}\nModded {:?}", self.added.keys(), self.modded.keys());
            }
            if !is_compared {
                //custom actor
                println!("Comparing custom actor");
                if self.global_sarc_data.is_empty() {
                    self.global_sarc_data = get_sarc_entries_data().unwrap_or_default();
                }
                let mut added: HashMap<String, String> = HashMap::default();
                let mut modded: HashMap<String, String> = HashMap::default();
                for (file, hash) in opened.hashes.iter() {
                    let van_hash = self.global_sarc_data.get(file);
                    match van_hash {
                        Some(h) => {
                            if h != hash {
                                modded.insert(file.to_string(), hash.to_string());
                            }
                        }
                        None => {
                            added.insert(file.to_string(), hash.to_string());
                        }
                    }
                }
                self.added = added;
                self.modded = modded;
                // println!("Added {:?}\nModded {:?}", self.added, self.modded);
            }
        }
    }

    pub fn get_vanila_mals(path: &Pathlib, zstd: Arc<TotkZstd<'a>>) -> Option<PackFile<'a>> {
        println!("Getting the mals: {}", &path.stem);
        // let versions: Vec<usize> = (100..130).collect();//130, in case new updates are issued in the future for TOTK
        let romfs = &zstd.clone().totk_config.romfs;
        for version in (99..=130).rev() {
            let mut prob_mals_path = PathBuf::from(romfs);
            prob_mals_path.push(format!("Mals/{}.Product.{}.sarc.zs", &path.stem, version));
            if prob_mals_path.exists() {
                if let Ok(pack) =
                    PackFile::new(prob_mals_path.to_string_lossy().to_string(), zstd.clone())
                {
                    println!("Got the mals! Version: {}", version);
                    return Some(pack);
                }
            }
        }

        // let path = zstd.clone().totk_config.clone().get_mals_path(&path.name);
        // if let Some(path) = &path {
        //     match PackFile::new(path.to_string_lossy().to_string(), zstd.clone()) {
        //         Ok(pack) => {
        //             println!("Got the mals!");
        //             return Some(pack);
        //         }
        //         _ => {
        //             return None;
        //         }
        //     }
        // }
        None
    }
    #[allow(dead_code)]
    pub fn get_vanila_sarc(path: &Pathlib, zstd: Arc<TotkZstd<'a>>) -> Option<PackFile<'a>> {
        let pack = PackComparer::get_vanila_pack(path, zstd.clone());
        if pack.is_some() {
            return pack;
        }
        let mals = PackComparer::get_vanila_mals(path, zstd.clone());
        if mals.is_some() {
            return mals;
        }
        None
    }
    #[allow(dead_code)]
    pub fn get_vanila_pack(path: &Pathlib, zstd: Arc<TotkZstd<'a>>) -> Option<PackFile<'a>> {
        println!("Getting the pack: {}", &path.name);
        let path = zstd.clone().totk_config.clone().get_pack_path(&path.stem);
        if let Some(path) = &path {
            match PackFile::new(path.to_string_lossy().to_string(), zstd.clone()) {
                Ok(pack) => {
                    return Some(pack);
                }
                _ => {
                    return None;
                }
            }
        }
        None
    }
}

pub struct PackFile<'a> {
    pub path: Pathlib,
    pub totk_config: Arc<TotkConfig>,
    zstd: Arc<TotkZstd<'a>>,
    pub data: Vec<u8>,
    pub file_type: TotkFileType,
    pub endian: roead::Endian,
    pub writer: SarcWriter,
    pub hashes: HashMap<String, String>,
    pub sarc: Sarc<'a>,
    pub is_yaz0: bool,
    pub yaz0_alignment: u32,
    pub dirty: bool,
}

#[allow(dead_code)]
impl<'a> PackFile<'_> {
    pub fn default(zstd: Arc<TotkZstd<'a>>) -> io::Result<PackFile<'a>> {
        let mut writer = SarcWriter::new(roead::Endian::Little);
        let sarc = Sarc::new(writer.to_binary().clone())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(PackFile {
            path: Pathlib::default(),
            totk_config: zstd.totk_config.clone(),
            zstd: zstd.clone(),
            data: Default::default(),
            file_type: TotkFileType::Sarc,
            endian: roead::Endian::Little,
            writer: writer,
            hashes: HashMap::default(),
            sarc: sarc,
            is_yaz0: false,
            yaz0_alignment: 0,
            dirty: false,
        })
    }

    pub fn new<P: AsRef<Path>>(
        path: P,
        //totk_config: Arc<TotkConfig>,
        zstd: Arc<TotkZstd<'a>>,
        //decompressor: &'a ZstdDecompressor,
        //compressor: &'a ZstdCompressor
    ) -> io::Result<PackFile<'a>> {
        let mut pack = Self::default(zstd.clone())?;
        pack.sarc_file_to_bytes(&path)?;
        // pack.sarc = Sarc::new(&pack.data).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        pack.writer = SarcWriter::from_sarc(&pack.sarc);
        pack.endian = pack.sarc.endian();
        pack.path = Pathlib::new(path.as_ref());
        Ok(pack)
    }

    pub fn rename(&mut self, old_name: &str, new_name: &str) -> io::Result<()> {
        let some_data = self.writer.get_file(old_name).cloned();
        match some_data {
            Some(data) => {
                self.mutate_writer(|writer| {
                    writer.add_file(new_name, data);
                    writer.remove_file(old_name);
                })?;
            }
            None => {
                let e = format!("File {} absent in sarc {}", &old_name, &self.path.full_path);
                return Err(io::Error::new(io::ErrorKind::InvalidInput, e));
            }
        }
        Ok(())
    }

    pub fn mutate_writer(&mut self, operation: impl FnOnce(&mut SarcWriter)) -> io::Result<()> {
        let mut candidate_writer = self.writer.clone();
        operation(&mut candidate_writer);
        let candidate_sarc = Sarc::new(candidate_writer.to_binary())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let candidate_hashes = PackFile::populate_hashes(&candidate_sarc);
        self.writer = candidate_writer;
        self.sarc = candidate_sarc;
        self.hashes = candidate_hashes;
        self.dirty = true;
        Ok(())
    }

    pub fn reload(&mut self) -> io::Result<()> {
        let data: Vec<u8> = self.writer.to_binary();
        self.sarc =
            Sarc::new(data).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(())
    }

    pub fn self_populate_hashes(&mut self) {
        self.hashes = PackFile::populate_hashes(&self.sarc);
    }

    pub fn populate_hashes(sarc: &Sarc) -> HashMap<String, String> {
        let mut hashes: HashMap<String, String> = HashMap::default();
        for file in sarc.files() {
            let file_name = file.name.unwrap_or("");
            if !file_name.is_empty() {
                let hash = sha256(file.data().to_vec());
                hashes.insert(file_name.to_string(), hash);
            }
        }
        hashes
    }

    //Save the sarc file, compress if file ends with .zs, create directory if needed
    pub fn save_default(&mut self) -> io::Result<()> {
        let dest_file = self.path.full_path.clone();
        self.save(dest_file)
    }

    fn compress(&self, data: &Vec<u8>) -> io::Result<Vec<u8>> {
        // let zstd = ZstdCppCompressor::from_totk_zstd(self.zstd.clone());
        match self.file_type {
            TotkFileType::Sarc => {
                println!("Compressing SARC");
                // return self.zstd.compressor.compress_pack(data);
                return self.zstd.compress_pack(data);
            }
            TotkFileType::MalsSarc => {
                println!("Compressing MALS SARC");
                // return self.zstd.compressor.compress_zs(data);
                return self.zstd.compress_zs(data);
            }
            _ => {
                return Ok(data.to_vec());
            }
        }
    }

    pub fn save(&mut self, dest_file: String) -> io::Result<()> {
        makedirs(&PathBuf::from(&dest_file))?;
        let raw_data = if self.dirty || self.data.is_empty() {
            self.writer.to_binary()
        } else {
            self.data.clone()
        };
        let mut data = raw_data.clone();
        if dest_file.to_lowercase().ends_with(".zs") {
            data = self.compress(&data)?;
        } else if self.is_yaz0 {
            data = TotkZstd::compress_yaz0_with_alignment(&data, self.yaz0_alignment)?;
        }
        let mut file_handle: fs::File = fs::File::create(dest_file)?;
        file_handle.write_all(&data)?;
        self.data = raw_data;
        self.dirty = false;
        Ok(())
    }
    pub fn sarc_file_to_bytes<P: AsRef<Path>>(&mut self, path: P) -> io::Result<()> {
        let mut f_handle: fs::File = fs::File::open(&path)?;
        let mut buffer: Vec<u8> = Vec::new();
        f_handle.read_to_end(&mut buffer)?;
        if buffer.starts_with(b"Yaz0") {
            self.yaz0_alignment = TotkZstd::yaz0_alignment(&buffer);
            if let Ok(dec_data) = TotkZstd::decompress_yaz0(&buffer) {
                buffer = dec_data;
                self.is_yaz0 = true;
            }
            if is_sarc(&buffer) {
                self.data = buffer.clone();
                self.sarc = Sarc::new(buffer)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
                return Ok(());
            }
        }
        if path
            .as_ref()
            .to_string_lossy()
            .to_lowercase()
            .ends_with(".zs")
        {
            if let Ok(dec_data) = self.zstd.decompress_pack(&buffer) {
                if is_sarc(&dec_data) {
                    self.data = dec_data.clone();
                    self.sarc = Sarc::new(dec_data)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
                    self.file_type = TotkFileType::Sarc;
                    return Ok(());
                }
            }
            if let Ok(dec_data) = self.zstd.decompressor.decompress_zs(&buffer) {
                if is_sarc(&dec_data) {
                    self.data = dec_data.clone();
                    self.sarc = Sarc::new(dec_data)
                        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
                    self.file_type = TotkFileType::MalsSarc;
                    return Ok(());
                }
            }
        }
        if is_sarc(&buffer) {
            self.data = buffer.clone();
            self.sarc =
                Sarc::new(buffer).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
            return Ok(());
        }

        Err(io::Error::new(
            io::ErrorKind::Other,
            "Invalid data, not a sarc",
        ))
    }
    //Read sarc file's bytes, decompress if needed
    // fn sarc_file_to_bytes1(path: &PathBuf, zstd: &'a TotkZstd) -> Result<(bool, FileData), io::Error> {
    //     let mut is_yaz0 = false;
    //     let mut f_handle: fs::File = fs::File::open(path)?;
    //     let mut buffer: Vec<u8> = Vec::new();
    //     //let mut returned_result: Vec<u8> = Vec::new();
    //     let mut file_data: FileData = FileData::new();
    //     file_data.file_type = TotkFileType::Sarc;
    //     f_handle.read_to_end(&mut buffer)?;

    //     if buffer.starts_with(b"Yaz0") {
    //         if let Ok(dec_data) = roead::yaz0::decompress(&buffer) {
    //             buffer = dec_data;
    //             is_yaz0 = true;
    //         }
    //     }
    //     if is_sarc(&buffer) {
    //         //buffer.as_slice().starts_with(b"SARC") {
    //         file_data.data = buffer;
    //         return Ok((is_yaz0, file_data));
    //     }
    //     match zstd.decompressor.decompress_pack(&buffer) {
    //         Ok(res) => {
    //             if is_sarc(&res) {
    //                 file_data.data = res;
    //             }
    //         }
    //         Err(_) => {
    //             match zstd.decompressor.decompress_zs(&buffer) {
    //                 //try decompressing with other dicts
    //                 Ok(res) => {
    //                     if is_sarc(&res) {
    //                         file_data.data = res;
    //                         file_data.file_type = TotkFileType::MalsSarc;
    //                     }
    //                 }
    //                 Err(err) => {
    //                     eprintln!(
    //                         "Error during zstd decompress {}",
    //                         &path.to_str().unwrap_or("UNKNOWN")
    //                     );
    //                     return Err(err);
    //                 }
    //             }
    //         }
    //     }
    //     if is_sarc(&file_data.data) {
    //         return Ok((is_yaz0, file_data));
    //     }
    //     return Err(io::Error::new(
    //         io::ErrorKind::Other,
    //         "Invalid data, not a sarc",
    //     ));
    // }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SarcPaths {
    pub paths: Vec<String>,
    pub added_paths: Vec<String>,
    pub modded_paths: Vec<String>,
    pub nested_paths: HashMap<String, Vec<String>>,
    pub read_only: bool,
}
impl Default for SarcPaths {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            added_paths: Vec::new(),
            modded_paths: Vec::new(),
            nested_paths: HashMap::new(),
            read_only: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untouched_yaz0_sarc_save_as_is_byte_exact() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/_ss/yaz0/MarioZombie.szs");
        if !source.is_file() {
            return;
        }
        let config = Arc::new(TotkConfig::default());
        let zstd = Arc::new(TotkZstd::dictionaryless(
            config,
            crate::Zstd::TOTK_ZSTD_COMPRESSION_LEVEL,
        ));
        let mut pack = PackFile::new(&source, zstd).unwrap();
        let destination =
            std::env::temp_dir().join(format!("totkbits-yaz0-save-as-{}.szs", std::process::id()));

        pack.save(destination.to_string_lossy().into_owned())
            .unwrap();
        let expected = fs::read(&source).unwrap();
        let actual = fs::read(&destination).unwrap();
        let _ = fs::remove_file(destination);
        assert_eq!(actual, expected);
        assert_eq!(
            sha256(actual),
            "CD0EB66CBED6FCA64011E6964CFB7831AF486E1B88E3D399D4375399FE64247E"
        );
    }
}
