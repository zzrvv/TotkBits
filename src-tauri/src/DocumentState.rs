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

impl DocumentState {
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
