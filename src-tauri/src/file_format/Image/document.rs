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
        let path = path.as_ref();
        let data = std::fs::read(path)?;
        let (rgba, format, mip_count, dds_type) = if data.starts_with(b"DDS ") {
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
        if data.starts_with(b"DDS ") || data.starts_with(b"\x89PNG\r\n\x1a\n") {
            return true;
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
            )
        )
    }
}
