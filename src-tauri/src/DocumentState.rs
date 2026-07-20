use serde::Serialize;
use std::{collections::HashMap, sync::Mutex};

use crate::{
    Open_and_Save::SendData,
    TotkApp::{SaveData, TotkBitsApp},
    Zstd::TotkFileType,
};

#[derive(Default)]
pub struct DocumentState {
    documents: Mutex<HashMap<String, TotkBitsApp<'static>>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenBphclDocument {
    pub document_id: String,
    pub label: String,
    pub path: String,
    pub location: String,
    pub cloth_count: usize,
    pub collidable_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BphclSelectableNode {
    pub document_id: String,
    pub node_id: String,
    pub kind: String,
    pub index: usize,
    pub name: String,
    pub item_index: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BphclMergeResult {
    pub selected_count: usize,
    pub imported_count: usize,
    pub skipped_count: usize,
    pub cloth_count: usize,
    pub collidable_count: usize,
    pub imported: Vec<String>,
    pub skipped: Vec<BphclMergeSkip>,
    pub sarc_paths: crate::file_format::Pack::SarcPaths,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BphclMergeSkip {
    pub name: String,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BphclMutationResult {
    pub status_text: String,
    pub sarc_paths: crate::file_format::Pack::SarcPaths,
}

impl DocumentState {
    pub fn remove_bphcl_node(
        &self,
        document_id: &str,
        path: &str,
    ) -> Result<BphclMutationResult, String> {
        let normalized = path.replace('\\', "/");
        let parts: Vec<_> = normalized.split('/').collect();
        let (kind, file_name) = parts
            .windows(2)
            .find_map(|parts| match parts[0] {
                "Cloth" => Some(("cloth", parts[1])),
                "Collidables" => Some(("collidable", parts[1])),
                _ => None,
            })
            .ok_or_else(|| "Only cloth and collidable nodes can be removed".to_string())?;
        let index = file_name
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or_else(|| format!("Cannot determine BPHCL node index from '{file_name}'"))?;

        let mut documents = self.documents();
        let app = documents
            .get(document_id)
            .ok_or_else(|| format!("Document '{document_id}' is not open"))?;
        let file = app
            .opened_file
            .bphcl
            .as_ref()
            .ok_or_else(|| format!("Document '{document_id}' is not a BPHCL document"))?;
        let source_path = file.source_path.clone();
        let parent_link = app.internal_parent.clone();
        let display_name = if kind == "cloth" {
            file.document
                .cloth
                .get(index)
                .map(|node| node.name.clone())
                .ok_or_else(|| format!("Cloth {index} no longer exists"))?
        } else {
            file.document
                .collidables
                .get(index)
                .map(|node| node.name.clone())
                .ok_or_else(|| format!("Collidable {index} no longer exists"))?
        };
        let bytes = if kind == "cloth" {
            file.document.remove_cloth(index)
        } else {
            file.document.remove_collidable(index)
        }
        .map_err(|error| error.to_string())?;
        let rebuilt = crate::parser::bphcl::BphclDocument::parse(&bytes)
            .map_err(|error| format!("Removed BPHCL did not reparse: {error}"))?;

        if let Some(link) = &parent_link {
            documents
                .get_mut(&link.document_id)
                .ok_or_else(|| format!("Parent document '{}' is not open", link.document_id))?
                .update_child_entry(link.outer_path.as_deref(), &link.inner_path, bytes)?;
        }
        let target = documents
            .get_mut(document_id)
            .ok_or_else(|| format!("Document '{document_id}' was closed during removal"))?;
        target.opened_file.bphcl = Some(crate::file_format::bphcl::BphclFile {
            source_path,
            document: rebuilt,
        });
        let root_name = if target.opened_file.path.name.is_empty() {
            "modified.bphcl".to_string()
        } else {
            target.opened_file.path.name.clone()
        };
        let mut sarc_paths = crate::file_format::Pack::SarcPaths::default();
        sarc_paths.read_only = true;
        sarc_paths.paths = target
            .opened_file
            .bphcl
            .as_ref()
            .expect("modified BPHCL was just assigned")
            .leaves()
            .map_err(|error| format!("Failed to refresh BPHCL tree: {error}"))?
            .into_iter()
            .map(|leaf| format!("{root_name}/{}", leaf.path))
            .collect();
        Ok(BphclMutationResult {
            status_text: format!("Removed {kind} '{display_name}'"),
            sarc_paths,
        })
    }

    pub fn merge_bphcl_nodes(
        &self,
        target_document_id: &str,
        source_document_id: &str,
        node_ids: &[String],
    ) -> Result<BphclMergeResult, String> {
        if target_document_id == source_document_id {
            return Err("Source and target BPHCL documents must be different".into());
        }
        if node_ids.is_empty() {
            return Err("Select at least one cloth or collidable to merge".into());
        }

        let mut documents = self.documents();
        let source = documents
            .get(source_document_id)
            .ok_or_else(|| format!("Document '{source_document_id}' is not open"))?
            .opened_file
            .bphcl
            .as_ref()
            .ok_or_else(|| format!("Document '{source_document_id}' is not a BPHCL document"))?
            .document
            .clone();
        let target_app = documents
            .get(target_document_id)
            .ok_or_else(|| format!("Document '{target_document_id}' is not open"))?;
        let target_file =
            target_app.opened_file.bphcl.as_ref().ok_or_else(|| {
                format!("Document '{target_document_id}' is not a BPHCL document")
            })?;
        let target_source_path = target_file.source_path.clone();
        let target_link = target_app.internal_parent.clone();
        let mut merged = target_file.document.clone();
        let mut imported = Vec::new();
        let mut skipped = Vec::new();
        let mut seen = std::collections::HashSet::new();

        for node_id in node_ids {
            if !seen.insert(node_id.as_str()) {
                skipped.push(BphclMergeSkip {
                    name: node_id.clone(),
                    reason: "Duplicate selection".into(),
                });
                continue;
            }
            let (kind, index) = node_id
                .split_once(':')
                .ok_or_else(|| format!("Invalid BPHCL node ID '{node_id}'"))?;
            let index: usize = index
                .parse()
                .map_err(|_| format!("Invalid BPHCL node ID '{node_id}'"))?;
            let before = merged.raw.clone();
            let (display_name, bytes) = match kind {
                "cloth" => {
                    let position = source
                        .cloth
                        .iter()
                        .position(|cloth| cloth.index == index)
                        .ok_or_else(|| format!("Source cloth '{node_id}' no longer exists"))?;
                    let name = source.cloth[position].name.clone();
                    (
                        format!("Cloth: {name}"),
                        merged.merge_complete_cloth(&source, position),
                    )
                }
                "collidable" => {
                    let position = source
                        .collidables
                        .iter()
                        .position(|collidable| collidable.index == index)
                        .ok_or_else(|| format!("Source collidable '{node_id}' no longer exists"))?;
                    let name = source.collidables[position].name.clone();
                    (
                        format!("Collidable: {name}"),
                        merged.merge_collidable(&source, position),
                    )
                }
                _ => (
                    node_id.clone(),
                    Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("Unsupported BPHCL node kind '{kind}'"),
                    )),
                ),
            };
            let bytes = match bytes {
                Ok(bytes) => bytes,
                Err(error)
                    if kind == "collidable"
                        && error
                            .to_string()
                            .contains("target already has a different collidable named") =>
                {
                    skipped.push(BphclMergeSkip {
                        name: display_name,
                        reason: error.to_string(),
                    });
                    continue;
                }
                Err(error) => return Err(format!("Failed to merge '{node_id}': {error}")),
            };
            if bytes == before {
                skipped.push(BphclMergeSkip {
                    name: display_name,
                    reason: if kind == "cloth" {
                        "Target already contains a cloth with this name"
                    } else {
                        "Target already contains an identical collidable"
                    }
                    .into(),
                });
            } else {
                imported.push(display_name);
                merged = crate::parser::bphcl::BphclDocument::parse(&bytes)
                    .map_err(|error| format!("Merged BPHCL did not reparse: {error}"))?;
            }
        }

        let merged_bytes = merged.raw.clone();
        if let Some(link) = &target_link {
            documents
                .get_mut(&link.document_id)
                .ok_or_else(|| format!("Parent document '{}' is not open", link.document_id))?
                .update_child_entry(link.outer_path.as_deref(), &link.inner_path, merged_bytes)?;
        }
        let cloth_count = merged.cloth.len();
        let collidable_count = merged.collidables.len();
        let target = documents
            .get_mut(target_document_id)
            .ok_or_else(|| format!("Document '{target_document_id}' was closed during merge"))?;
        target.opened_file.bphcl = Some(crate::file_format::bphcl::BphclFile {
            source_path: target_source_path,
            document: merged,
        });
        let root_name = if target.opened_file.path.name.is_empty() {
            "merged.bphcl".to_string()
        } else {
            target.opened_file.path.name.clone()
        };
        let mut sarc_paths = crate::file_format::Pack::SarcPaths::default();
        sarc_paths.read_only = true;
        sarc_paths.paths = target
            .opened_file
            .bphcl
            .as_ref()
            .expect("merged BPHCL was just assigned")
            .leaves()
            .map_err(|error| format!("Failed to refresh merged BPHCL tree: {error}"))?
            .into_iter()
            .map(|leaf| format!("{root_name}/{}", leaf.path))
            .collect();

        Ok(BphclMergeResult {
            selected_count: node_ids.len(),
            imported_count: imported.len(),
            skipped_count: skipped.len(),
            cloth_count,
            collidable_count,
            imported,
            skipped,
            sarc_paths,
        })
    }

    pub fn open_bphcl_documents(&self) -> Vec<OpenBphclDocument> {
        let documents = self.documents();
        let mut result: Vec<_> = documents
            .iter()
            .filter_map(|(document_id, app)| {
                let bphcl = app.opened_file.bphcl.as_ref()?;
                let path = app.opened_file.path.full_path.clone();
                let label = if app.opened_file.path.name.is_empty() {
                    document_id.clone()
                } else {
                    app.opened_file.path.name.clone()
                };
                let location = match app
                    .internal_parent
                    .as_ref()
                    .and_then(|link| link.outer_path.as_ref())
                {
                    Some(_) => "nested-archive",
                    None if app.internal_parent.is_some() => "archive",
                    None => "disk",
                };
                Some(OpenBphclDocument {
                    document_id: document_id.clone(),
                    label,
                    path,
                    location: location.into(),
                    cloth_count: bphcl.document.cloth.len(),
                    collidable_count: bphcl.document.collidables.len(),
                })
            })
            .collect();
        result.sort_by(|left, right| left.document_id.cmp(&right.document_id));
        result
    }

    pub fn bphcl_selectable_nodes(
        &self,
        document_id: &str,
    ) -> Result<Vec<BphclSelectableNode>, String> {
        let documents = self.documents();
        let app = documents
            .get(document_id)
            .ok_or_else(|| format!("Document '{document_id}' is not open"))?;
        let bphcl = app
            .opened_file
            .bphcl
            .as_ref()
            .ok_or_else(|| format!("Document '{document_id}' is not a BPHCL document"))?;
        let mut nodes =
            Vec::with_capacity(bphcl.document.cloth.len() + bphcl.document.collidables.len());
        nodes.extend(
            bphcl
                .document
                .cloth
                .iter()
                .map(|cloth| BphclSelectableNode {
                    document_id: document_id.into(),
                    node_id: format!("cloth:{}", cloth.index),
                    kind: "cloth".into(),
                    index: cloth.index,
                    name: cloth.name.clone(),
                    item_index: cloth.item_index,
                }),
        );
        nodes.extend(
            bphcl
                .document
                .collidables
                .iter()
                .map(|collidable| BphclSelectableNode {
                    document_id: document_id.into(),
                    node_id: format!("collidable:{}", collidable.index),
                    kind: "collidable".into(),
                    index: collidable.index,
                    name: collidable.name.clone(),
                    item_index: collidable.item_index,
                }),
        );
        Ok(nodes)
    }
    pub fn open_bphcl_leaf(
        &self,
        parent_id: &str,
        child_id: &str,
        path: String,
    ) -> Option<SendData> {
        let mut documents = self.documents();
        let leaf = documents
            .get(parent_id)?
            .opened_file
            .bphcl
            .as_ref()?
            .leaf(&path)
            .ok()?;
        documents.entry(child_id.to_owned()).or_default();
        let mut data = SendData::default();
        data.text = leaf.yaml;
        data.tab = "YAML".into();
        data.lang = "yaml".into();
        let file_name = path.replace('\\', "/").rsplit('/').next()?.to_owned();
        data.path = crate::Settings::Pathlib::new(&file_name);
        data.file_label = format!("{file_name} [{}] [ReadOnly]", leaf.viewer_type);
        data.file_metadata = format!("[{}] [ReadOnly]", leaf.viewer_type);
        data.status_text = format!("Opened read-only BPHCL leaf: {path}");
        data.read_only = true;
        Some(data)
    }
    fn documents(&self) -> std::sync::MutexGuard<'_, HashMap<String, TotkBitsApp<'static>>> {
        match self.documents.lock() {
            Ok(documents) => documents,
            Err(poisoned) => {
                // A panic may have interrupted a multi-document update. Discard the
                // potentially partial graph rather than exposing inconsistent state.
                let mut documents = poisoned.into_inner();
                documents.clear();
                self.documents.clear_poison();
                documents
            }
        }
    }

    pub fn open_internal_child(
        &self,
        parent_id: &str,
        child_id: &str,
        outer_path: Option<String>,
        path: String,
    ) -> Option<SendData> {
        let mut documents = self.documents();
        let bytes = documents
            .get_mut(parent_id)?
            .internal_entry_bytes(outer_path.as_deref(), &path)?;
        documents
            .entry(child_id.to_owned())
            .or_default()
            .open_internal_child(parent_id.to_owned(), outer_path, path, bytes)
    }

    pub fn save_document(&self, id: &str, save_data: SaveData) -> Option<SendData> {
        let mut documents = self.documents();
        let link = documents.get(id)?.internal_parent.clone();
        let Some(link) = link else {
            return documents.get_mut(id)?.save(save_data);
        };
        if !documents.contains_key(&link.document_id) {
            let child = documents.get_mut(id)?;
            let result = child.save_as(save_data);
            if result.is_some() {
                if let Some(internal) = &child.internal_file {
                    child.opened_file.file_type = internal.file_type;
                    child.opened_file.endian = internal.endian;
                }
                child.internal_parent = None;
                child.internal_file = None;
            }
            return result;
        }
        let bytes = documents.get_mut(id)?.internal_binary(&save_data.text)?;
        documents
            .get_mut(&link.document_id)?
            .update_child_entry(link.outer_path.as_deref(), &link.inner_path, bytes)
            .ok()
    }

    pub fn compare_internal_file(
        &self,
        id: &str,
        internal_path: String,
        is_from_sarc: bool,
    ) -> Option<SendData> {
        let mut documents = self.documents();
        let msbt_parent = documents.get(id).and_then(|child| {
            let link = child.internal_parent.as_ref()?;
            let internal = child.internal_file.as_ref()?;
            (!is_from_sarc && link.outer_path.is_none() && internal.file_type == TotkFileType::Msbt)
                .then(|| (link.document_id.clone(), link.inner_path.clone()))
        });
        if let Some((parent_id, path)) = msbt_parent {
            if let Some(parent) = documents.get_mut(&parent_id) {
                return parent.compare_internal_file_with_original(path, true);
            }
        }
        documents
            .get_mut(id)?
            .compare_internal_file_with_original(internal_path, is_from_sarc)
    }
    pub fn with_mut<T>(
        &self,
        id: &str,
        operation: impl FnOnce(&mut TotkBitsApp<'static>) -> T,
    ) -> T {
        let mut documents = self.documents();
        operation(documents.entry(id.to_owned()).or_default())
    }

    pub fn with<T>(&self, id: &str, operation: impl FnOnce(&TotkBitsApp<'static>) -> T) -> T {
        let mut documents = self.documents();
        operation(documents.entry(id.to_owned()).or_default())
    }

    pub fn close(&self, id: &str) -> bool {
        self.documents().remove(id).is_some()
    }

    pub fn close_all(&self) {
        self.documents().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::DocumentState;

    #[test]
    fn documents_are_created_lazily_and_closed_independently() {
        let state = DocumentState::default();
        state.with_mut("doc-a", |app| app.opened_file.path.name = "a".into());
        state.with_mut("doc-b", |app| app.opened_file.path.name = "b".into());
        assert_eq!(
            state.with("doc-a", |app| app.opened_file.path.name.clone()),
            "a"
        );
        assert!(state.close("doc-a"));
        assert_eq!(
            state.with("doc-b", |app| app.opened_file.path.name.clone()),
            "b"
        );
    }
}
