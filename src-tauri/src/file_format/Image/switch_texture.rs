use image::{ImageBuffer, RgbaImage};
use image_dds::{ImageFormat, Surface};
use std::io;
use tegra_swizzle::{
    surface::{deswizzle_surface, swizzled_surface_size, BlockDim},
    BlockHeight,
};

pub fn decode(
    width: u32,
    height: u32,
    format: ImageFormat,
    swizzled: &[u8],
    block_height_log2: u8,
    linear: bool,
) -> io::Result<RgbaImage> {
    let (block_width, block_height, bytes_per_block) = format_layout(format)?;
    let blocks_wide = width.div_ceil(block_width);
    let blocks_high = height.div_ceil(block_height);
    let linear_size = (blocks_wide * blocks_high * bytes_per_block) as usize;
    let data = if linear {
        swizzled.get(..linear_size).unwrap_or(swizzled).to_vec()
    } else {
        let block_dim = if block_width == 4 && block_height == 4 {
            BlockDim::block_4x4()
        } else {
            BlockDim::uncompressed()
        };
        let block_height = BlockHeight::new(1usize << block_height_log2).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid Tegra block height")
        })?;
        let expected = swizzled_surface_size(
            width as usize,
            height as usize,
            1,
            block_dim,
            Some(block_height),
            bytes_per_block as usize,
            1,
            1,
        );
        let padded;
        let swizzled = if swizzled.len() < expected {
            padded = {
                let mut value = Vec::with_capacity(expected);
                value.extend_from_slice(swizzled);
                value.resize(expected, 0);
                value
            };
            padded.as_slice()
        } else {
            swizzled
        };
        deswizzle_surface(
            width as usize,
            height as usize,
            1,
            swizzled,
            block_dim,
            Some(block_height),
            bytes_per_block as usize,
            1,
            1,
        )
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?
    };
    let decoded = Surface {
        width,
        height,
        depth: 1,
        layers: 1,
        mipmaps: 1,
        image_format: format,
        data,
    }
    .decode_rgba8()
    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    ImageBuffer::from_raw(width, height, decoded.data).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "decoded texture dimensions do not match",
        )
    })
}

pub fn format_from_bntx(value: u32) -> io::Result<ImageFormat> {
    let srgb = value & 0xff == 6;
    match value >> 8 {
        0x0b => Ok(ImageFormat::R8Unorm),
        0x0d => Ok(ImageFormat::Rg8Unorm),
        0x1a => Ok(if srgb {
            ImageFormat::Rgba8UnormSrgb
        } else {
            ImageFormat::Rgba8Unorm
        }),
        0x1f => Ok(if srgb {
            ImageFormat::BC1RgbaUnormSrgb
        } else {
            ImageFormat::BC1RgbaUnorm
        }),
        0x20 => Ok(if srgb {
            ImageFormat::BC2RgbaUnormSrgb
        } else {
            ImageFormat::BC2RgbaUnorm
        }),
        0x21 => Ok(if srgb {
            ImageFormat::BC3RgbaUnormSrgb
        } else {
            ImageFormat::BC3RgbaUnorm
        }),
        0x22 => Ok(ImageFormat::BC4RUnorm),
        0x23 => Ok(ImageFormat::BC5RgUnorm),
        0x2d => Ok(if srgb {
            ImageFormat::BC7RgbaUnormSrgb
        } else {
            ImageFormat::BC7RgbaUnorm
        }),
        _ => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("unsupported BNTX format 0x{value:X}"),
        )),
    }
}

pub fn format_from_textogo(value: u16) -> io::Result<ImageFormat> {
    match value {
        0x202 | 0x302 => Ok(ImageFormat::BC1RgbaUnorm),
        0x203 => Ok(ImageFormat::BC1RgbaUnormSrgb),
        0x505 => Ok(ImageFormat::BC3RgbaUnormSrgb),
        0x602 | 0x606 | 0x607 => Ok(ImageFormat::BC4RUnorm),
        0x702 | 0x703 | 0x707 => Ok(ImageFormat::BC5RgUnorm),
        0x901 => Ok(ImageFormat::BC7RgbaUnorm),
        _ => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("unsupported TexToGo format 0x{value:X}"),
        )),
    }
}

pub fn inferred_block_height_log2(height: u32, block_height: u32) -> u8 {
    height
        .div_ceil(block_height)
        .div_ceil(8)
        .next_power_of_two()
        .min(16)
        .trailing_zeros() as u8
}

pub fn apply_component_selectors(image: &mut RgbaImage, selectors: [u8; 4]) {
    for pixel in image.pixels_mut() {
        let source = pixel.0;
        for (target, selector) in selectors.into_iter().enumerate() {
            pixel.0[target] = match selector {
                0..=3 => source[selector as usize],
                4 => 0,
                5 => 255,
                _ => source[target],
            };
        }
    }
}

fn format_layout(format: ImageFormat) -> io::Result<(u32, u32, u32)> {
    use ImageFormat::*;
    match format {
        BC1RgbaUnorm | BC1RgbaUnormSrgb | BC4RUnorm | BC4RSnorm => Ok((4, 4, 8)),
        BC2RgbaUnorm | BC2RgbaUnormSrgb | BC3RgbaUnorm | BC3RgbaUnormSrgb | BC5RgUnorm
        | BC5RgSnorm | BC6hRgbUfloat | BC6hRgbSfloat | BC7RgbaUnorm | BC7RgbaUnormSrgb => {
            Ok((4, 4, 16))
        }
        R8Unorm | R8Snorm => Ok((1, 1, 1)),
        Rg8Unorm | Rg8Snorm => Ok((1, 1, 2)),
        Rgba8Unorm | Rgba8UnormSrgb | Rgba8Snorm | Bgra8Unorm | Bgra8UnormSrgb => Ok((1, 1, 4)),
        _ => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!("unsupported Switch image format {format:?}"),
        )),
    }
}
