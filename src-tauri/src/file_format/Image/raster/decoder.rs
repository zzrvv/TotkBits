use image::RgbaImage;
use std::io;

pub fn decode(data: &[u8]) -> io::Result<RgbaImage> {
    image::load_from_memory(data)
        .map(image::DynamicImage::into_rgba8)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}
