use base64::Engine;
use serde::Serialize;
use std::{io, path::Path};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedImage {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub mip_count: u32,
    pub dds_type: Option<String>,
    pub entries: Vec<ImageEntry>,
    pub selected_index: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageEntry {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub mip_count: u32,
    pub array_count: u32,
    pub format: String,
}

pub struct ImageDocument;

impl ImageDocument {
    pub fn open(
        path: &Path,
    ) -> Option<(
        crate::file_format::BinTextFile::OpenedFile<'static>,
        crate::Open_and_Save::SendData,
    )> {
        let data = std::fs::read(path).ok()?;
        if !Self::supports(path, &data) {
            return None;
        }
        let mut opened = crate::file_format::BinTextFile::OpenedFile::default();
        opened.path = crate::Settings::Pathlib::new(path);
        opened.file_type = crate::Zstd::TotkFileType::Other;

        let mut send = crate::Open_and_Save::SendData::default();
        send.path = crate::Settings::Pathlib::new(path);
        send.file_label = format!("{} [IMAGE]", send.path.name);
        send.file_metadata = "[IMAGE] [READ ONLY]".into();
        send.status_text = format!("Opened image {}", path.display());
        send.tab = "IMAGE".into();
        send.read_only = true;
        Some((opened, send))
    }

    pub fn render_path(path: impl AsRef<Path>) -> io::Result<RenderedImage> {
        Self::render_path_selection(path, 0, 0, 0)
    }

    pub fn render_path_selection(
        path: impl AsRef<Path>,
        texture_index: usize,
        _array_index: u32,
        _mip_index: u32,
    ) -> io::Result<RenderedImage> {
        let path = path.as_ref();
        let source = std::fs::read(path)?;
        let data = maybe_zstd(&source)?;
        let mut entries = Vec::new();
        let (rgba, format, mip_count, dds_type) = if data.starts_with(b"BNTX") {
            let bntx = crate::parser::bntx::BntxFile::parse(&data).map_err(invalid)?;
            entries = bntx
                .textures
                .iter()
                .map(|texture| ImageEntry {
                    name: texture.name.clone(),
                    width: texture.width,
                    height: texture.height,
                    mip_count: u32::from(texture.mip_count),
                    array_count: texture.array_length,
                    format: format!("0x{:X}", texture.format),
                })
                .collect();
            let texture = bntx.textures.get(texture_index).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "BNTX texture index is out of range",
                )
            })?;
            let offset = *texture.data_offsets.first().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "BNTX texture has no image data")
            })? as usize;
            let end = offset
                .saturating_add(texture.image_size as usize)
                .min(data.len());
            let image_format = super::switch_texture::format_from_bntx(texture.format)?;
            let mut image = super::switch_texture::decode(
                texture.width,
                texture.height,
                image_format,
                data.get(offset..end).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "BNTX image offset is outside the file",
                    )
                })?,
                texture.block_height_log2,
                texture.tile_mode == 1,
            )?;
            super::switch_texture::apply_component_selectors(&mut image, texture.channel_types);
            (
                image,
                "BNTX".into(),
                u32::from(texture.mip_count),
                Some(format!("{image_format:?}")),
            )
        } else if data.get(4..8) == Some(b"6PK0") {
            let txtg = crate::parser::textogo::TexToGoFile::parse(&data).map_err(invalid)?;
            let image_format = super::switch_texture::format_from_textogo(txtg.header.format)?;
            entries.push(ImageEntry {
                name: path
                    .file_stem()
                    .and_then(|x| x.to_str())
                    .unwrap_or("TexToGo")
                    .into(),
                width: u32::from(txtg.header.width),
                height: u32::from(txtg.header.height),
                mip_count: u32::from(txtg.header.mip_count),
                array_count: u32::from(txtg.header.depth),
                format: format!("0x{:X}", txtg.header.format),
            });
            let surface = txtg
                .surfaces
                .iter()
                .find(|surface| surface.array_level == 0 && surface.mip_level == 0)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "TexToGo has no base surface")
                })?;
            let log2 =
                super::switch_texture::inferred_block_height_log2(u32::from(txtg.header.height), 4);
            (
                super::switch_texture::decode(
                    u32::from(txtg.header.width),
                    u32::from(txtg.header.height),
                    image_format,
                    &surface.data,
                    log2,
                    false,
                )?,
                "TexToGo".into(),
                u32::from(txtg.header.mip_count),
                Some(format!("{image_format:?}")),
            )
        } else if data.starts_with(b"DDS ") {
            let header = super::dds::DdsHeader::parse(&data)?;
            let dds = image_dds::ddsfile::Dds::read(&mut std::io::Cursor::new(&data))
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
            let dds_type = image_dds::dds_image_format(&dds)
                .map(|value| format!("{value:?}"))
                .unwrap_or_else(|_| "Unknown".into());
            (
                super::dds::decode(&data)?,
                "DDS".to_string(),
                header.mipmap_count,
                Some(dds_type),
            )
        } else {
            let image = super::raster::decode(&data)?;
            let format = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("image")
                .to_ascii_uppercase();
            (image, format, 1, None)
        };
        let width = rgba.width();
        let height = rgba.height();
        let png = super::png::encode(&rgba)?;
        Ok(RenderedImage {
            data_url: format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(png)
            ),
            width,
            height,
            format,
            mip_count,
            dds_type,
            entries,
            selected_index: texture_index,
        })
    }

    pub fn replace_dds(
        target: impl AsRef<Path>,
        png: impl AsRef<Path>,
        dds_type: &str,
        mip_count: u32,
    ) -> io::Result<()> {
        super::dds::replace_from_png(target, png, dds_type, mip_count)
    }

    pub fn export_png(source: impl AsRef<Path>, output: impl AsRef<Path>) -> io::Result<()> {
        let rendered = Self::render_path(source)?;
        let encoded = rendered
            .data_url
            .split_once(',')
            .map(|(_, value)| value)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid rendered image URL")
            })?;
        let png = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        std::fs::write(output, png)
    }

    fn supports(path: &Path, data: &[u8]) -> bool {
        if data.starts_with(b"DDS ")
            || data.starts_with(b"BNTX")
            || data.get(4..8) == Some(b"6PK0")
            || data.starts_with(b"\x89PNG\r\n\x1a\n")
        {
            return true;
        }
        if data.starts_with(b"\x28\xB5\x2F\xFD") {
            if let Ok(decoded) = maybe_zstd(data) {
                return decoded.starts_with(b"BNTX");
            }
        }
        matches!(
            path.extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref(),
            Some(
                "bmp"
                    | "gif"
                    | "ico"
                    | "jpg"
                    | "jpeg"
                    | "png"
                    | "pnm"
                    | "qoi"
                    | "tga"
                    | "tif"
                    | "tiff"
                    | "webp"
                    | "dds"
                    | "bntx"
                    | "txtg"
                    | "zs"
            )
        )
    }
}

fn maybe_zstd(data: &[u8]) -> io::Result<Vec<u8>> {
    if data.starts_with(b"\x28\xB5\x2F\xFD") {
        zstd::stream::decode_all(std::io::Cursor::new(data))
    } else {
        Ok(data.to_vec())
    }
}
fn invalid(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn renders_bntx_sample() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/tex/Animal_Insect_A.bntx");
        let rendered = ImageDocument::render_path(path).unwrap();
        assert_eq!(
            (rendered.width, rendered.height, rendered.format.as_str()),
            (256, 256, "BNTX")
        );
    }
    #[test]
    fn renders_textogo_sample() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/tex/Armor_1006_Lower_Alb.7.txtg");
        let rendered = ImageDocument::render_path(path).unwrap();
        assert_eq!(
            (rendered.width, rendered.height, rendered.format.as_str()),
            (640, 640, "TexToGo")
        );
    }
}
