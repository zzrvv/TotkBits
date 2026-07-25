use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Returns the directory containing the running executable.
///
/// Resolution tries Rust's native API, Win32's module API, argv[0], and the
/// process `_` environment entry, in that order.
pub fn running_exe_dir() -> std::io::Result<PathBuf> {
    let parent = |path: PathBuf| path.parent().map(PathBuf::from);
    if let Ok(path) = std::env::current_exe() {
        if let Some(dir) = parent(path) {
            return Ok(dir);
        }
    }
    #[cfg(windows)]
    {
        use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
        let mut buffer = vec![0u16; 32768];
        let length = unsafe { GetModuleFileNameW(None, &mut buffer) } as usize;
        if length > 0 && length < buffer.len() {
            if let Some(dir) = parent(PathBuf::from(String::from_utf16_lossy(&buffer[..length]))) {
                return Ok(dir);
            }
        }
    }
    if let Some(path) = std::env::args_os().next().map(PathBuf::from) {
        let path = path.canonicalize().unwrap_or(path);
        if let Some(dir) = parent(path) {
            return Ok(dir);
        }
    }
    if let Some(path) = std::env::var_os("_").map(PathBuf::from) {
        let path = path.canonicalize().unwrap_or(path);
        if let Some(dir) = parent(path) {
            return Ok(dir);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "unable to locate the running executable directory",
    ))
}

pub fn exe_relative_path(path: impl AsRef<Path>) -> PathBuf {
    running_exe_dir()
        .map(|dir| dir.join(path.as_ref()))
        .unwrap_or_else(|_| path.as_ref().to_path_buf())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Pathlib {
    pub parent: String,
    pub name: String,
    pub stem: String,
    pub extension: String,
    pub ext_last: String,
    pub full_path: String,
}

impl Default for Pathlib {
    fn default() -> Self {
        Self::new("")
    }
}

impl Pathlib {
    pub fn new<P: AsRef<Path>>(path: P) -> Self {
        let path_str = path.as_ref().to_str().unwrap_or_default().to_string();
        Self {
            parent: Self::get_parent(&path),
            name: Self::get_name(&path),
            stem: Self::get_stem(&path),
            extension: Self::get_extension(&path),
            ext_last: Self::get_ext_last(&path),
            full_path: path_str,
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.full_path.is_empty()
    }

    #[inline]
    pub fn is_file(&self) -> bool {
        Path::new(&self.full_path).is_file()
    }

    #[inline]
    pub fn is_dir(&self) -> bool {
        Path::new(&self.full_path).is_dir()
    }

    #[inline]
    pub fn exists(&self) -> bool {
        Path::new(&self.full_path).exists()
    }

    pub fn get_ext_last<P: AsRef<Path>>(path: P) -> String {
        let extension = Self::get_extension(&path);
        if !extension.contains('.') {
            return String::new();
        }
        extension
            .split('.')
            .next_back()
            .unwrap_or_default()
            .to_string()
    }

    pub fn get_parent<P: AsRef<Path>>(path: P) -> String {
        path.as_ref()
            .parent()
            .and_then(Path::to_str)
            .unwrap_or_default()
            .replace('\\', "/")
    }

    pub fn get_name<P: AsRef<Path>>(path: P) -> String {
        path.as_ref()
            .file_name()
            .and_then(|part| part.to_str())
            .unwrap_or_default()
            .replace('\\', "/")
    }

    pub fn get_stem<P: AsRef<Path>>(path: P) -> String {
        let stem = path
            .as_ref()
            .file_stem()
            .and_then(|part| part.to_str())
            .unwrap_or_default()
            .replace('\\', "/");
        stem.split('.').next().unwrap_or_default().to_string()
    }

    pub fn get_extension<P: AsRef<Path>>(path: P) -> String {
        let path_str = path.as_ref().to_str().unwrap_or_default();
        match path_str.matches('.').count() {
            0 => String::new(),
            1 => path
                .as_ref()
                .extension()
                .and_then(|part| part.to_str())
                .unwrap_or_default()
                .replace('\\', "/"),
            _ => path_str.split('.').skip(1).collect::<Vec<_>>().join("."),
        }
    }

    fn is_aamp_path<P: AsRef<Path>>(path: P) -> bool {
        let path = path.as_ref().to_string_lossy().to_ascii_lowercase();

        [
            ".bxml",
            ".baiprog",
            ".bas",
            ".baslist",
            ".bgparamlist",
            ".bmodellist",
            ".bphysics",
            ".bphyssb",
        ]
        .iter()
        .any(|ext| path.ends_with(ext))
    }
}
