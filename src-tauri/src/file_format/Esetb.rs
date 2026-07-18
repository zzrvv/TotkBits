#![allow(non_snake_case, non_camel_case_types)]
use super::BinTextFile::{BymlFile, FileData};
use crate::file_format::BinTextFile::OpenedFile;
use crate::parser::ptcl::Ptcl;
use crate::Open_and_Save::SendData;
use crate::Zstd::is_esetb;
use crate::{
    Settings::Pathlib,
    Zstd::{TotkFileType, TotkZstd},
};
use roead::byml::Byml;
use std::{io, path::Path, sync::Arc};

const PTCL_JSON_KEY: &str = "PTCL_JSON";
const PTCL_BIN_KEY: &str = "PtclBin";

pub struct Esetb<'a> {
    pub byml: BymlFile<'a>,
    pub ptcl: Vec<u8>,
}

#[allow(dead_code)]
impl<'a> Esetb<'a> {
    pub fn open_esetb<P: AsRef<Path>>(
        path: P,
        zstd: Arc<TotkZstd<'a>>,
    ) -> Option<(
        super::BinTextFile::OpenedFile<'a>,
        crate::Open_and_Save::SendData,
    )> {
        let mut opened_file = OpenedFile::default();
        let path_ref = path.as_ref();
        let mut data = SendData::default();
        print!("Is {:?} a esetb? ", &path_ref);
        if is_esetb(&path) {
            opened_file.esetb = Esetb::from_file(path_ref, zstd.clone()).ok();
            if let Some(esetb) = &opened_file.esetb {
                println!(" yes!");
                data.tab = "YAML".to_string();
                opened_file.path = Pathlib::new(path_ref);
                opened_file.endian = esetb.byml.endian;
                opened_file.file_type = TotkFileType::Esetb;
                data.status_text = format!("Opened {}", path_ref.display());
                data.path = Pathlib::new(path_ref);
                data.text = esetb.to_string();
                data.get_file_label(TotkFileType::Esetb, esetb.byml.endian);
                return Some((opened_file, data));
            }
        }
        println!("no");

        None
    }
    pub fn from_binary(data: &Vec<u8>, zstd: Arc<TotkZstd<'a>>) -> io::Result<Esetb<'a>> {
        let file_data = FileData {
            file_type: TotkFileType::Esetb,
            data: data.to_vec(),
        };
        let pio = Byml::from_binary(data).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let mut byml = BymlFile {
            endian: BymlFile::get_endiannes(&file_data.data),
            file_data: file_data,
            path: Pathlib::default(),
            pio: pio,
            zstd: zstd.clone(),
            file_type: TotkFileType::Byml,
        };
        let ptcl = Self::process_ptcl_binary(&mut byml.pio)?;
        Ok(Esetb {
            byml: byml,
            ptcl: ptcl,
        })
    }

    pub fn to_binary(&mut self) -> Vec<u8> {
        serialize_preserving_original(
            &self.byml.pio,
            &self.byml.file_data.data,
            self.byml.endian.unwrap_or(roead::Endian::Little),
        )
    }

    pub fn process_ptcl_binary(pio: &mut Byml) -> io::Result<Vec<u8>> {
        let pio_map = pio.as_mut_map().map_err(io::Error::other)?;
        let ptcl_data = pio_map
            .get(PTCL_BIN_KEY)
            .and_then(|value| match value {
                Byml::FileData(data) => Some(data.clone()),
                _ => None,
            })
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "PtclBin key is not FileData")
            })?;
        let ptcl = Ptcl::parse(&ptcl_data)?;
        let yaml = ptcl.to_yaml()?;
        let yaml_node = Byml::from_text(&yaml).map_err(io::Error::other)?;
        pio_map.insert(PTCL_JSON_KEY.into(), yaml_node);
        Ok(ptcl_data)
    }

    pub fn from_file<P: AsRef<Path>>(file: P, zstd: Arc<TotkZstd<'a>>) -> io::Result<Esetb<'a>> {
        if let Some(byml) = BymlFile::new(file.as_ref(), zstd.clone()) {
            let mut esetb = Esetb {
                byml: byml,
                ptcl: Vec::new(),
            };
            match Self::process_ptcl_binary(&mut esetb.byml.pio) {
                Ok(ptcl) => esetb.ptcl = ptcl,
                Err(e) => {
                    println!("Error while reading PtclBin key: {}", e);
                    return Err(e);
                }
            }
            // esetb.ptcl = Self::process_ptcl_binary(&esetb.byml.pio)?;

            esetb.remove_ptclbin_entry()?;

            return Ok(esetb);
        } else {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "Error while reading BYML file",
            ))
        }
        // let mut byml = BymlFile::new(file.to_string(), zstd.clone()).ok_or(io::Error::new(io::ErrorKind::Other, "Error while reading BYML file"))?;
    }
    pub fn remove_ptclbin_entry(&mut self) -> io::Result<()> {
        if let Ok(pio_map) = self.byml.pio.as_mut_map() {
            if pio_map.contains_key(PTCL_BIN_KEY) {
                pio_map.remove(PTCL_BIN_KEY);
            }
        }
        Ok(())
    }

    pub fn to_string(&self) -> String {
        self.byml.pio.to_text()
    }

    pub fn update_from_text(&mut self, text: &str) -> io::Result<()> {
        self.byml.pio = Byml::from_text(text).map_err(io::Error::other)?;
        self.remove_ptclbin_entry()?;
        if let Ok(pio_map) = self.byml.pio.as_mut_map() {
            if let Some(ptcl_json) = pio_map.get(PTCL_JSON_KEY).cloned() {
                let ptcl_json_str = Byml::to_text(&ptcl_json);
                let ptcl = Ptcl::parse(&self.ptcl)?;
                let new_ptcl_data = ptcl.apply_yaml(&ptcl_json_str)?;
                pio_map.insert(PTCL_BIN_KEY.into(), Byml::FileData(new_ptcl_data));
                pio_map.remove(PTCL_JSON_KEY);
            } else {
                let new_node = Byml::FileData(self.ptcl.clone());
                pio_map.insert(PTCL_BIN_KEY.into(), new_node);
            }
        } else {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Error while converting Ptcl byml as mut map",
            ));
        }

        Ok(())
    }

    pub fn text_to_binary(&mut self, text: &str) -> io::Result<Vec<u8>> {
        self.update_from_text(text)?;
        Ok(self
            .byml
            .pio
            .to_binary(self.byml.endian.unwrap_or(roead::Endian::Little)))
    }

    pub fn save_from_text(&mut self, path: &str, text: &str) -> io::Result<()> {
        self.update_from_text(text)?;
        self.byml.save(path.to_string())?;

        Ok(())
    }
}

pub(crate) fn serialize_preserving_original(
    pio: &Byml,
    original: &[u8],
    endian: roead::Endian,
) -> Vec<u8> {
    if Byml::from_binary(original).is_ok_and(|original_pio| original_pio == *pio) {
        original.to_vec()
    } else {
        pio.to_binary(endian)
    }
}
