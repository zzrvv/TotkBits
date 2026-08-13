use image::{DynamicImage, ImageFormat, RgbaImage};
use std::{io, io::Cursor};

pub fn encode(image: &RgbaImage) -> io::Result<Vec<u8>> {
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(output.into_inner())
}

#[cfg(test)]
mod tests {
    #[test]
    fn emits_png_signature() {
        let image = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]));
        let png = super::encode(&image).unwrap();
        assert!(crate::Settings::Magic::is_png(&png));
    }
}
