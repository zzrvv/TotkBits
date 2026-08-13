use image::RgbaImage;
use std::{io, io::Cursor};

pub fn decode(data: &[u8]) -> io::Result<RgbaImage> {
    let header = super::DdsHeader::parse(data)?;
    let dds = image_dds::ddsfile::Dds::read(&mut Cursor::new(data))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let image = image_dds::image_from_dds(&dds, 0)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    if image.width() != header.width || image.height() < header.height {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "decoded DDS dimensions do not match header",
        ));
    }
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn decodes_switch_toolbox_bc_textures() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/Switch-Toolbox/File_Format_Library/Resources");
        for name in ["White.dds", "Basic_NrmBC5.dds"] {
            let path = root.join(name);
            let data =
                std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let image = decode(&data).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert!(
                image.width() > 0 && image.height() > 0,
                "{}",
                path.display()
            );
        }
    }
}
