use std::{collections::HashMap, io, sync::Arc};

use roead::sarc::{Sarc, SarcWriter};

use crate::Zstd::{is_sarc, TotkZstd};

#[derive(Clone, Copy, Debug)]
enum NestedEncoding {
    Raw,
    Yaz0,
    ZstdPack,
    Zstd,
}

pub struct NestedArchive {
    writer: SarcWriter,
    encoding: NestedEncoding,
}

impl NestedArchive {
    pub fn parse(data: &[u8], zstd: Arc<TotkZstd<'_>>) -> io::Result<Self> {
        let (raw, encoding) = if is_sarc(data) {
            (data.to_vec(), NestedEncoding::Raw)
        } else if data.starts_with(b"Yaz0") {
            (
                roead::yaz0::decompress(data)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
                NestedEncoding::Yaz0,
            )
        } else if let Ok(raw) = zstd.decompressor.decompress_pack(data) {
            (raw, NestedEncoding::ZstdPack)
        } else if let Ok(raw) = zstd.decompressor.decompress_zs(data) {
            (raw, NestedEncoding::Zstd)
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "entry is not a raw, Yaz0, or Zstd SARC",
            ));
        };
        if !is_sarc(&raw) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "decoded entry does not contain SARC data",
            ));
        }
        let sarc =
            Sarc::new(raw).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(Self {
            writer: SarcWriter::from_sarc(&sarc),
            encoding,
        })
    }

    pub fn paths(&self) -> Vec<String> {
        let mut paths: Vec<_> = self.writer.files.keys().cloned().collect();
        paths.sort_by_key(|path| path.to_lowercase());
        paths
    }

    pub fn get(&mut self, path: &str) -> Option<&[u8]> {
        self.writer.get_file(path).map(Vec::as_slice)
    }
    pub fn set(&mut self, path: &str, data: Vec<u8>) {
        self.writer.add_file(path, data);
    }

    pub fn to_encoded(&mut self, zstd: Arc<TotkZstd<'_>>) -> io::Result<Vec<u8>> {
        let raw = self.writer.to_binary();
        match self.encoding {
            NestedEncoding::Raw => Ok(raw),
            NestedEncoding::Yaz0 => Ok(roead::yaz0::compress(&raw)),
            NestedEncoding::ZstdPack => zstd.cpp_compressor.compress_pack(&raw),
            NestedEncoding::Zstd => zstd.cpp_compressor.compress_zs(&raw),
        }
    }
}

pub type NestedArchives = HashMap<String, NestedArchive>;

#[cfg(test)]
mod tests {
    use roead::{
        sarc::{Sarc, SarcWriter},
        Endian,
    };

    #[test]
    fn raw_writer_roundtrip_preserves_nested_edit() {
        let mut writer = SarcWriter::new(Endian::Little);
        writer.add_file("a/test.txt", b"before".to_vec());
        let sarc = Sarc::new(writer.to_binary()).unwrap();
        let mut nested = SarcWriter::from_sarc(&sarc);
        nested.add_file("a/test.txt", b"after".to_vec());
        let result = Sarc::new(nested.to_binary()).unwrap();
        assert_eq!(result.get_data("a/test.txt"), Some(&b"after"[..]));
    }
}
