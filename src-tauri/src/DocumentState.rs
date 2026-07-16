use std::{collections::HashMap, sync::Mutex};

use crate::TotkApp::TotkBitsApp;

#[derive(Default)]
pub struct DocumentState {
    documents: Mutex<HashMap<String, TotkBitsApp<'static>>>,
}

impl DocumentState {
    pub fn with_mut<T>(
        &self,
        id: &str,
        operation: impl FnOnce(&mut TotkBitsApp<'static>) -> T,
    ) -> T {
        let mut documents = self
            .documents
            .lock()
            .expect("Failed to lock document state");
        operation(documents.entry(id.to_owned()).or_default())
    }

    pub fn with<T>(&self, id: &str, operation: impl FnOnce(&TotkBitsApp<'static>) -> T) -> T {
        let mut documents = self
            .documents
            .lock()
            .expect("Failed to lock document state");
        operation(documents.entry(id.to_owned()).or_default())
    }

    pub fn close(&self, id: &str) -> bool {
        self.documents
            .lock()
            .expect("Failed to lock document state")
            .remove(id)
            .is_some()
    }

    pub fn close_all(&self) {
        self.documents
            .lock()
            .expect("Failed to lock document state")
            .clear();
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
