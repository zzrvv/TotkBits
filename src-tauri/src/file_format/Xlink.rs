use std::{
    ffi::CStr,
    io,
    os::raw::c_char,
    path::{Path, PathBuf},
    sync::Arc,
};

use libloading::{Library, Symbol};
use roead::Endian;

use crate::{
    file_format::BinTextFile::OpenedFile,
    utils::exe_relative_path,
    Open_and_Save::SendData,
    Settings::Pathlib,
    TotkApp::InternalFile,
    Zstd::{is_xlink, is_xlink_path, TotkFileType, TotkZstd, ZstdDictionary},
};

/// A lazily-created handle to the native XLink converter.
pub struct Xlink_rs<'a> {
    pub zstd: Arc<TotkZstd<'a>>,
}

type BinaryToYaml = unsafe extern "C" fn(*const c_char, usize) -> *const c_char;
type YamlToBinary = unsafe extern "C" fn(*const c_char, usize, *mut usize) -> *mut c_char;
type FreeBinary = unsafe extern "C" fn(*mut c_char);
type FreeString = unsafe extern "C" fn(*mut c_char);

fn xlink_dll_path() -> io::Result<PathBuf> {
    let installed = exe_relative_path("bin/dlls/xlink_tool.dll");
    if installed.is_file() {
        return Ok(installed);
    }
    let development = Path::new(env!("CARGO_MANIFEST_DIR")).join("bin/dlls/xlink_tool.dll");
    if development.is_file() {
        return Ok(development);
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "XLink converter DLL was not found at {}",
            installed.display()
        ),
    ))
}

unsafe fn load_symbol<'dll, T>(library: &'dll Library, name: &[u8]) -> io::Result<Symbol<'dll, T>> {
    library
        .get(name)
        .map_err(|error| io::Error::other(error.to_string()))
}

#[cfg(windows)]
impl<'a> Xlink_rs<'a> {
    pub fn new(zstd: Arc<TotkZstd<'a>>) -> io::Result<Self> {
        #[cfg(windows)]
        {
            Ok(Self { zstd })
        }
        #[cfg(not(windows))]
        {
            let _ = zstd;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Xlink bindings are only supported on Windows",
            ))
        }
    }

    pub fn binary_to_yaml(&self, data: &[u8]) -> io::Result<String> {
        let rawdata = if is_xlink(data) {
            data.to_vec()
        } else {
            self.zstd
                .decompressor
                .decompress_zs(&data.to_vec())
                .map_err(|err| io::Error::other(err.to_string()))?
        };
        if !is_xlink(&rawdata) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a valid XLink binary (missing XLNK magic)",
            ));
        }

        let dll_path = xlink_dll_path()?;
        unsafe {
            let library = Library::new(&dll_path).map_err(|error| {
                io::Error::other(format!("failed to load {}: {error}", dll_path.display()))
            })?;
            let convert: Symbol<BinaryToYaml> = load_symbol(&library, b"xlink_binary_to_yaml\0")?;
            let free: Symbol<FreeString> = load_symbol(&library, b"free_xlink_string\0")?;
            let yaml_ptr = convert(rawdata.as_ptr().cast(), rawdata.len());
            if yaml_ptr.is_null() {
                return Err(io::Error::other(
                    "XLink converter failed to convert binary to YAML",
                ));
            }
            let yaml_bytes = CStr::from_ptr(yaml_ptr).to_bytes().to_vec();
            free(yaml_ptr.cast_mut());
            String::from_utf8(yaml_bytes).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("XLink converter returned invalid UTF-8 text: {error}"),
                )
            })
        }
    }

    pub fn yaml_to_binary(&self, data: &str) -> io::Result<Vec<u8>> {
        let dll_path = xlink_dll_path()?;
        unsafe {
            let library = Library::new(&dll_path).map_err(|error| {
                io::Error::other(format!("failed to load {}: {error}", dll_path.display()))
            })?;
            let convert: Symbol<YamlToBinary> = load_symbol(&library, b"xlink_yaml_to_binary\0")?;
            let free: Symbol<FreeBinary> = load_symbol(&library, b"free_xlink_binary\0")?;
            let mut out_size = 0usize;
            let binary_ptr = convert(data.as_ptr().cast(), data.len(), &mut out_size);
            if binary_ptr.is_null() {
                return Err(io::Error::other(
                    "XLink converter failed to convert YAML to binary",
                ));
            }
            let binary = std::slice::from_raw_parts(binary_ptr as *const u8, out_size).to_vec();
            free(binary_ptr);
            if !is_xlink(&binary) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "XLink converter returned binary data without XLNK magic",
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
        if !is_xlink(&rawdata) {
            // if !is_xlink_path(path) && !is_xlink(&rawdata) {
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

#[cfg(test)]
mod tests {
    use super::Xlink_rs;
    use crate::TotkConfig::TotkConfig;
    use crate::Zstd::TotkZstd;
    use std::fs;
    use std::path::Path;
    use std::sync::Arc;

    #[test]
    #[cfg(windows)]
    fn compressed_elink_fixture_produces_parseable_yaml_and_round_trips() {
        let romfs = Path::new("E:/TOTK_modding/0100F2C0115B6000/romfs");
        if !romfs.join("Pack/ZsDic.pack.zs").is_file() {
            return;
        }
        let mut config = TotkConfig::default();
        config.romfs = romfs.to_string_lossy().into_owned();
        let zstd = Arc::new(TotkZstd::new(Arc::new(config), 16).expect("load ZSTD dictionaries"));
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/elink2.Product.110.belnk.zs");
        let input = fs::read(path).expect("missing compressed ELink fixture");
        let converter = Xlink_rs::new(zstd).expect("construct XLink converter");
        let yaml = converter
            .binary_to_yaml(&input)
            .expect("convert ELink to YAML");
        serde_yaml::from_str::<serde_yaml::Value>(&yaml).expect("XLink output is not valid YAML");
        let rebuilt = converter
            .yaml_to_binary(&yaml)
            .expect("rebuild ELink from YAML");
        assert!(crate::Zstd::is_xlink(&rebuilt));
    }

    #[test]
    #[cfg(windows)]
    fn belnk_file_reaches_xlink_bindings() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/elink2.Product.110.belnk.zs");
        let data = fs::read(&path).expect("missing test vector");
        let zstd = Arc::new(TotkZstd::dictionaryless(
            Arc::new(TotkConfig::default()),
            16,
        ));
        let xlink = Xlink_rs::new(zstd).unwrap_or_else(|error| {
            panic!("Failed to construct Xlink bindings: {error}");
        });

        let err = xlink
            .binary_to_yaml(&data)
            .expect_err("XLink parse should fail without a valid dictionary");
        assert!(
            err.to_string().contains("Requested ZSTD dictionary")
                || err.to_string().contains("Unable to convert binary to YAML")
        );
    }
}

#[cfg(not(windows))]
impl<'a> Xlink_rs<'a> {
    pub fn new(zstd: Arc<TotkZstd<'a>>) -> io::Result<Self> {
        let _ = zstd;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Xlink bindings are only supported on Windows",
        ))
    }

    pub fn binary_to_yaml(&self, _data: &[u8]) -> io::Result<String> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Xlink bindings are only supported on Windows",
        ))
    }

    pub fn yaml_to_binary(&self, _data: &str) -> io::Result<Vec<u8>> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Xlink bindings are only supported on Windows",
        ))
    }

    pub fn open_internal<P: AsRef<Path>>(
        _path: P,
        _data: &[u8],
        _zstd: Arc<TotkZstd<'a>>,
    ) -> Option<(InternalFile<'static>, String)> {
        None
    }

    pub fn text_to_binary(
        _text: &str,
        _file_path: &str,
        _zstd: Arc<TotkZstd<'a>>,
        _dictionary: Option<ZstdDictionary>,
    ) -> Option<Vec<u8>> {
        None
    }

    pub fn open_xlink<P: AsRef<Path>>(
        _path: P,
        _zstd: Arc<TotkZstd<'a>>,
    ) -> Option<(OpenedFile<'static>, SendData)> {
        None
    }
}
