use std::{ffi::CStr, io, path::Path, sync::Arc};

use roead::Endian;
use xlink2_bindings as xlink_bindings;

use crate::{
    file_format::BinTextFile::OpenedFile,
    Open_and_Save::SendData,
    Settings::Pathlib,
    TotkApp::InternalFile,
    Zstd::{is_xlink, is_xlink_path, TotkFileType, TotkZstd, ZstdDictionary},
};

/// A lazily-created handle to the native XLink converter.
pub struct Xlink_rs<'a> {
    pub zstd: Arc<TotkZstd<'a>>,
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
                .try_decompress(data)
                .map_err(|err| io::Error::other(err.to_string()))?
        };
        if !is_xlink(&rawdata) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "not a valid XLink binary (missing XLNK magic)",
            ));
        }

        unsafe {
            let yaml_ptr = xlink_bindings::binary_to_yaml(&rawdata).cast::<i8>();
            if yaml_ptr.is_null() {
                return Err(io::Error::other(
                    "XLink converter failed to convert binary to YAML",
                ));
            }
            let yaml_bytes = CStr::from_ptr(yaml_ptr).to_bytes().to_vec();
            xlink_bindings::free_string(yaml_ptr.cast_mut());
            String::from_utf8(yaml_bytes).map_err(|error| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("XLink converter returned invalid UTF-8 text: {error}"),
                )
            })
        }
    }

    pub fn yaml_to_binary(&self, data: &str) -> io::Result<Vec<u8>> {
        unsafe {
            let (binary_ptr, out_size) = xlink_bindings::yaml_to_binary(data.as_bytes());
            if binary_ptr.is_null() {
                return Err(io::Error::other(
                    "XLink converter failed to convert YAML to binary",
                ));
            }
            let binary = std::slice::from_raw_parts(binary_ptr as *const u8, out_size).to_vec();
            xlink_bindings::free_binary(binary_ptr);
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
            zstd.compress_zs(&data).ok()
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
        data.lang = "xlink".to_string();
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
    fn elink_binary_fixture_converts_to_yaml_and_round_trips() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/_ss/elink2.Product.110.belnk");
        let input = fs::read(path).expect("missing ELink binary fixture");
        let zstd = Arc::new(TotkZstd::dictionaryless(
            Arc::new(TotkConfig::default()),
            16,
        ));
        let converter = Xlink_rs::new(zstd).expect("construct XLink converter");
        let yaml = converter
            .binary_to_yaml(&input)
            .expect("convert ELink to YAML");
        assert!(!yaml.is_empty(), "XLink converter returned empty YAML");
        let rebuilt = converter
            .yaml_to_binary(&yaml)
            .expect("rebuild ELink from YAML");
        assert!(crate::Zstd::is_xlink(&rebuilt));
    }

    #[test]
    #[cfg(windows)]
    fn elink_yaml_fixture_converts_to_binary_and_round_trips() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/_ss/elink2.Product.110.belnk.yaml");
        let yaml = fs::read_to_string(path).expect("missing ELink YAML fixture");
        let zstd = Arc::new(TotkZstd::dictionaryless(
            Arc::new(TotkConfig::default()),
            16,
        ));
        let converter = Xlink_rs::new(zstd).expect("construct XLink converter");
        let binary = converter
            .yaml_to_binary(&yaml)
            .expect("convert YAML fixture to ELink binary");
        assert!(crate::Zstd::is_xlink(&binary));
        let rebuilt_yaml = converter
            .binary_to_yaml(&binary)
            .expect("convert rebuilt ELink binary to YAML");
        assert!(
            !rebuilt_yaml.is_empty(),
            "XLink converter returned empty rebuilt YAML"
        );
    }

    #[test]
    #[cfg(windows)]
    fn compressed_elink_fixture_decompresses_and_converts() {
        let romfs = Path::new("E:/TOTK_modding/0100F2C0115B6000/romfs");
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/elink2.Product.110.belnk.zs");
        let mut config = TotkConfig::default();
        config.romfs = romfs.to_string_lossy().into_owned();
        let zstd = Arc::new(TotkZstd::new(Arc::new(config), 16).expect("load ZSTD dictionaries"));
        let input = fs::read(fixture).expect("missing compressed ELink fixture");
        let converter = Xlink_rs::new(zstd).expect("construct XLink converter");
        let yaml = converter
            .binary_to_yaml(&input)
            .expect("decompress and convert compressed ELink fixture");
        assert!(!yaml.is_empty(), "compressed XLink returned empty text");
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
