use std::ffi::c_int;
use std::io;

const MCPK_MAGIC: &[u8; 4] = b"MCPK";
const MCPK_VERSION: [u8; 4] = [1, 1, 0, 0];
const MCPK_ALIGNMENT: usize = 0x1000;
const MCPK_ZSTD_LEVEL: i32 = 20;
use meshcodec_bindings::MeshCodecBindings;

pub struct MeshCodec;

impl MeshCodec {
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
        if !crate::Settings::Magic::is_mcpk(data) {
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
        Self::decompress_loaded(data).or_else(|_| Self::decompress_pseudo(data))
    }

    /// Reproduces Switch Toolbox's "fake" MeshCodec compression: an MCPK
    /// header followed by a dictionaryless, magicless ZSTD frame.
    pub fn compress(data: &[u8]) -> io::Result<Vec<u8>> {
        // Keep BFRES external-resource flags intact. This writer preserves the
        // game's external string and GPU references, so labelling the payload
        // as a fully self-contained Toolbox resave makes readers skip the
        // required external tables and interpret those references as invalid
        // stream offsets.
        let source = data.to_vec();

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

    fn decompress_pseudo(data: &[u8]) -> io::Result<Vec<u8>> {
        let flags = data
            .get(8..12)
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .map(u32::from_le_bytes)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "truncated MCPK header"))?;
        let size = ((flags >> 5) << (flags & 0xf)) as usize;
        let mut decompressor = zstd::bulk::Decompressor::new()?;
        decompressor.set_parameter(zstd::zstd_safe::DParameter::Format(
            zstd::zstd_safe::FrameFormat::Magicless,
        ))?;
        let mut output = decompressor.decompress(&data[12..], size)?;
        if output.len() > size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "pseudo-MCPK payload exceeds its advertised size",
            ));
        }
        output.resize(size, 0);
        Ok(output)
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

    #[cfg(windows)]
    #[test]
    fn toolbox_compatible_compression_roundtrips_supplied_bfres() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/mcpk/Armor_001.Armor_001_Head_A.bfres");
        let input = std::fs::read(path).unwrap();
        let compressed = MeshCodec::compress(&input).unwrap();

        assert!(crate::Settings::Magic::is_mcpk(&compressed));
        assert_eq!(&compressed[4..8], &[1, 1, 0, 0]);
        let flags = u32::from_le_bytes(compressed[8..12].try_into().unwrap());
        let expected_size = (input.len() + 0xfff) & !0xfff;
        assert_eq!(((flags >> 5) << (flags & 0xf)) as usize, expected_size);

        let decompressed = MeshCodec::decompress(&compressed).unwrap();
        assert_eq!(&decompressed[..input.len()], input);
        assert!(decompressed[input.len()..].iter().all(|byte| *byte == 0));
    }
}
