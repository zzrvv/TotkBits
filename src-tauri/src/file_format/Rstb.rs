#![allow(non_snake_case, non_camel_case_types)]
// use std::any;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::file_format::BinTextFile::OpenedFile;
use crate::parser::rstb::ResourceSizeTable;
use crate::Open_and_Save::SendData;
use crate::Settings::Magic;
use crate::Settings::{list_files_recursively, Pathlib};
use crate::Zstd::{TotkFileType, TotkZstd, ZstdDictionary};
// use serde_json::to_string_pretty;

use super::Pack::PackFile;

// use super::RstbData::get_rstb_data;

#[allow(dead_code)]
pub struct Restbl<'a> {
    pub path: Pathlib,
    pub zstd: Arc<TotkZstd<'a>>,
    // buffer: Arc<Vec<u8>>, // Use Arc to share ownership
    pub table: ResourceSizeTable,
    pub hash_table: Arc<Vec<String>>,
    pub compression: Option<ZstdDictionary>,
}

impl<'a> Restbl<'_> {
    pub fn cached_path(&self, path: &str) -> Option<&str> {
        self.hash_table
            .iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(path))
            .map(String::as_str)
    }

    pub fn open_restbl<P: AsRef<Path>>(
        path: P,
        zstd: Arc<TotkZstd<'a>>,
    ) -> Option<(
        super::BinTextFile::OpenedFile<'a>,
        crate::Open_and_Save::SendData,
    )> {
        let mut opened_file = OpenedFile::default();
        let path_ref = path.as_ref();
        let mut data = SendData::default();
        println!("[RSTB] route: considering {}", path_ref.display());
        let pathlib_var = Pathlib::new(path_ref);
        let recognized_name = pathlib_var
            .name
            .to_lowercase()
            .starts_with("resourcesizetable.product");
        println!(
            "[RSTB] route: file name {:?}, ResourceSizeTable.Product prefix match={}",
            pathlib_var.name, recognized_name
        );
        if recognized_name {
            println!("[RSTB] route: dispatching to Restbl::from_path");
            opened_file.restbl = Restbl::from_path(path_ref, zstd.clone());
            if let Some(_restbl) = &mut opened_file.restbl {
                println!("[RSTB] route: RSTB opened successfully");
                data.tab = "RSTB".to_string();
                opened_file.path = pathlib_var.clone();
                opened_file.endian = Some(roead::Endian::Little);
                opened_file.file_type = TotkFileType::Restbl;
                data.status_text = format!("Opened {}", &pathlib_var.full_path);
                data.path = pathlib_var;
                // data.text = restbl.to_text();
                data.get_file_label(TotkFileType::Restbl, Some(roead::Endian::Little));
                return Some((opened_file, data));
            }
            println!("[RSTB] route: Restbl::from_path rejected the file");
        }
        println!("[RSTB] route: not handled as RSTB");
        None
    }

    pub fn get_restb_entries<P: AsRef<Path>>(&mut self, path: P) -> io::Result<Arc<Vec<String>>> {
        let mut res = crate::LookupData::rstb_paths();
        let mut p = PathBuf::from(path.as_ref());
        for _ in 0..3 {
            if !p.pop() {
                return Ok(res); //unable to go to mod romfs path
            }
        }
        let mod_romfs_path = p.to_string_lossy().to_string().replace("\\", "/");
        //No point in updating from romfs dump
        if mod_romfs_path == self.zstd.totk_config.romfs {
            return Ok(res);
        }
        let mod_romfs_path_len = mod_romfs_path.len();
        //limit to map files and actors
        let valid_paths = vec!["Pack/Actor", "AI", "AS"];
        for entry in valid_paths.iter() {
            //no point in calling res.contains() since the check costs more time than
            //adding redundant local path
            let valid_path = PathBuf::from(&mod_romfs_path).join(entry);
            if !valid_path.exists() {
                continue;
            }
            for file in list_files_recursively(&valid_path) {
                // println!("  {}", &file);
                let mut local_path = file.replace("\\", "/")[mod_romfs_path_len..].to_string();
                if local_path.starts_with("/") {
                    local_path = local_path[1..].to_string()
                }
                let local_path_lower = local_path.to_ascii_lowercase();
                if local_path.to_ascii_lowercase().ends_with(".zs") {
                    local_path = local_path[..(local_path.len() - 3)].to_string()
                }
                // if !res.contains(&local_path) {
                // println!("Adding custom rstb path: {}", &local_path);
                // }
                if entry == &"Pack/Actor"
                    && (local_path_lower.ends_with(".pack") || local_path_lower.ends_with(".sarc"))
                {
                    if let Ok(pack) = PackFile::new(&file, self.zstd.clone()) {
                        for entry in pack.sarc.files() {
                            let entry_path = entry.name.unwrap_or_default().to_string();
                            // if !entry_path.is_empty() && !res.contains(&entry_path) {
                            if !entry_path.is_empty() {
                                // println!("Adding custom rstb sarc path: {}", &entry_path);
                                Arc::make_mut(&mut res).push(entry_path);
                            }
                        }
                    }
                } else {
                    Arc::make_mut(&mut res).push(local_path);
                }
            }
        }

        Ok(res)
    }

    pub fn from_path<P: AsRef<Path>>(path: P, zstd: Arc<TotkZstd<'_>>) -> Option<Restbl> {
        let mut f_handle = File::open(&path).ok()?;
        let mut buffer = Vec::new();
        f_handle.read_to_end(&mut buffer).ok()?;
        let (buffer, compression) = zstd.try_decompress_all_ordered_safe(&buffer, &path);
        if !Magic::is_restbl(&buffer) {
            return None; //invalid rstb
        }

        println!("[RSTB] native parser: parsing {} bytes", buffer.len());
        match ResourceSizeTable::from_bytes(&buffer) {
            Ok(t) => {
                println!("[RSTB] native parser: parse succeeded");
                // let hash_table = get_rstb_data().unwrap_or_default();

                let mut new_restbl = Restbl {
                    path: Pathlib::new(&path),
                    zstd: zstd.clone(),
                    table: t,
                    hash_table: Default::default(),
                    compression: (compression != ZstdDictionary::None).then_some(compression),
                };
                //TODO: check if self function works
                new_restbl.hash_table = new_restbl.get_restb_entries(&path).unwrap_or_default();
                return Some(new_restbl);
            }
            Err(err) => {
                println!("[RSTB] native parser: parse failed: {err}");
                eprintln!("{:?}", err);
            }
        }
        // return Err(io::Error::new(io::ErrorKind::InvalidData, ""));
        None
    }

    pub fn save_default(&mut self) -> io::Result<()> {
        self.save(&self.path.full_path.clone())
    }

    pub fn save(&mut self, path: &str) -> io::Result<()> {
        let mut buffer = self
            .table
            .to_bytes()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if let Some(compression) = self.compression {
            buffer = self.zstd.compress_with_dictionary(&buffer, compression)?;
        }
        if buffer.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "refusing to save an empty RSTB",
            ));
        }
        let mut f = File::create(&path)?;
        f.write_all(&buffer)?;
        Ok(())
    }
}
