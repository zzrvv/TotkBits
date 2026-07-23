use std::ffi::c_int;
use std::io;

const MCPK_MAGIC: &[u8; 4] = b"MCPK";
const MCPK_VERSION: [u8; 4] = [1, 1, 0, 0];
const MCPK_ALIGNMENT: usize = 0x1000;
const MCPK_ZSTD_LEVEL: i32 = 20;
const BFRES_EXTERNAL_FLAGS_OFFSET: usize = 0xee;
const BFRES_MCPK_RESAVE_FLAG_OFFSET: usize = 0xef;
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

    /// Reproduces Switch Toolbox's "fake" MeshCodec compression: an MCPK
    /// header followed by a dictionaryless, magicless ZSTD frame.
    pub fn compress(data: &[u8]) -> io::Result<Vec<u8>> {
        let mut source = data.to_vec();
        if source.starts_with(b"FRES") && source.len() > BFRES_MCPK_RESAVE_FLAG_OFFSET {
            source[BFRES_EXTERNAL_FLAGS_OFFSET] = 0;
            source[BFRES_MCPK_RESAVE_FLAG_OFFSET] = 1;
        }

        let aligned_size = source
            .len()
            .checked_add(MCPK_ALIGNMENT - 1)
            .map(|size| size & !(MCPK_ALIGNMENT - 1))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "MCPK input is too large")
            })?;
        let aligned_size = u32::try_from(aligned_size).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "MCPK decompressed size exceeds the 32-bit header",
            )
        })?;
        let flags = ((aligned_size >> 12) << 5) + 12;

        let mut compressor = zstd::bulk::Compressor::new(MCPK_ZSTD_LEVEL)?;
        compressor.set_parameter(zstd::zstd_safe::CParameter::ContentSizeFlag(false))?;
        compressor.set_parameter(zstd::zstd_safe::CParameter::ChecksumFlag(false))?;
        compressor.set_parameter(zstd::zstd_safe::CParameter::DictIdFlag(false))?;
        compressor.set_parameter(zstd::zstd_safe::CParameter::Format(
            zstd::zstd_safe::FrameFormat::Magicless,
        ))?;
        let compressed = compressor.compress(&source)?;

        let mut output = Vec::with_capacity(12 + compressed.len());
        output.extend_from_slice(MCPK_MAGIC);
        output.extend_from_slice(&MCPK_VERSION);
        output.extend_from_slice(&flags.to_le_bytes());
        output.extend_from_slice(&compressed);
        Ok(output)
    }

    fn decompress_loaded(data: &[u8]) -> io::Result<Vec<u8>> {
        MeshCodecBindings::decompress(data)
    }
}

#[cfg(test)]
mod tests {
    use super::{MeshCodec, BFRES_EXTERNAL_FLAGS_OFFSET, BFRES_MCPK_RESAVE_FLAG_OFFSET};

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

    #[cfg(windows)]
    #[test]
    fn toolbox_compatible_compression_roundtrips_supplied_bfres() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/mcpk/Armor_001.Armor_001_Head_A.bfres");
        let input = std::fs::read(path).unwrap();
        let compressed = MeshCodec::compress(&input).unwrap();

        assert!(MeshCodec::has_magic(&compressed));
        assert_eq!(&compressed[4..8], &[1, 1, 0, 0]);
        let flags = u32::from_le_bytes(compressed[8..12].try_into().unwrap());
        let expected_size = (input.len() + 0xfff) & !0xfff;
        assert_eq!(((flags >> 5) << (flags & 0xf)) as usize, expected_size);

        let decompressed = MeshCodec::decompress(&compressed).unwrap();
        let mut expected = input;
        expected[BFRES_EXTERNAL_FLAGS_OFFSET] = 0;
        expected[BFRES_MCPK_RESAVE_FLAG_OFFSET] = 1;
        assert_eq!(&decompressed[..expected.len()], expected);
        assert!(decompressed[expected.len()..].iter().all(|byte| *byte == 0));
    }
}
