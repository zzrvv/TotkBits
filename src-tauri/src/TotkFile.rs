
pub enum FileGenesis {
    Disk,
    Archive,
    None
}

pub enum TotkEndian {
    LE,
    BE,
    None
}

pub struct TotkFile<'a> {
    pub zstd: Arc<TotkZstd<'a>>,
    pub file_type: TotkFileType,
    pub endian: TotkEndian,
    pub compression: ZstdDictionary,
    pub genesis: FileGenesis,
    pub path: Pathlib,
    pub binary_raw: Vec<u8>,
    pub text: String
}


impl<'a> TotkFile<'_> {
    pub fn default(zstd: Arc<TotkZstd<'a>>) -> Self {
        Self {
            zstd.clone(),
           TotkFileType::None,
           TotkEndian::None,
           ZstdDictionary::None,
            FileGenesis::None,
            Pathlib::default(),
            Default::default(),
            Default::default()
        }

    }

    pub fn from_binary(data: &[u8], zstd: Arc<TotkZstd<'a>>, path: impl AsRef<Path>) -> io::Result<Self> {
        let mut res = Self::default(zstd.clone());
        let path = path.as_ref();
        let path_str = path.as_ref();
        (res.binary_raw, res.compression) = zstd.try_decompress_all_ordered_safe(&data, &path);


        Ok(res)
    }


}