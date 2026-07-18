use crate::file_format::Archive::{ArchiveDocument, RootArchive};
use crate::file_format::BinTextFile::{BymlFile, OpenedFile};
use crate::file_format::Esetb::Esetb;
use crate::file_format::Pack::{PackComparer, SarcPaths};
use crate::Comparer::DiffComparer;
use crate::NestedSarc::{NestedArchive, NestedArchives};
use crate::Open_and_Save::{
    check_if_save_in_romfs, file_from_disk_to_senddata, get_binary_by_filetype,
    get_string_from_data, SaveFileDialog, SendData,
};
use crate::Settings::{list_files_recursively, write_string_to_file, Pathlib};
use crate::TotkConfig::TotkConfig;
use crate::Zstd::{global_totk_zstd, TotkFileType, TotkZstd, ZstdDictionary};
use rfd::{FileDialog, MessageDialog};
use roead::byml::Byml;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::io::{Read, Write};

use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SaveData {
    pub tab: String,
    pub text: String,
}

#[derive(Clone, Debug)]
pub struct InternalParentLink {
    pub document_id: String,
    pub outer_path: Option<String>,
    pub inner_path: String,
}

pub struct TotkBitsApp<'a> {
    pub opened_file: OpenedFile<'a>, //path to opened file in string
    pub text: String,
    pub status_text: String,
    pub zstd: Arc<TotkZstd<'a>>,
    // pub zstd_cpp: Arc<ZstdCppCompressor>,
    pub pack: Option<PackComparer<'a>>,
    pub archive: Option<ArchiveDocument>,
    pub internal_file: Option<InternalFile<'a>>,
    pub nested_archives: NestedArchives,
    pub nested_edit: Option<(String, String)>,
    pub internal_parent: Option<InternalParentLink>,
}

unsafe impl<'a> Send for TotkBitsApp<'a> {}

impl Default for TotkBitsApp<'_> {
    fn default() -> Self {
        match TotkConfig::safe_new() {
            Ok(conf) => {
                let totk_config: Arc<TotkConfig> = Arc::new(conf);
                match global_totk_zstd(totk_config, 16) {
                    Ok(zstd) => {
                        return Self {
                            opened_file: OpenedFile::default(),
                            text: "".to_string(),
                            status_text: "Ready".to_string(),
                            zstd: zstd.clone(),
                            pack: None,
                            archive: None,
                            internal_file: None,
                            nested_archives: NestedArchives::default(),
                            nested_edit: None,
                            internal_parent: None,
                        };
                    }
                    Err(_) => {
                        println!("Error while initializing romfs path");
                        std::process::exit(1);
                    }
                }
            }
            Err(_) => {
                println!("Error while initializing romfs path");
                std::process::exit(2);
            }
        }

        // std::process::exit(3); //should never reach here
    }
}

impl<'a> TotkBitsApp<'a> {
    pub fn internal_entry_bytes(&mut self, outer: Option<&str>, path: &str) -> Option<Vec<u8>> {
        match outer {
            Some(outer) => self
                .nested_archives
                .get_mut(outer)?
                .get(path)
                .map(<[u8]>::to_vec),
            None => self.root_entry(path),
        }
    }

    pub fn open_internal_child(
        &mut self,
        parent_document_id: String,
        outer_path: Option<String>,
        path: String,
        bytes: Vec<u8>,
    ) -> Option<SendData> {
        let (internal, text) = get_string_from_data(path.clone(), bytes, self.zstd.clone())?;
        let mut data = SendData::default();
        data.text = text;
        data.path = internal.path.clone();
        data.tab = "YAML".into();
        data.status_text = match &outer_path {
            Some(outer) => format!("Opened {path} inside {outer}"),
            None => format!("Opened {path} from archive"),
        };
        data.get_file_label(internal.file_type, internal.endian);
        data.set_file_metadata(internal.file_type, internal.zstd_dictionary);
        self.opened_file = OpenedFile::default();
        self.internal_file = Some(internal);
        self.internal_parent = Some(InternalParentLink {
            document_id: parent_document_id,
            outer_path,
            inner_path: path,
        });
        Some(data)
    }

    pub fn internal_binary(&mut self, text: &str) -> Option<Vec<u8>> {
        let internal = self.internal_file.as_ref()?;
        get_binary_by_filetype(
            internal.file_type,
            text,
            internal.endian.unwrap_or(roead::Endian::Little),
            self.zstd.clone(),
            &internal.path.full_path,
            &mut self.opened_file,
            internal.zstd_dictionary,
        )
    }

    pub fn update_child_entry(
        &mut self,
        outer: Option<&str>,
        path: &str,
        bytes: Vec<u8>,
    ) -> Result<SendData, String> {
        if let Some(outer) = outer {
            self.nested_archives
                .get_mut(outer)
                .ok_or_else(|| format!("nested archive cache missing: {outer}"))?
                .set(path, bytes);
            self.flush_nested_chain(outer)?;
        } else {
            self.set_root_entry(path, bytes)?;
        }
        let mut data = self.archive_send_data(format!("Updated {path} in archive"));
        data.tab = "YAML".into();
        Ok(data)
    }
    fn root_entry(&mut self, path: &str) -> Option<Vec<u8>> {
        if let Some(archive) = &self.archive {
            return archive.get(path).map(<[u8]>::to_vec);
        }
        self.pack
            .as_mut()?
            .opened
            .as_mut()?
            .writer
            .get_file(path)
            .cloned()
    }
    fn set_root_entry(&mut self, path: &str, bytes: Vec<u8>) -> Result<(), String> {
        if let Some(archive) = &mut self.archive {
            return archive.set(path, bytes);
        }
        let pack = self.pack.as_mut().ok_or("no root archive")?;
        pack.opened
            .as_mut()
            .ok_or("root SARC unavailable")?
            .writer
            .add_file(path, bytes);
        pack.compare_and_reload();
        Ok(())
    }
    fn flush_nested_chain(&mut self, chain: &str) -> Result<(), String> {
        let mut current = chain.to_string();
        loop {
            let encoded = self
                .nested_archives
                .get_mut(&current)
                .ok_or_else(|| format!("nested archive cache missing: {current}"))?
                .to_encoded(self.zstd.clone())
                .map_err(|e| e.to_string())?;
            if let Some((parent, entry)) = current.rsplit_once("::") {
                self.nested_archives
                    .get_mut(parent)
                    .ok_or_else(|| format!("parent archive cache missing: {parent}"))?
                    .set(entry, encoded);
                current = parent.to_string();
            } else {
                self.set_root_entry(&current, encoded)?;
                break;
            }
        }
        Ok(())
    }
    fn add_nested_paths(&self, data: &mut SendData) {
        for (chain, archive) in &self.nested_archives {
            data.sarc_paths
                .nested_paths
                .insert(chain.clone(), archive.paths());
        }
    }
    fn drop_nested_descendants(&mut self, chain: &str, path: &str) {
        let identity = format!("{chain}::{path}");
        let prefix = format!("{identity}::");
        self.nested_archives
            .retain(|key, _| key != &identity && !key.starts_with(&prefix));
    }
    fn archive_send_data(&self, status: String) -> SendData {
        let mut data = SendData::default();
        data.tab = "SARC".to_string();
        data.status_text = status;
        if let Some(archive) = &self.archive {
            data.path = Pathlib::new(archive.path.clone());
            let kind = if matches!(archive.archive, RootArchive::Folder(_)) {
                "Folder"
            } else {
                "Archive"
            };
            data.file_label = format!("{} [{kind}]", data.path.name);
            data.sarc_paths.paths = archive.paths();
            data.sarc_paths.added_paths = archive.added.iter().cloned().collect();
            data.sarc_paths.modded_paths = archive.modified.iter().cloned().collect();
        } else if let Some(pack) = &self.pack {
            data.get_sarc_paths(pack);
        }
        self.add_nested_paths(&mut data);
        data
    }
    //RSTB
    pub fn get_rstb_entries_by_query(&mut self, entry: String) -> Option<SendData> {
        let mut data = SendData::default();
        let mut isDefaultAdded = false;
        if let Some(rstb) = &mut self.opened_file.restbl {
            data.tab = "RSTB".to_string();

            let entry_low = entry.to_lowercase();
            for elem in rstb.hash_table.iter() {
                if elem.to_lowercase().contains(&entry_low) {
                    if let Some(val) = rstb.table.get(elem.clone()) {
                        if elem == &entry {
                            isDefaultAdded = true;
                        }
                        data.rstb_paths
                            .push(json!({ "path": elem.clone(), "val": val.to_string() }));
                    }
                }
            }
            if !isDefaultAdded {
                if let Some(val) = rstb.table.get(entry.clone()) {
                    //entry exists
                    data.rstb_paths
                        .push(json!({ "path": entry.clone(), "val": val.to_string() }));
                }
            }
            data.status_text = format!("Found entries: {}", data.rstb_paths.len());
        } else {
            data.status_text = "Error: No RSTB opened".to_string();
            data.tab = "ERROR".to_string();
        }

        Some(data)
    }

    pub fn rstb_edit_entry(&mut self, entry: String, val: String) -> Option<SendData> {
        let mut data = SendData::default();
        if let Some(rstb) = &mut self.opened_file.restbl {
            data.tab = "RSTB".to_string();
            if let Some(_) = rstb.table.get(entry.clone()) {
                //entry exists
                data.status_text = format!("Modified: {}", &entry);
            } else {
                data.status_text = format!("Added: {}", &entry);
            }
            rstb.table
                .set(entry, val.parse::<u32>().expect("Failed to parse value"));
        } else {
            data.status_text = "Error: No RSTB opened".to_string();
            data.tab = "ERROR".to_string();
        }

        Some(data)
    }
    pub fn rstb_remove_entry(&mut self, entry: String) -> Option<SendData> {
        let mut data = SendData::default();
        if let Some(rstb) = &mut self.opened_file.restbl {
            data.tab = "RSTB".to_string();
            if let Some(_) = rstb.table.get(entry.clone()) {
                //entry exists
                data.status_text = format!("Removed: {}", &entry);
                rstb.table.remove(entry.clone());
            } else {
                data.status_text = format!("Error: entry absent in RSTB ({})", &entry);
            }
        } else {
            data.status_text = "Error: No RSTB opened".to_string();
            data.tab = "ERROR".to_string();
        }

        Some(data)
    }
    //END RSTB

    pub fn extract_opened_sarc(&self) -> Option<SendData> {
        let mut data = SendData::default();
        let dest_folder = FileDialog::new()
            .set_title("Choose folder to extract to")
            .pick_folder();
        if let Some(dest_folder) = dest_folder {
            if let Some(archive) = &self.archive {
                for path in archive.paths() {
                    let destination = dest_folder.join(&path);
                    if let Some(parent) = destination.parent() {
                        fs::create_dir_all(parent).ok()?;
                    }
                    fs::write(destination, archive.get(&path)?).ok()?;
                }
                data.status_text = format!("Extracted archive to {}", dest_folder.display());
                return Some(data);
            }
            if let Some(pack) = &self.pack {
                match pack.extract_all_to_folder(&dest_folder) {
                    Ok(m) => {
                        data.status_text = m;
                    }
                    Err(e) => {
                        data.status_text = "Error: Failed to extract SARC".to_string();
                        MessageDialog::new()
                            .set_title("Error")
                            .set_description(format!("Error: {}", e))
                            .set_buttons(rfd::MessageButtons::Ok)
                            .show();
                    }
                }

                return Some(data);
                // if let Ok(m) = pack.extract_all_to_folder(&dest_folder) {
                //     data.status_text = m;
                // } else {
                //     data.status_text = "Error: Failed to extract SARC".to_string();
                // }
            } else {
                return None; //no sarc opened
            }
        } else {
            return None; //no folder selected
        }
    }

    pub fn extract_folder_from_opened_sarc(&mut self, source_folder: String) -> Option<SendData> {
        let dest_folder = FileDialog::new()
            .set_title("Choose folder to extract to")
            .pick_folder()?;
        let mut data = SendData::default();
        if let Some(archive) = &self.archive {
            let prefix = if source_folder.is_empty() {
                String::new()
            } else {
                format!("{}/", source_folder.trim_end_matches('/'))
            };
            let paths: Vec<_> = archive
                .paths()
                .into_iter()
                .filter(|p| p.starts_with(&prefix))
                .collect();
            for path in &paths {
                let relative = path.strip_prefix(&prefix).unwrap_or(path);
                let destination = dest_folder.join(relative);
                if let Some(parent) = destination.parent() {
                    fs::create_dir_all(parent).ok()?;
                }
                fs::write(destination, archive.get(path)?).ok()?;
            }
            data.status_text = format!("Extracted {} archive entries", paths.len());
            return Some(data);
        }
        let pack = self.pack.as_mut()?;
        match pack.extract_folder_to(&source_folder, &dest_folder) {
            Ok(message) => data.status_text = message,
            Err(error) => data.status_text = format!("Error: {}", error),
        }
        Some(data)
    }

    pub fn remove_internal_elem(&mut self, internal_path: String) -> Option<SendData> {
        if let Some(archive) = &mut self.archive {
            return Some(match archive.remove_prefix(&internal_path) {
                Ok(count) => self.archive_send_data(format!("Removed {count} archive entries")),
                Err(error) => {
                    let mut d = self.archive_send_data(format!("Error: {error}"));
                    d.tab = "ERROR".into();
                    d
                }
            });
        }
        let mut data = SendData::default();
        let mut is_reload = false;

        if let Some(pack) = &mut self.pack {
            if let Some(opened) = &mut pack.opened {
                is_reload = true;
                if let Some(_) = opened.writer.get_file(&internal_path) {
                    //its a file
                    if MessageDialog::new()
                        .set_title("Remove file")
                        .set_description(format!("The file:\n{}\nwill be removed. This operation cannot be reverted! Proceed?", &internal_path))
                        .set_buttons(rfd::MessageButtons::YesNo)
                        .show()
                        == rfd::MessageDialogResult::No
                    {
                        return None;
                    }
                    opened.writer.remove_file(&internal_path);
                    data.status_text = format!("Removed {}", &internal_path);
                } else {
                    //its a directory
                    if MessageDialog::new()
                    .set_title("Remove directory")
                    .set_description(format!("All files from directory:\n{}\nwill be removed. This operation cannot be reverted! Proceed?", &internal_path))
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show()
                    == rfd::MessageDialogResult::No
                {
                    return None;
                }
                    let mut to_remove: Vec<String> = Vec::new();
                    for file in opened.writer.files.keys() {
                        if file.starts_with(&internal_path) {
                            to_remove.push(file.clone());
                        }
                    }
                    let mut i: usize = 0;
                    for file in to_remove {
                        opened.writer.remove_file(&file);
                        i += 1;
                    }
                    data.status_text = format!("Removed {} files from {}", i, &internal_path);
                }
            }
            if is_reload {
                pack.compare_and_reload();
                data.get_sarc_paths(pack);
            }
            return Some(data);
        }

        None
    }

    pub fn close_all_click(&mut self) -> Option<SendData> {
        if self.zstd.totk_config.close_all_prompt
            && MessageDialog::new()
                .set_title("Close all")
                .set_description("All currently opened files will be closed. Proceed?")
                .set_buttons(rfd::MessageButtons::YesNo)
                .show()
                == rfd::MessageDialogResult::No
        {
            return None;
        }
        let mut data = SendData::default();
        self.opened_file = OpenedFile::default();
        self.text = String::new();
        self.status_text = "Ready".to_string();
        self.pack = None;
        self.archive = None;
        self.internal_file = None;
        self.nested_archives.clear();
        self.nested_edit = None;
        data.status_text = "Closed all opened files".to_string();
        data.sarc_paths = SarcPaths::default();
        Some(data)
    }

    pub fn get_binary_for_opened_file(
        &mut self,
        text: &str,
        zstd: Arc<TotkZstd>,
        dest_file: &str,
    ) -> Option<Vec<u8>> {
        get_binary_by_filetype(
            self.opened_file.file_type,
            text,
            self.opened_file.endian.unwrap_or(roead::Endian::Little),
            zstd.clone(),
            dest_file,
            &mut self.opened_file,
            None,
        )
    }

    pub fn extract_file(&mut self, internal_path: String) -> Option<SendData> {
        let mut dialog = SaveFileDialog::new(
            "YAML".to_string(),
            &None,
            &self.opened_file,
            "Choose save location".to_string(),
        );
        dialog.name = Some(Pathlib::new(internal_path.clone()).name);
        dialog.filters_from_path(&internal_path);
        let path = dialog.show();
        if path.is_empty() {
            return None;
        }

        let mut data = SendData::default();
        if let Some(archive) = &self.archive {
            let rawdata = archive.get(&internal_path)?;
            fs::write(&path, rawdata).ok()?;
            data.status_text = format!("Extracted {} to {}", internal_path, path);
            return Some(data);
        }
        if let Some(pack) = &mut self.pack {
            if let Some(opened) = &mut pack.opened {
                if let Some(rawdata) = opened.writer.get_file(&internal_path) {
                    let mut file = fs::File::create(&path).ok()?;
                    file.write_all(&rawdata).ok()?;
                    data.status_text = format!("Extracted {} to {}", &internal_path, &path);
                    return Some(data);
                }
                data.status_text = format!(
                    "Error: {} not found in {}",
                    &internal_path, &opened.path.name
                );
                return Some(data);
            }
        }
        None
    }

    pub fn rename_internal_file_from_path(
        &mut self,
        internal_path: String,
        new_internal_path: String,
    ) -> Option<SendData> {
        let mut data = SendData::default();
        if let Some(archive) = &mut self.archive {
            let target = if internal_path.contains('/') {
                format!(
                    "{}/{}",
                    Path::new(&internal_path)
                        .parent()
                        .unwrap_or(Path::new(""))
                        .to_string_lossy()
                        .replace('\\', "/"),
                    Path::new(&new_internal_path).file_name()?.to_string_lossy()
                )
                .trim_start_matches('/')
                .to_string()
            } else {
                Path::new(&new_internal_path)
                    .file_name()?
                    .to_string_lossy()
                    .into_owned()
            };
            return Some(match archive.rename_prefix(&internal_path, &target) {
                Ok(count) => self.archive_send_data(format!("Renamed {count} archive entries")),
                Err(error) => {
                    let mut d = self.archive_send_data(format!("Error: {error}"));
                    d.tab = "ERROR".into();
                    d
                }
            });
        }
        let p1 = Pathlib::new(internal_path.clone());
        let p2 = Pathlib::new(new_internal_path.clone());
        println!(
            "trying to rename  {} to sarc path {}",
            &new_internal_path, &internal_path
        );
        let mut is_reload = false;
        if let Some(pack) = &mut self.pack {
            if let Some(opened) = &mut pack.opened {
                is_reload = true;
                //file is in sarc
                if let Some(rawdata) = opened.writer.get_file(&internal_path) {
                    let rawdata_backup = rawdata.clone();
                    let new_path = format!("{}/{}", &p1.parent, &p2.name);
                    opened.writer.remove_file(&internal_path);
                    opened.writer.add_file(&new_path, rawdata_backup);
                    data.status_text = format!("Renamed {} to {}", &p1.name, &p2.name);
                } else {
                    //assuming the node is a directory
                    let mut to_remove: Vec<String> = Vec::new();
                    for file in opened.writer.files.keys() {
                        if file.starts_with(&internal_path) {
                            to_remove.push(file.clone());
                        }
                    }
                    println!("{:?}", to_remove);
                    let mut i: usize = 0;
                    for file in to_remove {
                        if let Some(rawdata) = opened.writer.get_file(&file) {
                            let input = "asdf.qwre.zxcv";
                            let _result: String =
                                input.split('.').skip(1).collect::<Vec<&str>>().join(".");

                            let rawdata_backup = rawdata.clone();
                            let new_path = format!("{}/{}", &p1.parent, &p2.name);
                            // let new_file_path = file.replace(&internal_path, &new_path);
                            let tmp = file
                                .split(&internal_path)
                                .skip(1)
                                .collect::<Vec<&str>>()
                                .join(&internal_path);
                            let mut new_file_path = format!("{}/{}", &new_path, &tmp);
                            if new_file_path.starts_with("/") {
                                new_file_path = new_file_path[1..].to_string();
                            }
                            new_file_path = new_file_path.replace("//", "/");
                            println!("{} -> {}", &file, &new_file_path);
                            opened.writer.remove_file(&file);
                            opened.writer.add_file(&new_file_path, rawdata_backup);
                            i += 1;
                        }
                    }
                    data.status_text = format!(
                        "Renamed {} to {} ({} files affected)",
                        &p1.name, &p2.name, i
                    );
                }
            }
            if is_reload {
                pack.compare_and_reload();
                data.get_sarc_paths(pack);
            }
            return Some(data);
        }
        None
    }

    pub fn add_dir_to_sarc(&mut self, internal_dir: String, path: String) -> Option<SendData> {
        let mut data = SendData::default();
        let mut int_path = Pathlib::new(internal_dir.replace("\\", "/"));
        let path_var = Pathlib::new(path.replace("\\", "/"));
        let dirname = path_var.name.clone();
        let dirname_len = dirname.len();
        let mut path_root_len = path_var.full_path.len() - dirname_len;
        let mut path_root = &path_var.full_path[..path_root_len];
        if path_root.ends_with("/") {
            path_root = &path_root[..path_root.len() - 1];
        }
        path_root_len = path_root.len();
        if int_path.full_path.ends_with("/") {
            int_path.full_path = int_path.full_path[..int_path.full_path.len() - 1].to_string();
        }
        let files = list_files_recursively(&path_var.full_path);
        let files_len = files.len();
        if files_len == 0 {
            data.status_text = format!("No files found in {}", &path_var.full_path);
        } else {
            for file in files {
                let file_path = Pathlib::new(&file);
                let mut file_path_to_add = file_path.full_path[path_root_len..].to_string();
                if file_path_to_add.starts_with("/") {
                    file_path_to_add = file_path_to_add[1..].to_string();
                }
                let new_internal_path = format!("{}/{}", &int_path.full_path, &file_path_to_add);
                let _ = self.add_internal_file_from_path(new_internal_path, file, true);
            }
            data.status_text = format!("Added {} files to {}", files_len, &int_path.full_path);
        }
        if let Some(pack) = &mut self.pack {
            data.get_sarc_paths(pack);
        }
        Some(data)
        // None
    }

    pub fn add_internal_file_to_dir(
        &mut self,
        internal_dir: String,
        path: String,
    ) -> Option<SendData> {
        // let mut data = SendData::default();
        let p1 = Pathlib::new(internal_dir.replace("\\", "/"));
        let p2 = Pathlib::new(path.replace("\\", "/"));
        let internal_path = format!("{}/{}", &p1.full_path, &p2.name);
        return self.add_internal_file_from_path(internal_path, p2.full_path, true);

        // Some(data)
    }

    pub fn add_internal_file_from_path(
        &mut self,
        internal_path: String,
        path: String,
        overwrite: bool,
    ) -> Option<SendData> {
        let mut data = SendData::default();
        if let Some(archive) = &mut self.archive {
            let normalized = internal_path.replace('\\', "/");
            if archive.get(&normalized).is_some() && !overwrite {
                return None;
            }
            let bytes = fs::read(&path).ok()?;
            return Some(match archive.set(&normalized, bytes) {
                Ok(()) => self.archive_send_data(format!("Added/replaced: {normalized}")),
                Err(error) => {
                    let mut d = self.archive_send_data(format!("Error: {error}"));
                    d.tab = "ERROR".into();
                    d
                }
            });
        }
        println!("trying to add  {} to sarc path {}", &path, &internal_path);
        let mut is_reload = false;
        if let Some(pack) = &mut self.pack {
            if let Some(opened) = &mut pack.opened {
                if let Some(_) = &opened.writer.get_file(&internal_path) {
                    if !overwrite {
                        let m = format!(
                            "{}\nalready exists in {}. Proceed?",
                            &internal_path, &opened.path.name
                        );
                        if MessageDialog::new()
                            .set_title("File already exists")
                            .set_description(m)
                            .set_buttons(rfd::MessageButtons::YesNo)
                            .show()
                            == rfd::MessageDialogResult::No
                        {
                            return None;
                        }
                    }
                }

                is_reload = true;
                let mut f_handle = fs::File::open(&path).ok()?;
                let mut buffer: Vec<u8> = Vec::new();
                f_handle.read_to_end(&mut buffer).ok()?;
                opened
                    .writer
                    .add_file(&internal_path.replace("\\", "/"), buffer);
                data.status_text = format!("Added/replaced: {}", &internal_path);
            }
            if is_reload {
                pack.compare_and_reload();
                data.get_sarc_paths(pack);
            }
            return Some(data);
        }
        println!(
            "ERROR: unable to add {} to sarc path {}",
            &path, &internal_path
        );
        // println!("{:?}", data);
        None
    }

    pub fn save_as(&mut self, save_data: SaveData) -> Option<SendData> {
        //TODO: FINISH!
        let mut data = SendData::default();
        let mut dialog = SaveFileDialog::new(
            save_data.tab.to_string(),
            &self.pack,
            &self.opened_file,
            //"Save file as".to_string(),
            format!("Save {} as", save_data.tab),
        );
        dialog.generate_filters_and_name();
        if let Some(archive) = &self.archive {
            dialog.name = Some(Pathlib::new(archive.path.clone()).name);
            dialog.filters_from_path(&archive.path);
        }
        if self.opened_file.path.full_path.is_empty() {
            if let Some(internal_file) = &self.internal_file {
                dialog.name = Some(internal_file.path.name.clone());
                dialog.filters_from_path(&internal_file.path.full_path);
            }
        }
        if save_data.tab == "RSTB" && self.opened_file.restbl.is_some() {
            if let Some(rstb) = &self.opened_file.restbl {
                dialog.name = Some(rstb.path.name.clone());
                dialog.filters_from_path(&rstb.path.full_path);
            }
        }
        if (dialog.name.clone().unwrap_or_default().is_empty() && save_data.tab == "SARC")
            || (save_data.tab == "RSTB" && self.opened_file.restbl.is_none())
        {
            println!("Nothing is opened, nothing to save");
            return None;
        }
        let dest_file = dialog.show();
        if !dest_file.is_empty() && !check_if_save_in_romfs(&dest_file, self.zstd.clone()) {
            match save_data.tab.as_str() {
                "YAML" => {
                    if dialog.isText {
                        write_string_to_file(&dest_file, &save_data.text).ok()?;
                        data.tab = "YAML".to_string();
                        data.status_text = format!("Saved {}", &dest_file);
                        data.path = Pathlib::new(dest_file.clone());
                        self.opened_file.path = Pathlib::new(dest_file);
                    } else {
                        let rawdata = if self.internal_file.is_some() {
                            self.internal_binary(&save_data.text)
                        } else {
                            self.get_binary_for_opened_file(
                                &save_data.text,
                                self.zstd.clone(),
                                &dest_file,
                            )
                        };
                        if let Some(rawdata) = rawdata {
                            let mut file = fs::File::create(&dest_file).ok()?;
                            file.write_all(&rawdata).ok()?;
                            data.tab = "YAML".to_string();
                            data.status_text = format!("Saved {}", &dest_file);
                            data.path = Pathlib::new(dest_file.clone());
                            self.opened_file.path = Pathlib::new(dest_file);
                        } else {
                            data.status_text = format!(
                                "Error: Failed to save [{:?}] {}",
                                self.opened_file.file_type, &dest_file
                            );
                        }
                    }

                    return Some(data);
                }
                "SARC" => {
                    if let Some(archive) = &mut self.archive {
                        return Some(match archive.save_atomic(Path::new(&dest_file)) {
                            Ok(()) => self.archive_send_data(format!("Saved archive {dest_file}")),
                            Err(error) => {
                                let mut d = self.archive_send_data(format!("Error: {error}"));
                                d.tab = "ERROR".into();
                                d
                            }
                        });
                    }
                    let mut is_reload = false;
                    if let Some(pack) = &mut self.pack {
                        if let Some(opened) = &mut pack.opened {
                            match opened.save(dest_file.clone()) {
                                Ok(_) => {
                                    is_reload = true;
                                    println!("Saved SARC {}", &dest_file);
                                    data.tab = "SARC".to_string();
                                    data.status_text = format!("Saved SARC {}", &dest_file);
                                    data.path = Pathlib::new(dest_file.clone());
                                    self.opened_file.path = Pathlib::new(dest_file);
                                }
                                Err(_) => {
                                    println!("ERROR SAVING SARC {}", &dest_file);
                                    data.status_text =
                                        format!("Error: Failed to save SARC {}", &dest_file);
                                }
                            }
                        }
                        if is_reload {
                            pack.compare_and_reload();
                            data.get_sarc_paths(pack);
                        }
                        return Some(data);
                    }
                    return None; //no sarc opened
                }
                "RSTB" => {
                    println!("About to save RSTB");
                    if let Some(rstb) = &mut self.opened_file.restbl {
                        if let Ok(_) = rstb.save(&dest_file) {
                            data.tab = "RSTB".to_string();
                            data.status_text =
                                format!("Saved {}", &self.opened_file.path.full_path);
                        } else {
                            data.status_text = format!(
                                "Error: Failed to save {}",
                                &self.opened_file.path.full_path
                            );
                        }
                    } else {
                        data.status_text = format!("Error: No RSTB opened");
                    }
                }
                _ => {
                    data.status_text = format!("Error: Unsupported tab {}", &save_data.tab);
                    return Some(data);
                }
            }
        }

        None
    }

    pub fn save_tab_yaml(&mut self, save_data: SaveData) -> Option<SendData> {
        let mut data = SendData::default();
        let mut is_reload = false;
        let text = &save_data.text;
        if let Some((outer_path, inner_path)) = self.nested_edit.clone() {
            let internal_file = self.internal_file.as_ref()?;
            let rawdata = get_binary_by_filetype(
                internal_file.file_type,
                text,
                internal_file.endian.unwrap_or(roead::Endian::Little),
                self.zstd.clone(),
                &inner_path,
                &mut self.opened_file,
                internal_file.zstd_dictionary,
            )?;
            let archive = self.nested_archives.get_mut(&outer_path)?;
            archive.set(&inner_path, rawdata);
            self.flush_nested_chain(&outer_path).ok()?;
            data.tab = "YAML".to_string();
            data.status_text = format!("Saved {} inside {}", inner_path, outer_path);
            if let Some(pack) = &self.pack {
                data.get_sarc_paths(pack);
            } else if let Some(root) = &self.archive {
                data.sarc_paths.paths = root.paths();
            }
            self.add_nested_paths(&mut data);
            return Some(data);
        }
        if let Some(internal_file) = &self.internal_file {
            if self.archive.is_some() {
                let path = internal_file.path.full_path.clone();
                let rawdata = get_binary_by_filetype(
                    internal_file.file_type,
                    text,
                    internal_file.endian.unwrap_or(roead::Endian::Little),
                    self.zstd.clone(),
                    &path,
                    &mut self.opened_file,
                    internal_file.zstd_dictionary,
                )?;
                self.archive.as_mut()?.set(&path, rawdata).ok()?;
                data.tab = "YAML".into();
                data.status_text = format!("Updated {path} in archive (save archive to write it)");
                return Some(data);
            }
            if let Some(pack) = &mut self.pack {
                if let Some(opened) = &mut pack.opened {
                    let path = &internal_file.path.full_path;
                    let rawdata: Vec<u8> = get_binary_by_filetype(
                        internal_file.file_type,
                        text,
                        internal_file.endian.unwrap_or(roead::Endian::Little),
                        self.zstd.clone(),
                        &path,
                        &mut self.opened_file,
                        internal_file.zstd_dictionary,
                    )?;
                    if rawdata.is_empty() {
                        data.status_text =
                            format!("Error: Failed to save {} for {}", &path, &opened.path.name);
                        data.tab = "ERROR".to_string();
                        println!("{:?}", &data);
                        return Some(data);
                    } else {
                        opened.writer.add_file(path, rawdata);
                        is_reload = true;
                        data.tab = "YAML".to_string();
                        data.status_text = format!(
                            "Saved {} for {}",
                            &internal_file.path.name, &opened.path.name
                        );
                    }
                }
                if is_reload {
                    pack.compare_and_reload();
                    data.get_sarc_paths(pack);
                }
            }
        } else {
            let fullpath = self.opened_file.path.full_path.clone();
            let rawdata: Vec<u8> = get_binary_by_filetype(
                self.opened_file.file_type,
                text,
                self.opened_file.endian.unwrap_or(roead::Endian::Little),
                self.zstd.clone(),
                &fullpath,
                &mut self.opened_file,
                None,
            )?;
            if rawdata.is_empty() {
                data.status_text =
                    format!("Error: Failed to save {}", &self.opened_file.path.full_path);
                data.tab = "ERROR".to_string();
                // println!("{:?}", &data);
                return Some(data);
            } else {
                let mut file = fs::File::create(&self.opened_file.path.full_path).ok()?;
                file.write_all(&rawdata).ok()?;
                data.tab = "YAML".to_string();
                data.status_text = format!("Saved {}", &self.opened_file.path.full_path);
                // println!("{:?}", &data);
                return Some(data);
            }
        }

        Some(data)
    }

    // #[allow(unused_variables)]
    pub fn save(&mut self, save_data: SaveData) -> Option<SendData> {
        println!(
            "About to save {} text len {}",
            save_data.tab,
            save_data.text.len()
        );
        let mut data = SendData::default();
        // let mut is_reload = false;
        let _text = &save_data.text;

        match save_data.tab.as_str() {
            "YAML" => {
                return self.save_tab_yaml(save_data);
            }
            "SARC" => {
                if let Some(archive) = &mut self.archive {
                    let destination = PathBuf::from(&archive.path);
                    return Some(match archive.save_atomic(&destination) {
                        Ok(()) => self
                            .archive_send_data(format!("Saved archive {}", destination.display())),
                        Err(error) => {
                            let mut d = self.archive_send_data(format!("Error: {error}"));
                            d.tab = "ERROR".into();
                            d
                        }
                    });
                }
                if let Some(pack) = &mut self.pack {
                    if let Some(opened) = &mut pack.opened {
                        if !check_if_save_in_romfs(&opened.path.full_path, self.zstd.clone()) {
                            opened.reload();
                            match opened.save_default() {
                                Ok(_) => {
                                    // is_reload = true;
                                    data.tab = "SARC".to_string();
                                    data.status_text =
                                        format!("Saved SARC {}", &opened.path.full_path);
                                }
                                Err(err) => {
                                    data.status_text = format!(
                                        "Error: Failed to save SARC {}",
                                        &opened.path.full_path
                                    );
                                    eprintln!("{:?}", err);
                                }
                            }
                            // } else {
                            //     data.status_text = format!(
                            //         "Error: Failed to save SARC {}",
                            //         &opened.path.full_path
                            //     );
                            // }
                        }
                    }
                }
            }
            "RSTB" => {
                println!("About to save RSTB");
                if let Some(rstb) = &mut self.opened_file.restbl {
                    if let Ok(_) = rstb.save_default() {
                        data.tab = "RSTB".to_string();
                        data.status_text = format!("Saved {}", &self.opened_file.path.full_path);
                    } else {
                        data.status_text =
                            format!("Error: Failed to save {}", &self.opened_file.path.full_path);
                    }
                } else {
                    data.status_text = format!("Error: No RSTB opened");
                }
            }

            _ => {
                // data.status_text = format!("Error: Unsupported tab {}", &save_data.tab);
            }
        }

        Some(data)
    }

    pub fn add_empty_byml(&mut self, folder: String) -> Option<SendData> {
        let mut data = SendData::default();
        let mut is_reload = false;
        if let Some(pack) = &mut self.pack {
            if let Some(opened) = &mut pack.opened {
                let raw_data = Byml::default().to_binary(opened.endian);
                let path = PathBuf::from(&folder);
                let mut i: i32 = 1;
                while i < 9999 {
                    let mut new_path = path.clone();
                    new_path.push(format!("new_{}.byml", i));
                    let dest_file = new_path.to_string_lossy().to_string().replace("\\", "/");
                    if !opened.writer.files.contains_key(&dest_file) {
                        opened.writer.add_file(&dest_file, raw_data.clone());
                        data.status_text = format!("Added {}", &dest_file);
                        is_reload = true;
                        break;
                    }
                    i += 1;
                }
            }
            if is_reload {
                pack.compare_and_reload();
                data.get_sarc_paths(pack);
            }
        }

        Some(data)
    }

    pub fn edit_internal_file(&mut self, path: String) -> Option<SendData> {
        self.nested_edit = None;
        if path.is_empty() || !path.contains(".") {
            return None;
        }
        let mut data = SendData::default();
        if let Some(archive) = &self.archive {
            let raw_data = archive.get(&path)?.to_vec();
            if let Some((intern, text)) =
                get_string_from_data(path.clone(), raw_data, self.zstd.clone())
            {
                self.internal_file = Some(intern);
                self.opened_file = OpenedFile::default();
                let internal = self.internal_file.as_ref()?;
                data.text = text;
                data.path = internal.path.clone();
                data.tab = "YAML".into();
                data.status_text = format!("Opened {} from archive", path);
                data.get_file_label(internal.file_type, internal.endian);
            } else {
                data.tab = "ERROR".into();
                data.status_text = format!("Error: unsupported data type for {path}");
            }
            return Some(data);
        }
        if let Some(pack) = &mut self.pack {
            if let Some(opened) = &mut pack.opened {
                let raw_data = opened.sarc.get_data(&path);
                if let Some(raw_data) = raw_data {
                    //if let Some(res) = get_string_from_data(path, raw_data.to_vec(), self.zstd.clone()) {
                    if let Some((intern, text)) =
                        get_string_from_data(path.clone(), raw_data.to_vec(), self.zstd.clone())
                    {
                        self.opened_file = OpenedFile::default();
                        data.text = text;
                        data.path = intern.path.clone();
                        data.status_text = format!(
                            "Opened {} [{:?}] from {}",
                            &intern.path.name, &intern.file_type, &opened.path.name
                        );
                        data.tab = "YAML".to_string();
                        data.get_file_label(intern.file_type, intern.endian);
                        self.internal_file = Some(intern);
                        return Some(data);
                    } else {
                        data.status_text =
                            format!("Error: unsupported data type for {}", &path.clone());
                    }
                } else {
                    data.status_text = format!("Error: {} absent in {}", &path, &opened.path.name);
                }
                data.tab = "ERROR".to_string();
                return Some(data);
            }
        }

        None
    }

    pub fn expand_nested_sarc(&mut self, outer_path: String) -> SendData {
        let mut data = SendData::default();
        data.tab = "SARC".to_string();
        let result = (|| -> Result<Vec<String>, String> {
            if !self.nested_archives.contains_key(&outer_path) {
                let (name, bytes) = if let Some((parent, entry)) = outer_path.rsplit_once("::") {
                    (
                        entry.to_string(),
                        self.nested_archives
                            .get_mut(parent)
                            .and_then(|a| a.get(entry))
                            .ok_or_else(|| format!("{entry} was not found in {parent}"))?
                            .to_vec(),
                    )
                } else {
                    (
                        outer_path.clone(),
                        self.root_entry(&outer_path).ok_or_else(|| {
                            format!("{} was not found in the root archive", outer_path)
                        })?,
                    )
                };
                let archive = NestedArchive::parse_named(&name, &bytes, self.zstd.clone())
                    .map_err(|error| error.to_string())?;
                self.nested_archives.insert(outer_path.clone(), archive);
            }
            self.nested_archives
                .get(&outer_path)
                .map(|archive| archive.paths())
                .ok_or_else(|| format!("nested archive {outer_path} was not retained"))
        })();
        match result {
            Ok(_paths) => {
                data.status_text = format!("Expanded archive {}", outer_path);
                if let Some(pack) = &self.pack {
                    data.get_sarc_paths(pack);
                } else if let Some(root) = &self.archive {
                    data.sarc_paths.paths = root.paths();
                }
                self.add_nested_paths(&mut data);
            }
            Err(error) => {
                data.tab = "ERROR".to_string();
                data.status_text = format!("Error: unable to expand archive: {error}");
            }
        }
        data
    }

    pub fn edit_nested_sarc_file(&mut self, outer_path: String, inner_path: String) -> SendData {
        let mut data = SendData::default();
        let result = self
            .nested_archives
            .get_mut(&outer_path)
            .and_then(|archive| archive.get(&inner_path))
            .and_then(|bytes| {
                get_string_from_data(inner_path.clone(), bytes.to_vec(), self.zstd.clone())
            });
        if let Some((internal, text)) = result {
            self.nested_edit = Some((outer_path.clone(), inner_path.clone()));
            data.tab = "YAML".to_string();
            data.text = text;
            data.lang = "yaml".to_string();
            data.path = Pathlib::new(&inner_path);
            data.status_text = format!("Opened {} inside {}", inner_path, outer_path);
            data.get_file_label(internal.file_type, internal.endian);
            self.internal_file = Some(internal);
        } else {
            data.tab = "ERROR".to_string();
            data.status_text = format!("Error: unsupported or missing nested entry {}", inner_path);
        }
        data
    }

    pub fn extract_nested_sarc_file(
        &mut self,
        outer_path: String,
        inner_path: String,
    ) -> Option<SendData> {
        let bytes = self
            .nested_archives
            .get_mut(&outer_path)?
            .get(&inner_path)?
            .to_vec();
        let destination = FileDialog::new()
            .set_file_name(Pathlib::new(&inner_path).name)
            .save_file()?;
        fs::write(&destination, bytes).ok()?;
        let mut data = SendData::default();
        data.status_text = format!(
            "Extracted {} from {} to {}",
            inner_path,
            outer_path,
            destination.display()
        );
        Some(data)
    }

    pub fn mutate_nested_archive(
        &mut self,
        chain: String,
        path: String,
        action: String,
        new_path: Option<String>,
        source_path: Option<String>,
    ) -> SendData {
        if action == "compare" {
            let mut data = self.archive_send_data(format!("Comparing nested entry {path}"));
            let result = (|| -> Result<(String, String), String> {
                let bytes = self
                    .nested_archives
                    .get_mut(&chain)
                    .ok_or("nested archive is not expanded")?
                    .get(&path)
                    .ok_or_else(|| format!("nested entry not found: {path}"))?
                    .to_vec();
                let current = get_string_from_data(&path, bytes, self.zstd.clone())
                    .map(|(_, text)| text)
                    .ok_or_else(|| format!("unsupported nested entry type: {path}"))?;
                let original = self
                    .zstd
                    .find_vanila_internal_file_data_in_romfs(&path, self.zstd.clone())
                    .map_err(|error| error.to_string())?;
                Ok((current, original))
            })();
            match result {
                Ok((current, original)) => {
                    data.tab = "COMPARER".into();
                    data.status_text = format!("Compared nested entry {path}");
                    data.compare_data.file1.text = current;
                    data.compare_data.file1.label = format!("{path} (nested archive)");
                    data.compare_data.file2.text = original;
                    data.compare_data.file2.label = "Original".into();
                }
                Err(error) => {
                    data.tab = "ERROR".into();
                    data.status_text = format!("Error: failed to compare {path}: {error}");
                }
            }
            return data;
        }
        let result = (|| -> Result<String, String> {
            match action.as_str() {
                "delete" => {
                    let count = self
                        .nested_archives
                        .get_mut(&chain)
                        .ok_or("nested archive is not expanded")?
                        .remove_prefix(&path)
                        .map_err(|e| e.to_string())?;
                    self.drop_nested_descendants(&chain, &path);
                    self.flush_nested_chain(&chain)?;
                    Ok(format!("Deleted {count} nested entries"))
                }
                "rename" => {
                    let target = new_path.ok_or("missing rename target")?;
                    let count = self
                        .nested_archives
                        .get_mut(&chain)
                        .ok_or("nested archive is not expanded")?
                        .rename_prefix(&path, &target)
                        .map_err(|e| e.to_string())?;
                    self.drop_nested_descendants(&chain, &path);
                    self.flush_nested_chain(&chain)?;
                    Ok(format!("Renamed {count} nested entries"))
                }
                "replace" | "add" => {
                    let source = source_path.ok_or("missing source file")?;
                    let bytes = fs::read(&source).map_err(|e| e.to_string())?;
                    self.nested_archives
                        .get_mut(&chain)
                        .ok_or("nested archive is not expanded")?
                        .set(&path, bytes);
                    if action == "replace" {
                        self.drop_nested_descendants(&chain, &path);
                    }
                    self.flush_nested_chain(&chain)?;
                    Ok(format!("Updated nested entry {path}"))
                }
                "add_dir" => {
                    let source = source_path.ok_or("missing source directory")?;
                    let files = list_files_recursively(&source);
                    for file in &files {
                        let relative = Path::new(file)
                            .strip_prefix(&source)
                            .map_err(|e| e.to_string())?
                            .to_string_lossy()
                            .replace('\\', "/");
                        let target = format!("{}/{}", path.trim_end_matches('/'), relative)
                            .trim_start_matches('/')
                            .to_string();
                        let bytes = fs::read(file).map_err(|e| e.to_string())?;
                        self.nested_archives
                            .get_mut(&chain)
                            .ok_or("nested archive is not expanded")?
                            .set(&target, bytes);
                    }
                    self.flush_nested_chain(&chain)?;
                    Ok(format!("Added {} nested files", files.len()))
                }
                "new_byml" => {
                    let mut index = 1;
                    loop {
                        let target = format!("{}/new_{index}.byml", path.trim_end_matches('/'))
                            .trim_start_matches('/')
                            .to_string();
                        if self
                            .nested_archives
                            .get_mut(&chain)
                            .ok_or("nested archive is not expanded")?
                            .get(&target)
                            .is_none()
                        {
                            self.nested_archives
                                .get_mut(&chain)
                                .ok_or("nested archive is not expanded")?
                                .set(&target, Byml::default().to_binary(roead::Endian::Little));
                            break;
                        }
                        index += 1;
                    }
                    self.flush_nested_chain(&chain)?;
                    Ok("Added nested BYML".into())
                }
                "extract_folder" => {
                    let destination = FileDialog::new().pick_folder().ok_or("cancelled")?;
                    let prefix = if path.is_empty() {
                        String::new()
                    } else {
                        format!("{}/", path.trim_end_matches('/'))
                    };
                    let paths: Vec<_> = self
                        .nested_archives
                        .get(&chain)
                        .ok_or("nested archive is not expanded")?
                        .paths()
                        .into_iter()
                        .filter(|p| p.starts_with(&prefix))
                        .collect();
                    for entry in &paths {
                        let output = destination.join(entry.strip_prefix(&prefix).unwrap_or(entry));
                        if let Some(parent) = output.parent() {
                            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                        }
                        let bytes = self
                            .nested_archives
                            .get_mut(&chain)
                            .ok_or("nested archive is not expanded")?
                            .get(entry)
                            .ok_or_else(|| format!("nested entry disappeared: {entry}"))?;
                        fs::write(output, bytes).map_err(|e| e.to_string())?;
                    }
                    Ok(format!("Extracted {} nested entries", paths.len()))
                }
                _ => Err(format!("unsupported nested action: {action}")),
            }
        })();
        match result {
            Ok(status) => self.archive_send_data(status),
            Err(error) => {
                let mut data = self.archive_send_data(format!("Error: {error}"));
                data.tab = "ERROR".into();
                data
            }
        }
    }

    pub fn search_in_sarc(&mut self, query: String) -> Option<SendData> {
        let mut data = SendData::default();
        let pattern = query.to_lowercase();
        data.tab = "SARC".to_string();
        // data.get_sarc_paths(&self.pack.as_ref().unwrap());
        // data.sarc_paths.paths = Vec::new();
        if let Some(pack) = &mut self.pack {
            data.get_sarc_paths(pack);
            data.sarc_paths.paths = Vec::new();
            if let Some(opened) = &mut pack.opened {
                for file in opened.sarc.files() {
                    if let Some((_, text)) =
                        get_string_from_data("".to_string(), file.data.to_vec(), self.zstd.clone())
                    {
                        let filename = file.name.unwrap_or_default().to_string();
                        if !filename.is_empty() && text.to_lowercase().contains(&pattern) {
                            data.sarc_paths.paths.push(filename);
                        }
                    }
                }
                data.sarc_paths
                    .paths
                    .sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
                data.status_text = format!(
                    "Found {} entries for \"{}\"",
                    data.sarc_paths.paths.len(),
                    &query
                );
                return Some(data);
            }
            data.status_text = format!("Error: No SARC opened for Pack comparer");
        }
        data.status_text = format!("Error: No SARC opened");
        return Some(data);
    }

    pub fn clear_search_in_sarc(&mut self) -> Option<SendData> {
        let mut data = SendData::default();
        data.tab = "SARC".to_string();
        if let Some(pack) = &mut self.pack {
            data.get_sarc_paths(pack);
            data.status_text = "Cleared search results".to_string();
            return Some(data);
        }
        None
    }

    pub fn open_from_path(&mut self, file_name: String) -> Option<SendData> {
        let mut data = SendData::default();
        let dictionary = if file_name.to_ascii_lowercase().ends_with(".zs") {
            fs::read(&file_name)
                .ok()
                .and_then(|bytes| self.zstd.try_decompress_with_dictionary(&bytes).ok())
                .map(|(_, dictionary)| dictionary)
        } else {
            None
        };
        //let file_name = file.to_string_lossy().to_string().replace("\\", "/");
        if check_if_filepath_valid(&file_name) {
            if let Some((pack, mut data)) = PackComparer::open_sarc(&file_name, self.zstd.clone()) {
                self.pack = Some(pack);
                self.internal_file = None;
                data.set_file_metadata(TotkFileType::Sarc, dictionary);
                return Some(data);
            }
            match ArchiveDocument::open(Path::new(&file_name)) {
                Ok(Some(archive)) => {
                    self.pack = None;
                    self.internal_file = None;
                    self.opened_file = OpenedFile::default();
                    self.archive = Some(archive);
                    let mut data = self.archive_send_data(format!("Opened archive {file_name}"));
                    data.set_file_metadata(TotkFileType::Sarc, dictionary);
                    return Some(data);
                }
                Ok(None) => {}
                Err(error) => {
                    data.tab = "ERROR".into();
                    data.status_text = format!("Error: {error}");
                    return Some(data);
                }
            }
            let res = file_from_disk_to_senddata(&file_name, self.zstd.clone());
            if let Some((opened, mut data)) = res {
                self.archive = None;
                data.set_file_metadata(opened.file_type, dictionary);
                self.opened_file = opened;
                self.internal_file = None;
                return Some(data);
            }
            // let res = open_tag(file_name.clone(), self.zstd.clone())
            //     .or_else(|| open_esetb(file_name.clone(), self.zstd.clone()))
            //     .or_else(|| open_restbl(file_name.clone(), self.zstd.clone()))
            //     .or_else(|| open_asb(file_name.clone(), self.zstd.clone()))
            //     .or_else(|| open_ainb(file_name.clone(), self.zstd.clone()))
            //     .or_else(|| open_byml(file_name.clone(), self.zstd.clone()))
            //     .or_else(|| open_msbt(file_name.clone()))
            //     .or_else(|| open_aamp(file_name.clone()))
            //     .or_else(|| open_text(file_name.clone()))
            //     .map(|(opened_file, data)| {
            //         self.opened_file = opened_file;
            //         self.internal_file = None;
            //         data
            //     });
            // if res.is_some() {
            //     return res;
            // }
        } else {
            return None;
        }
        data.tab = "ERROR".to_string();
        data.status_text = format!("Error: Failed to open {}", &file_name);
        return Some(data);
    }

    pub fn compare_files(&self, is_from_disk: bool) -> Option<SendData> {
        // let mut c = DiffComparer::default();
        // c.compare_by_choice(decision, &self.pack, &int_or_regular_path, self.zstd.clone(), is_from_disk)
        DiffComparer::files_from_disk(self.zstd.clone(), is_from_disk)
    }

    pub fn open(&mut self) -> Option<SendData> {
        if let Some(file) = FileDialog::new()
            .set_title("Choose file to open")
            .pick_file()
        {
            return self.open_from_path(file.to_string_lossy().to_string().replace("\\", "/"));
        }
        None
    }

    pub fn open_folder(&mut self) -> Option<SendData> {
        let folder = FileDialog::new()
            .set_title("Choose folder to open")
            .pick_folder()?;
        match ArchiveDocument::open_folder(&folder) {
            Ok(archive) => {
                self.pack = None;
                self.internal_file = None;
                self.opened_file = OpenedFile::default();
                self.archive = Some(archive);
                Some(self.archive_send_data(format!("Opened folder {}", folder.display())))
            }
            Err(error) => {
                let mut data = SendData::default();
                data.tab = "ERROR".into();
                data.status_text = format!("Error: {error}");
                Some(data)
            }
        }
    }

    pub fn compare_opened_file_with_original_monaco(&mut self) -> Option<SendData> {
        let mut data = SendData::default();
        if self.opened_file.path.full_path.is_empty() {
            data.status_text = "Error: No file from disk nor SARC opened".to_string();
            return Some(data);
        }
        let mut van_path = String::new();
        match self
            .zstd
            .clone()
            .totk_config
            .find_vanila_file_in_romfs(&self.opened_file.path.full_path)
        {
            Ok(p) => {
                van_path = p;
            }
            Err(e) => {
                eprintln!("{:?}", e);
                data.status_text = format!("Error: {} not found", &self.opened_file.path.name);
                return Some(data);
            }
        }
        println!(
            "Comparing {} with original {}",
            &self.opened_file.path.full_path, &van_path
        );
        let mut text2 = String::new();
        if let Some((_, t)) = file_from_disk_to_senddata(&van_path, self.zstd.clone()) {
            text2 = t.text;
        } else {
            data.status_text = format!(
                "Error: Failed to parse {}",
                &self.opened_file.path.full_path
            );
            return Some(data);
        }
        data.compare_data.file1.label = self.opened_file.path.full_path.clone();
        data.compare_data.file1.path = self.opened_file.path.clone();

        data.compare_data.file2.label = van_path.clone();
        data.compare_data.file2.path = Pathlib::new(van_path.clone());
        data.compare_data.file2.text = text2;
        data.tab = "COMPARE".to_string();
        data.status_text = format!(
            "Compared {} with original",
            &self.opened_file.path.full_path
        );

        //TODO: finish it
        //compare internal file
        // let mut data = SendData::default();
        // let vanila_sarc_path = self.zstd.clone().find_vanila_internal_file_path_in_romfs(&path, self.zstd.clone());

        Some(data)
    }

    pub fn compare_internal_file_with_original(
        &mut self,
        path: String,
        is_from_sarc: bool,
    ) -> Option<SendData> {
        let mut data = SendData::default();
        let mut path = path.clone();
        let mut text1 = String::new();
        let mut text2 = String::new();
        let is_from_monaco = !is_from_sarc;
        if is_from_monaco {
            //from monaco
            if let Some(internal_file) = &self.internal_file {
                path = internal_file.path.full_path.clone();
                data.compare_data.file1.label = format!("{} (from YAML Editor)", &path);
            } else {
                // data.status_text = "Error: No SARC internal file opened".to_string();
                // return Some(data);
                // Perhaps the file is from disk, check self.opened_file
                //simple fallback
                return self.compare_opened_file_with_original_monaco();
            }
        }
        println!("Comparing {} with original", &path);

        if let Some(pack) = &self.pack {
            if let Some(opened) = &pack.opened {
                if is_from_sarc {
                    if let Some(rawdata1) = opened.sarc.get_data(&path) {
                        text1 = get_string_from_data(&path, rawdata1.to_vec(), self.zstd.clone())
                            .map(|(_, t)| t)
                            .unwrap_or_default();
                    }
                }

                if let Some(vanila) = &pack.vanila {
                    if let Some(rawdata2) = vanila.sarc.get_data(&path) {
                        text2 = get_string_from_data(&path, rawdata2.to_vec(), self.zstd.clone())
                            .map(|(_, t)| t)
                            .unwrap_or_default();
                    }
                }
                // println!("{} {}", text1.len(), text2.len());
            }
        }

        // Internal editor child documents deliberately do not own their parent's
        // pack. Resolve the vanilla entry globally when it was not available from
        // a directly associated vanilla archive.
        if text2.is_empty() {
            text2 = self
                .zstd
                .clone()
                .find_vanila_internal_file_data_in_romfs(&path, self.zstd.clone())
                .unwrap_or_else(|err| {
                    data.status_text = format!("ERROR: {:?}", &err);
                    String::new()
                });
        }

        // println!("{} {}", text1.len(), text2.len());

        if (!text1.is_empty() || is_from_monaco) && !text2.is_empty() {
            data.tab = "COMPARE".to_string();
            data.compare_data.file1.text = text1;
            data.compare_data.file2.text = text2;
            data.status_text = format!("Compared {}", &path);
            if data.compare_data.file1.label.is_empty() {
                data.compare_data.file1.label = format!("{} (from SARC)", &path);
            }
            data.compare_data.file2.label = "Original".to_string();
        } else {
            data.status_text = format!("Error: Failed to compare {}", &path);
        }

        Some(data)
    }
}

pub struct InternalFile<'a> {
    pub path: Pathlib,
    pub file_type: TotkFileType,
    pub endian: Option<roead::Endian>,
    pub byml: Option<BymlFile<'a>>,
    pub msyt: Option<String>,
    pub text: Option<String>,
    pub aamp: Option<String>,
    pub esetb: Option<Esetb<'a>>,
    pub zstd_dictionary: Option<ZstdDictionary>,
}

impl Default for InternalFile<'_> {
    fn default() -> Self {
        Self {
            path: Pathlib::default(),
            file_type: TotkFileType::None,
            endian: None,
            byml: None,
            msyt: None,
            text: None,
            aamp: None,
            esetb: None,
            zstd_dictionary: None,
        }
    }
}

impl InternalFile<'_> {
    #[allow(dead_code)]
    pub fn new(path: String) -> Self {
        let path = Pathlib::new(path);
        Self {
            path,
            file_type: TotkFileType::None,
            endian: None,
            byml: None,
            msyt: None,
            text: None,
            aamp: None,
            esetb: None,
            zstd_dictionary: None,
        }
    }
}

pub fn check_if_filepath_valid(path: &str) -> bool {
    if path.is_empty() {
        return false;
    }
    let path = Path::new(path);
    path.exists() && path.is_file()
}
