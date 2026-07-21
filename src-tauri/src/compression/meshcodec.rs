use std::ffi::c_int;
use std::io;

const MCPK_MAGIC: &[u8; 4] = b"MCPK";
use meshcodec_bindings::MeshCodecBindings;

pub struct MeshCodec;

impl MeshCodec {
    pub fn has_magic(data: &[u8]) -> bool {
        data.starts_with(MCPK_MAGIC)
    }

    pub fn new() -> io::Result<Self> {
        Self::ensure_platform()
    }

    #[cfg(windows)]
    fn ensure_platform() -> io::Result<Self> {
        Ok(Self)
    }

    #[cfg(not(windows))]
    fn ensure_platform() -> io::Result<Self> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "MeshCodec bindings are only supported on Windows",
        ))
    }

    pub fn decompress(data: &[u8]) -> io::Result<Vec<u8>> {
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

        Self::ensure_platform()?;
        Self::decompress_loaded(data)
    }

    fn decompress_loaded(data: &[u8]) -> io::Result<Vec<u8>> {
        MeshCodecBindings::decompress(data)
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
        assert!(count > 0, "MCPK corpus is empty");
    }
}
