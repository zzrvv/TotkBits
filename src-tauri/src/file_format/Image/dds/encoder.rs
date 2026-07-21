use image_dds::{ImageFormat, Mipmaps, Quality};
use std::{fs, io, path::Path};

fn image_format(name: &str) -> io::Result<ImageFormat> {
    match name {
        "BC1" => Ok(ImageFormat::BC1RgbaUnorm),
        "BC3" => Ok(ImageFormat::BC3RgbaUnorm),
        "BC5" => Ok(ImageFormat::BC5RgUnorm),
        "BC7" => Ok(ImageFormat::BC7RgbaUnorm),
        "RGBA8" => Ok(ImageFormat::Rgba8Unorm),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unsupported DDS type {name}"),
        )),
    }
}

pub fn replace_from_png(
    target: impl AsRef<Path>,
    png: impl AsRef<Path>,
    format: &str,
    mip_count: u32,
) -> io::Result<()> {
    let source = image::open(png)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?
        .to_rgba8();
    let mipmaps = if mip_count <= 1 {
        Mipmaps::Disabled
    } else {
        Mipmaps::GeneratedExact(mip_count)
    };
    let dds = image_dds::dds_from_image(&source, image_format(format)?, Quality::Normal, mipmaps)
        .map_err(|error| io::Error::other(error.to_string()))?;
    let mut encoded = Vec::new();
    dds.write(&mut encoded)
        .map_err(|error| io::Error::other(error.to_string()))?;
    fs::write(target, encoded)
}
