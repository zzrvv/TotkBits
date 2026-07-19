use std::{
    ffi::CStr,
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

use libloading::{Library, Symbol};
use roead::Endian;

use crate::{
    file_format::BinTextFile::OpenedFile,
    Open_and_Save::SendData,
    Settings::{running_exe_dir, Pathlib},
    TotkApp::InternalFile,
    Zstd::{get_executable_dir, is_xlink, is_xlink_path, TotkFileType, TotkZstd, ZstdDictionary},
};

type XlinkBinaryToYaml = unsafe extern "C" fn(data: *const i8, size: usize) -> *const i8;
type XlinkYamlToBinary =
    unsafe extern "C" fn(data: *const i8, size: usize, out_size: *mut usize) -> *mut i8;
type FreeXlinkBinary = unsafe extern "C" fn(data: *mut i8);
type FreeXlinkString = unsafe extern "C" fn(data: *mut i8);

/// A lazily-created handle to the optional native XLink converter.
///
/// Keeping `Library` in this value guarantees that resolved symbols remain valid
/// for the duration of each conversion. Construction is only attempted when an
/// XLink file is actually opened or saved, so a missing DLL cannot break startup.
pub struct Xlink_rs<'a> {
    pub zstd: Arc<TotkZstd<'a>>,
    library: Library,
    library_path: PathBuf,
}

impl<'a> Xlink_rs<'a> {
    pub fn new(zstd: Arc<TotkZstd<'a>>) -> io::Result<Self> {
        let library_path = Self::find_library()?;
        let library = unsafe { Library::new(&library_path) }.map_err(|error| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "failed to load XLink converter DLL {}: {error}",
                    library_path.display()
                ),
            )
        })?;
        Ok(Self {
            zstd,
            library,
            library_path,
        })
    }

    fn find_library() -> io::Result<PathBuf> {
        let relative = running_exe_dir()?
            .join("bin")
            .join("dlls")
            .join("xlink_tool.dll");
        let mut candidates = Vec::new();
        if let Ok(current_dir) = std::env::current_dir() {
            candidates.push(current_dir.join(&relative));
        }
        if let Ok(executable) = std::env::current_exe() {
            if let Some(directory) = executable.parent() {
                candidates.push(directory.join(&relative));
            }
        }
        if let Some(path) = candidates.iter().find(|path| path.is_file()) {
            return Ok(path.clone());
        }
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "XLink converter DLL was not found; checked {}",
                candidates
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ))
    }

    fn symbol_error(&self, symbol: &str, error: libloading::Error) -> io::Error {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "XLink DLL {} does not export {symbol}: {error}",
                self.library_path.display()
            ),
        )
    }

    pub fn binary_to_yaml(&self, data: &[u8]) -> io::Result<String> {
        let rawdata = if is_xlink(data) {
            data.to_vec()
        } else {
            self.zstd.decompressor.decompress_zs(&data.to_vec())?
        };
        if !is_xlink(&rawdata) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a valid XLink binary (missing XLNK magic)",
            ));
        }

        let convert: Symbol<XlinkBinaryToYaml> =
            unsafe { self.library.get(b"xlink_binary_to_yaml\0") }
                .map_err(|error| self.symbol_error("xlink_binary_to_yaml", error))?;
        let free: Symbol<FreeXlinkString> = unsafe { self.library.get(b"free_xlink_string\0") }
            .map_err(|error| self.symbol_error("free_xlink_string", error))?;

        unsafe {
            let yaml_ptr = convert(rawdata.as_ptr() as *const i8, rawdata.len());
            if yaml_ptr.is_null() {
                return Err(io::Error::other(
                    "XLink DLL failed to convert binary to YAML",
                ));
            }
            let yaml_bytes = CStr::from_ptr(yaml_ptr).to_bytes().to_vec();
            free(yaml_ptr as *mut i8);
            String::from_utf8(yaml_bytes).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("XLink DLL returned invalid UTF-8 YAML: {error}"),
                )
            })
        }
    }

    pub fn yaml_to_binary(&self, data: &str) -> io::Result<Vec<u8>> {
        let convert: Symbol<XlinkYamlToBinary> =
            unsafe { self.library.get(b"xlink_yaml_to_binary\0") }
                .map_err(|error| self.symbol_error("xlink_yaml_to_binary", error))?;
        let free: Symbol<FreeXlinkBinary> = unsafe { self.library.get(b"free_xlink_binary\0") }
            .map_err(|error| self.symbol_error("free_xlink_binary", error))?;

        let mut out_size = 0;
        unsafe {
            let binary_ptr = convert(data.as_ptr() as *const i8, data.len(), &mut out_size);
            if binary_ptr.is_null() {
                return Err(io::Error::other(
                    "XLink DLL failed to convert YAML to binary",
                ));
            }
            let binary = std::slice::from_raw_parts(binary_ptr as *const u8, out_size).to_vec();
            free(binary_ptr);
            if !is_xlink(&binary) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "XLink DLL returned binary data without XLNK magic",
                ));
            }
            Ok(binary)
        }
    }

    pub fn open_internal<P: AsRef<Path>>(
        path: P,
        data: &[u8],
        zstd: Arc<TotkZstd<'a>>,
    ) -> Option<(InternalFile<'static>, String)> {
        let path = path.as_ref();
        if !is_xlink_path(path) && !is_xlink(data) {
            return None;
        }
        let text = match Self::new(zstd).and_then(|xlink| xlink.binary_to_yaml(data)) {
            Ok(text) => text,
            Err(error) => {
                println!("Unable to parse XLink entry {}: {error}", path.display());
                return None;
            }
        };
        let mut internal = InternalFile::default();
        internal.endian = Some(Endian::Little);
        internal.path = Pathlib::new(path);
        internal.file_type = TotkFileType::Xlink;
        Some((internal, text))
    }

    pub fn text_to_binary(
        text: &str,
        file_path: &str,
        zstd: Arc<TotkZstd<'a>>,
        dictionary: Option<ZstdDictionary>,
    ) -> Option<Vec<u8>> {
        let data = match Self::new(zstd.clone()).and_then(|xlink| xlink.yaml_to_binary(text)) {
            Ok(data) => data,
            Err(error) => {
                println!("Unable to save XLink YAML for {file_path}: {error}");
                return None;
            }
        };
        if file_path.to_ascii_lowercase().ends_with(".zs") && dictionary.is_none() {
            zstd.cpp_compressor.compress_zs(&data).ok()
        } else {
            Some(data)
        }
    }

    pub fn open_xlink<P: AsRef<Path>>(
        path: P,
        zstd: Arc<TotkZstd<'a>>,
    ) -> Option<(OpenedFile<'static>, SendData)> {
        let path = path.as_ref();
        let pathlib = Pathlib::new(path);
        let rawdata = std::fs::read(path).ok()?;
        if !is_xlink_path(path) && !is_xlink(&rawdata) {
            return None;
        }
        print!("Is {} an XLink file? ", path.display());
        let text = match Self::new(zstd).and_then(|xlink| xlink.binary_to_yaml(&rawdata)) {
            Ok(text) => text,
            Err(error) => {
                println!("no: {error}");
                return None;
            }
        };
        println!("yes");

        let mut opened_file = OpenedFile::default();
        opened_file.path = pathlib.clone();
        opened_file.endian = Some(Endian::Little);
        opened_file.file_type = TotkFileType::Xlink;

        let mut data = SendData::default();
        data.status_text = format!("Opened {}", pathlib.full_path);
        data.path = pathlib;
        data.text = text;
        data.tab = "YAML".to_string();
        data.lang = "yaml".to_string();
        data.get_file_label(TotkFileType::Xlink, Some(Endian::Little));
        Some((opened_file, data))
    }
}
