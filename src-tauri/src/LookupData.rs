use flate2::read::ZlibDecoder;
use serde::de::DeserializeOwned;
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

fn load_zlib_json<T: DeserializeOwned + Default>(name: &str) -> T {
    let paths = [
        crate::Settings::exe_relative_path(format!("misc/{name}")),
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("misc")
            .join(name),
    ];
    let mut last_error = None;
    for path in paths {
        match decode_zlib_json(&path) {
            Ok(value) => return value,
            Err(error) => last_error = Some((path, error)),
        }
    }
    if let Some((path, error)) = last_error {
        eprintln!("Failed to load lookup data {}: {error}", path.display());
    }
    T::default()
}

fn decode_zlib_json<T: DeserializeOwned>(path: &PathBuf) -> std::io::Result<T> {
    let compressed = std::fs::read(path)?;
    let mut json = String::new();
    ZlibDecoder::new(compressed.as_slice()).read_to_string(&mut json)?;
    serde_json::from_str(&json).map_err(std::io::Error::other)
}

static SARC_SHA256: LazyLock<Arc<HashMap<String, String>>> =
    LazyLock::new(|| Arc::new(load_zlib_json("totk_sarc_sha256.bin")));
static RSTB_PATHS: LazyLock<Arc<Vec<String>>> =
    LazyLock::new(|| Arc::new(load_zlib_json("totk_rstb_paths.bin")));
static INTERNAL_FILEPATHS: LazyLock<Arc<HashMap<String, String>>> =
    LazyLock::new(|| Arc::new(load_zlib_json("totk_internal_filepaths.bin")));
static FILENAME_TO_LOCALPATH: LazyLock<Arc<HashMap<String, String>>> =
    LazyLock::new(|| Arc::new(load_zlib_json("totk_filename_to_localpath.bin")));

pub fn sarc_sha256() -> Arc<HashMap<String, String>> {
    Arc::clone(&SARC_SHA256)
}

pub fn rstb_paths() -> Arc<Vec<String>> {
    Arc::clone(&RSTB_PATHS)
}

pub fn internal_filepaths() -> &'static HashMap<String, String> {
    INTERNAL_FILEPATHS.as_ref()
}

pub fn filename_to_localpath() -> &'static HashMap<String, String> {
    FILENAME_TO_LOCALPATH.as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_lookup_tables_are_initialized_once_and_shared() {
        let sarc_first = sarc_sha256();
        let sarc_second = sarc_sha256();
        assert!(!sarc_first.is_empty());
        assert!(Arc::ptr_eq(&sarc_first, &sarc_second));

        let rstb_first = rstb_paths();
        let rstb_second = rstb_paths();
        assert!(!rstb_first.is_empty());
        assert!(Arc::ptr_eq(&rstb_first, &rstb_second));

        assert!(!internal_filepaths().is_empty());
        assert!(std::ptr::eq(internal_filepaths(), internal_filepaths()));
        assert!(!filename_to_localpath().is_empty());
        assert!(std::ptr::eq(
            filename_to_localpath(),
            filename_to_localpath()
        ));
    }
}
