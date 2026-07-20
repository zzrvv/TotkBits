use libloading::{Library, Symbol};
use std::{
    ffi::{c_char, c_int},
    io,
    path::PathBuf,
    slice,
};

const MCPK_MAGIC: &[u8; 4] = b"MCPK";

type DecompressFn = unsafe extern "C" fn(*const c_char, c_int, *mut c_int) -> *mut c_char;
type FreeFn = unsafe extern "C" fn(*mut c_char);

pub struct MeshCodec {
    library: Library,
    library_path: PathBuf,
}

impl MeshCodec {
    pub fn has_magic(data: &[u8]) -> bool {
        data.starts_with(MCPK_MAGIC)
    }

    pub fn new() -> io::Result<Self> {
        let candidates = Self::library_candidates();
        let library_path = candidates
            .iter()
            .find(|path| path.is_file())
            .cloned()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!(
                        "MeshCodec DLL was not found; checked {}",
                        candidates
                            .iter()
                            .map(|path| path.display().to_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                )
            })?;
        let library = unsafe { Library::new(&library_path) }.map_err(|error| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "failed to load MeshCodec DLL {}: {error}",
                    library_path.display()
                ),
            )
        })?;
        Ok(Self {
            library,
            library_path,
        })
    }

    pub fn decompress(data: &[u8]) -> io::Result<Vec<u8>> {
        // This check deliberately precedes both path discovery and Library::new.
        if !Self::has_magic(data) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "input does not have MCPK magic",
            ));
        }
        if data.len() > c_int::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "MCPK input is too large for the MeshCodec ABI",
            ));
        }

        let codec = Self::new()?;
        unsafe { codec.decompress_loaded(data) }
    }

    unsafe fn decompress_loaded(&self, data: &[u8]) -> io::Result<Vec<u8>> {
        let decompress: Symbol<DecompressFn> = self
            .library
            .get(b"meshcodec_decompress\0")
            .map_err(|error| self.symbol_error("meshcodec_decompress", error))?;
        let free: Symbol<FreeFn> = self
            .library
            .get(b"meshcodec_free\0")
            .map_err(|error| self.symbol_error("meshcodec_free", error))?;

        let mut output_len: c_int = 0;
        let output = decompress(
            data.as_ptr().cast::<c_char>(),
            data.len() as c_int,
            &mut output_len,
        );
        if output.is_null() || output_len <= 0 {
            if !output.is_null() {
                free(output);
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MeshCodec failed to decompress MCPK input",
            ));
        }

        let result = slice::from_raw_parts(output.cast::<u8>(), output_len as usize).to_vec();
        free(output);
        Ok(result)
    }

    fn symbol_error(&self, symbol: &str, error: libloading::Error) -> io::Error {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "MeshCodec DLL {} is missing {symbol}: {error}",
                self.library_path.display()
            ),
        )
    }

    fn library_candidates() -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        if let Ok(executable) = std::env::current_exe() {
            if let Some(directory) = executable.parent() {
                candidates.push(directory.join("meshcodec.dll"));
                candidates.push(directory.join("bin").join("dlls").join("meshcodec.dll"));
            }
        }
        if let Ok(current) = std::env::current_dir() {
            candidates.push(current.join("meshcodec.dll"));
            candidates.push(
                current
                    .join("src-tauri")
                    .join("bin")
                    .join("dlls")
                    .join("meshcodec.dll"),
            );
        }
        candidates.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("bin")
                .join("dlls")
                .join("meshcodec.dll"),
        );
        candidates
    }
}

#[cfg(test)]
mod tests {
    use super::MeshCodec;

    #[test]
    fn magic_gate_rejects_non_mcpk_before_loading() {
        let error = MeshCodec::decompress(b"Yaz0not-mesh-codec").unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("MCPK magic"));
    }

    #[cfg(windows)]
    #[test]
    fn decompresses_local_mcpk_corpus() {
        let corpus = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/mcpk");
        let mut count = 0;
        for entry in std::fs::read_dir(corpus).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|value| value.to_str()) != Some("mc") {
                continue;
            }
            let input = std::fs::read(&path).unwrap();
            let flags = u32::from_le_bytes(input[8..12].try_into().unwrap());
            let expected_len = (flags >> 5) << (flags & 0xf);
            let output = MeshCodec::decompress(&input)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(output.len(), expected_len as usize, "{}", path.display());
            count += 1;
        }
        assert_eq!(count, 4);
    }
}
