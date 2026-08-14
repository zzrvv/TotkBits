use aes::cipher::{BlockEncrypt, KeyInit};
use base64::Engine;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::file_format::BinTextFile::OpenedFile;
use crate::Open_and_Save::SendData;
use crate::Settings::Pathlib;
use crate::Zstd::TotkFileType;
use tauri::Manager;

const RENDERER: &str = "https://mii-unsecure.ariankordi.net/miis";
const STUDIO_RENDERER: &str = "https://studio.mii.nintendo.com/miis/image.png";
const QR_KEY: [u8; 16] = [
    0x59, 0xfc, 0x81, 0x7e, 0x64, 0x46, 0xea, 0x61, 0x90, 0x34, 0x7b, 0x20, 0xe9, 0xbd, 0xce, 0x52,
];

fn show_connection_error(message: &str) {
    rfd::MessageDialog::new()
        .set_title("TotkBits - Mii renderer unavailable")
        .set_description(format!(
            "Could not connect to either Mii image renderer.\n\n{message}"
        ))
        .set_level(rfd::MessageLevel::Error)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

fn renderer_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(90))
        .user_agent("TotkBits/1.0 Mii renderer")
        .build()
        .map_err(|error| error.to_string())
}

fn supported_binary_extension(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "charinfo"
            | "ufsd"
            | "miigx"
            | "mii"
            | "mae"
            | "rcd"
            | "rsd"
            | "cfsd"
            | "ffsd"
            | "3dsmii"
            | "cfcd"
            | "nfcd"
            | "nfsd"
            | "mnms"
    )
}

fn possible_qr_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "png" | "jpg" | "jpeg" | "bmp" | "gif" | "webp" | "tif" | "tiff"
    )
}

fn decode_qr_payload(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let image = image::load_from_memory(bytes)
        .map_err(|error| format!("unable to decode QR image: {error}"))?
        .to_luma8();
    let mut prepared = rqrr::PreparedImage::prepare(image);
    let grids = prepared.detect_grids();
    let mut last_error = None;
    for grid in grids {
        let mut raw = Vec::new();
        let result = grid.decode_to(&mut raw);
        if !raw.is_empty() {
            // Some older Mii QR generators encoded arbitrary bytes as Latin-1
            // text and then UTF-8. Collapse that expansion when it yields a
            // complete wrapped Mii; otherwise retain the original byte mode.
            if let Ok(content) = std::str::from_utf8(&raw) {
                let normalized: Option<Vec<u8>> = content
                    .chars()
                    .map(|character| u8::try_from(character as u32).ok())
                    .collect();
                if let Some(normalized) = normalized.filter(|value| value.len() >= 112) {
                    return Ok(normalized);
                }
            }
            return Ok(raw);
        }
        match result {
            Ok(_) => last_error = Some("QR code has an empty payload".to_string()),
            Err(error) => last_error = Some(error.to_string()),
        }
    }
    Err(last_error.unwrap_or_else(|| "image does not contain a QR code".to_string()))
}

/// Decrypt CFLiWrappedMiiData from a 3DS/Wii U Mii QR code.
fn unwrap_qr_store_data(wrapped: &[u8]) -> Result<Vec<u8>, String> {
    if wrapped.len() < 112 {
        return Err(format!(
            "unsupported Mii QR payload size: {} bytes (expected at least 112)",
            wrapped.len()
        ));
    }
    let cipher = aes::Aes128::new_from_slice(&QR_KEY).map_err(|error| error.to_string())?;
    let mut plaintext = wrapped[8..96].to_vec();
    let mut nonce = [0_u8; 12];
    nonce[..8].copy_from_slice(&wrapped[..8]);

    // CCM with a 12-byte nonce uses a three-byte, big-endian counter. Nintendo's
    // QR implementation has a known authentication-tag erratum, so decrypt the
    // CTR stream and rely on the renderer's StoreData CRC validation.
    for (index, chunk) in plaintext.chunks_mut(16).enumerate() {
        let counter = (index + 1) as u32;
        let mut block = aes::Block::default();
        block[0] = 2; // L - 1, with L = 3.
        block[1..13].copy_from_slice(&nonce);
        block[13] = (counter >> 16) as u8;
        block[14] = (counter >> 8) as u8;
        block[15] = counter as u8;
        cipher.encrypt_block(&mut block);
        for (byte, key_byte) in chunk.iter_mut().zip(block.iter()) {
            *byte ^= key_byte;
        }
    }

    let mut store_data = Vec::with_capacity(96);
    store_data.extend_from_slice(&plaintext[..12]);
    store_data.extend_from_slice(&wrapped[..8]);
    store_data.extend_from_slice(&plaintext[12..]);
    Ok(store_data)
}

pub fn read_mii_data(path: &Path) -> Result<Vec<u8>, String> {
    let bytes = fs::read(path).map_err(|error| error.to_string())?;
    if supported_binary_extension(path) {
        if bytes.is_empty() {
            return Err("Mii file is empty".into());
        }
        return Ok(bytes);
    }
    if possible_qr_image(path) {
        return unwrap_qr_store_data(&decode_qr_payload(&bytes)?);
    }
    Err("unsupported Mii file type".into())
}

fn decode_utf16_name(bytes: &[u8], big_endian: bool) -> String {
    let units = bytes.chunks_exact(2).map(|pair| {
        if big_endian {
            u16::from_be_bytes([pair[0], pair[1]])
        } else {
            u16::from_le_bytes([pair[0], pair[1]])
        }
    });
    String::from_utf16_lossy(&units.take_while(|unit| *unit != 0).collect::<Vec<_>>())
        .trim()
        .to_string()
}

fn mii_name(data: &[u8]) -> Option<String> {
    let name = match data.len() {
        // nn::mii::CharInfo
        88 => decode_utf16_name(&data[16..36], false),
        // RFLCharData / RFLStoreData
        74 | 76 => decode_utf16_name(&data[2..22], true),
        // FFLiMiiDataCore / Official / StoreData
        72 | 92 | 96 => decode_utf16_name(&data[26..46], false),
        _ => return None,
    };
    (!name.is_empty()).then_some(name)
}

#[allow(non_snake_case)]
#[tauri::command]
pub fn read_mii_name(app_handle: tauri::AppHandle, documentId: String) -> Result<String, String> {
    let documents = app_handle.state::<crate::DocumentState::DocumentState>();
    let (file_type, path) = documents.with(&documentId, |app| {
        (
            app.opened_file.file_type,
            app.opened_file.path.full_path.clone(),
        )
    });
    if file_type != TotkFileType::Mii {
        return Err("active document is not a Mii".into());
    }
    let data = read_mii_data(Path::new(&path))?;
    mii_name(&data).ok_or_else(|| "Mii name is unavailable in this format".into())
}

fn request_render(data: &[u8], extension: &str, width: u32) -> Result<Vec<u8>, String> {
    match request_primary_render(data, extension, width) {
        Ok(result) => Ok(result),
        Err(primary_error) if extension == "png" => match request_studio_render(data, width) {
            Ok(result) => Ok(result),
            Err(fallback_error) => {
                let message = format!(
                    "Primary renderer: {primary_error}\nNintendo Studio fallback: {fallback_error}"
                );
                show_connection_error(&message);
                Err(format!("Mii renderer request failed: {message}"))
            }
        },
        Err(error) => {
            show_connection_error(&error);
            Err(format!("Mii renderer request failed: {error}"))
        }
    }
}

fn request_primary_render(data: &[u8], extension: &str, width: u32) -> Result<Vec<u8>, String> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(data);
    let url = format!("{RENDERER}/image.{extension}");
    let response = renderer_client()?
        .post(url)
        .query(&[
            ("erri", "spc9r-rsm".to_string()),
            ("data", encoded),
            ("type", "face".to_string()),
            ("width", width.to_string()),
        ])
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .send()
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let result = response
        .bytes()
        .map_err(|error| format!("unable to read Mii renderer response: {error}"))?
        .to_vec();
    if !status.is_success() {
        let message = String::from_utf8_lossy(&result);
        return Err(format!("Mii renderer returned HTTP {status}: {message}"));
    }
    match extension {
        "png" if !result.starts_with(b"\x89PNG\r\n\x1a\n") => {
            Err("Mii renderer did not return a PNG".into())
        }
        "glb" if result.len() < 12 || !result.starts_with(b"glTF") => {
            Err("Mii renderer did not return a GLB".into())
        }
        _ => Ok(result),
    }
}

fn studio_data(data: &[u8]) -> Result<[u8; 46], String> {
    let mut dst = [0_u8; 46];
    match data.len() {
        88 => {
            const MAP: [usize; 46] = [
                74, 75, 42, 55, 53, 56, 54, 52, 57, 58, 62, 60, 63, 61, 59, 64, 65, 46, 48, 45, 47,
                39, 40, 80, 81, 79, 82, 50, 51, 49, 41, 84, 83, 85, 86, 72, 70, 71, 69, 73, 77, 76,
                78, 67, 66, 68,
            ];
            for (output, input) in MAP.into_iter().enumerate() {
                dst[output] = data[input];
            }
        }
        72 | 92 | 96 => {
            dst[0] = data[66] >> 3 & 7;
            dst[1] = data[66] & 7;
            dst[2] = data[47];
            dst[3] = data[53] >> 5;
            dst[4] = (data[53] & 1) << 2 | data[52] >> 6;
            dst[5] = data[54] & 31;
            dst[6] = data[53] >> 1 & 15;
            dst[7] = data[52] & 63;
            dst[8] = (data[55] & 1) << 3 | data[54] >> 5;
            dst[9] = data[55] >> 1 & 31;
            dst[10] = data[57] >> 4 & 7;
            dst[11] = data[56] >> 5;
            dst[12] = data[58] & 31;
            dst[13] = data[57] & 15;
            dst[14] = data[56] & 31;
            dst[15] = (data[59] & 1) << 3 | data[58] >> 5;
            dst[16] = data[59] >> 1 & 31;
            dst[17] = data[48] >> 5;
            dst[18] = data[49] >> 4;
            dst[19] = data[48] >> 1 & 15;
            dst[20] = data[49] & 15;
            dst[21] = data[25] >> 2 & 15;
            dst[22] = data[24] & 1;
            dst[23] = data[68] >> 4 & 7;
            dst[24] = (data[69] & 7) * 2 | data[68] >> 7;
            dst[25] = data[68] & 15;
            dst[26] = data[69] >> 3;
            dst[27] = data[51] & 7;
            dst[28] = data[51] >> 3 & 1;
            dst[29] = data[50];
            dst[30] = data[46];
            dst[31] = data[70] >> 1 & 15;
            dst[32] = data[70] & 1;
            dst[33] = (data[71] & 3) << 3 | data[70] >> 5;
            dst[34] = data[71] >> 2 & 31;
            dst[35] = data[63] >> 5;
            dst[36] = (data[63] & 1) << 2 | data[62] >> 6;
            dst[37] = data[63] >> 1 & 15;
            dst[38] = data[62] & 63;
            dst[39] = data[64] & 31;
            dst[40] = (data[67] & 3) << 2 | data[66] >> 6;
            dst[41] = data[64] >> 5;
            dst[42] = data[67] >> 2 & 31;
            dst[43] = (data[61] & 1) << 3 | data[60] >> 5;
            dst[44] = data[60] & 31;
            dst[45] = data[61] >> 1 & 31;
            convert_legacy_studio_fields(&mut dst);
        }
        74 | 76 => {
            const FACE_TEXTURES: [u8; 24] = [
                0, 0, 0, 1, 0, 6, 0, 9, 5, 0, 2, 0, 3, 0, 7, 0, 8, 0, 0, 10, 9, 0, 11, 0,
            ];
            dst[0] = data[50] >> 1 & 7;
            dst[1] = data[50] >> 4 & 3;
            dst[2] = data[23];
            dst[3] = 3;
            dst[4] = data[42] >> 5;
            dst[5] = data[41] >> 5 | (data[40] & 3) << 3;
            dst[6] = data[42] >> 1 & 15;
            dst[7] = data[40] >> 2;
            dst[8] = data[43] >> 5 | (data[42] & 1) << 3;
            dst[9] = data[41] & 31;
            dst[10] = 3;
            dst[11] = data[38] >> 5;
            dst[12] = data[37] >> 6 | (data[36] & 7) << 2;
            dst[13] = data[38] >> 1 & 15;
            dst[14] = data[36] >> 3;
            dst[15] = data[39] & 15;
            dst[16] = data[39] >> 4 | (data[38] & 1) << 4;
            dst[17] = data[32] >> 2 & 7;
            let face_texture = (data[33] >> 6 | (data[32] & 3) << 2) as usize;
            let face_texture = face_texture.min(11) * 2;
            dst[18] = FACE_TEXTURES[face_texture + 1];
            dst[19] = data[32] >> 5;
            dst[20] = FACE_TEXTURES[face_texture];
            dst[21] = data[1] >> 1 & 15;
            dst[22] = data[0] >> 6 & 1;
            dst[23] = data[48] >> 1 & 7;
            dst[24] = data[49] >> 5 | (data[48] & 1) << 3;
            dst[25] = data[48] >> 4;
            dst[26] = data[49] & 31;
            dst[27] = data[35] >> 6 | (data[34] & 1) << 2;
            dst[28] = data[35] >> 5 & 1;
            dst[29] = data[34] >> 1;
            dst[30] = data[22];
            dst[31] = data[52] >> 3 & 15;
            dst[32] = data[52] >> 7;
            dst[33] = data[53] >> 1 & 31;
            dst[34] = data[53] >> 6 | (data[52] & 7) << 2;
            dst[35] = 3;
            dst[36] = data[46] >> 1 & 3;
            dst[37] = data[47] >> 5 | (data[46] & 1) << 3;
            dst[38] = data[46] >> 3;
            dst[39] = data[47] & 31;
            dst[40] = data[51] >> 5 | (data[50] & 1) << 3;
            dst[41] = data[50] >> 6;
            dst[42] = data[51] & 31;
            dst[43] = data[44] & 15;
            dst[44] = data[44] >> 4;
            dst[45] = data[45] >> 3;
            convert_legacy_studio_fields(&mut dst);
        }
        46 => dst.copy_from_slice(data),
        47 => {
            for index in 0..46 {
                dst[index] = data[index + 1].wrapping_sub(7) ^ data[index];
            }
        }
        size => {
            return Err(format!(
                "Nintendo Studio fallback does not support {size}-byte Mii data"
            ))
        }
    }
    Ok(dst)
}

fn convert_legacy_studio_fields(data: &mut [u8; 46]) {
    if data[27] == 0 {
        data[27] = 8;
    }
    if data[0] == 0 {
        data[0] = 8;
    }
    if data[11] == 0 {
        data[11] = 8;
    }
    data[36] += 19;
    data[4] += 8;
    if data[23] == 0 {
        data[23] = 8;
    } else if data[23] < 6 {
        data[23] += 13;
    }
    data[2] = data[2].min(127);
    data[30] = data[30].min(127);
}

fn studio_url_data(data: &[u8]) -> Result<String, String> {
    let raw = studio_data(data)?;
    let mut encoded = [0_u8; 47];
    for index in 0..46 {
        encoded[index + 1] = 7_u8.wrapping_add(raw[index] ^ encoded[index]);
    }
    Ok(encoded.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn request_studio_render(data: &[u8], width: u32) -> Result<Vec<u8>, String> {
    // Nintendo's endpoint rejects the 1024-pixel size accepted by the primary
    // renderer. Its own editor and the archived request use a 270-pixel face.
    let width = width.min(270);
    let response = renderer_client()?
        .get(STUDIO_RENDERER)
        .query(&[
            ("data", studio_url_data(data)?),
            ("width", width.to_string()),
            ("type", "face".to_string()),
        ])
        .send()
        .map_err(|error| error.to_string())?;
    let status = response.status();
    let result = response
        .bytes()
        .map_err(|error| error.to_string())?
        .to_vec();
    if !status.is_success() {
        return Err(format!(
            "HTTP {status}: {}",
            String::from_utf8_lossy(&result)
        ));
    }
    if !result.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err("Nintendo Studio did not return a PNG".into());
    }
    Ok(result)
}

fn unique_sibling(path: &Path, extension: &str) -> PathBuf {
    let first = path.with_extension(extension);
    if !first.exists() {
        return first;
    }
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("mii");
    for suffix in 1_u32.. {
        let candidate = parent.join(format!("{stem}_{suffix}.{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn error_result(path: &Path, message: String) -> (OpenedFile<'static>, SendData) {
    let mut data = SendData::default();
    data.path = Pathlib::new(path);
    data.tab = "ERROR".into();
    data.status_text = format!("Error opening Mii: {message}");
    data.text = data.status_text.clone();
    (OpenedFile::default(), data)
}

pub fn open(path: &Path) -> Option<(OpenedFile<'static>, SendData)> {
    let is_binary = supported_binary_extension(path);
    if !is_binary && !possible_qr_image(path) {
        return None;
    }
    let mii_data = match read_mii_data(path) {
        Ok(data) => data,
        Err(_) if !is_binary => return None, // A normal image, not a Mii QR.
        Err(error) => return Some(error_result(path, error)),
    };
    let png = match request_render(&mii_data, "png", 1024) {
        Ok(png) => png,
        Err(error) => return Some(error_result(path, error)),
    };
    let preview_path = unique_sibling(path, "png");
    if let Err(error) = fs::write(&preview_path, &png) {
        return Some(error_result(path, error.to_string()));
    }

    let mut opened = OpenedFile::from_path(path.to_string_lossy().into_owned(), TotkFileType::Mii);
    opened.visual_data = Some(png);
    let mut data = SendData::default();
    data.path = Pathlib::new(path);
    data.set_file_metadata(TotkFileType::Mii, None);
    data.file_label = format!("{} [Mii]", data.path.name);
    data.tab = "IMAGE".into();
    data.read_only = true;
    data.status_text = format!("Rendered HD Mii preview to {}", preview_path.display());
    Some((opened, data))
}

pub fn download_glb(path: &Path) -> Result<PathBuf, String> {
    let mii_data = read_mii_data(path)?;
    let glb = request_render(&mii_data, "glb", 1024)?;
    let declared_length = u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize;
    if declared_length != glb.len() {
        return Err(format!(
            "incomplete GLB: header declares {declared_length} bytes, received {}",
            glb.len()
        ));
    }
    let output = unique_sibling(path, "glb");
    fs::write(&output, glb).map_err(|error| error.to_string())?;
    Ok(output)
}

#[allow(non_snake_case)]
#[tauri::command]
pub fn download_mii_glb(
    app_handle: tauri::AppHandle,
    documentId: String,
) -> Result<String, String> {
    let documents = app_handle.state::<crate::DocumentState::DocumentState>();
    let (file_type, path) = documents.with(&documentId, |app| {
        (
            app.opened_file.file_type,
            app.opened_file.path.full_path.clone(),
        )
    });
    if file_type != TotkFileType::Mii {
        return Err("Download GLB is only available for Mii documents".into());
    }
    download_glb(Path::new(&path)).map(|path| path.to_string_lossy().replace('\\', "/"))
}

#[allow(non_snake_case)]
#[tauri::command]
pub fn read_glb_preview(
    app_handle: tauri::AppHandle,
    documentId: String,
) -> Result<String, String> {
    let documents = app_handle.state::<crate::DocumentState::DocumentState>();
    let (file_type, bytes) = documents.with(&documentId, |app| {
        (
            app.opened_file.file_type,
            app.opened_file.visual_data.clone(),
        )
    });
    if file_type != TotkFileType::Glb {
        return Err("active document is not a GLB preview".into());
    }
    let bytes = bytes.ok_or_else(|| "GLB preview data is missing".to_string())?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

pub fn open_glb(path: &Path) -> io::Result<(OpenedFile<'static>, SendData)> {
    let bytes = fs::read(path)?;
    if bytes.len() < 12 || !bytes.starts_with(b"glTF") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid GLB header",
        ));
    }
    let mut opened = OpenedFile::from_path(path.to_string_lossy().into_owned(), TotkFileType::Glb);
    opened.visual_data = Some(bytes);
    let mut data = SendData::default();
    data.path = Pathlib::new(path);
    data.set_file_metadata(TotkFileType::Glb, None);
    data.file_label = format!("{} [GLB]", data.path.name);
    data.tab = "3D".into();
    data.read_only = true;
    data.status_text = format!("Opened GLB preview: {}", path.display());
    Ok((opened, data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_sibling_uses_requested_extension() {
        let result = unique_sibling(Path::new("Mii.charinfo"), "glb");
        assert_eq!(result, PathBuf::from("Mii.glb"));
    }

    #[test]
    fn supplied_qr_corpus_decodes_to_store_data() {
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/mii/data/qr_clean");
        if !corpus.is_dir() {
            return;
        }
        let mut decoded = 0;
        for entry in fs::read_dir(corpus).unwrap() {
            let path = entry.unwrap().path();
            if !path.is_file() {
                continue;
            }
            let image = fs::read(&path).unwrap();
            let payload = decode_qr_payload(&image)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let store_data = unwrap_qr_store_data(&payload)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(store_data.len(), 96, "{}", path.display());
            decoded += 1;
        }
        assert!(decoded >= 25, "expected the supplied QR corpus");
    }

    #[test]
    fn archived_studio_url_data_round_trips() {
        let archived = "000f145b5f5e646752585e64737d80909a9ca0b1bdc4ccd3e2edf4050b12135a656c7568726873829499a1b1bcc6d1";
        let bytes: Vec<u8> = archived
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        assert_eq!(bytes.len(), 47);
        assert_eq!(studio_url_data(&bytes).unwrap(), archived);
    }

    #[test]
    fn reads_names_from_supplied_switch_and_wii_miis() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/mii");
        let charinfo = fs::read(root.join("Alphie.charinfo")).unwrap();
        assert_eq!(mii_name(&charinfo).as_deref(), Some("Alphie"));

        let miigx = fs::read(root.join("data/miigx/MiiCharInfo000.miigx")).unwrap();
        assert_eq!(mii_name(&miigx).as_deref(), Some("MiiCharInf"));
    }

    #[test]
    #[ignore = "contacts the public Mii renderer"]
    fn live_renderer_accepts_charinfo_and_qr_data() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/mii/data");
        let charinfo = fs::read(root.join("miis_charinfo/Alphie.charinfo")).unwrap();
        assert!(request_render(&charinfo, "png", 1024)
            .unwrap()
            .starts_with(b"\x89PNG"));
        let qr_image = fs::read(root.join("qr_clean/Alphie.jpg")).unwrap();
        let wrapped = decode_qr_payload(&qr_image).unwrap();
        let qr_data = unwrap_qr_store_data(&wrapped).unwrap();
        assert!(request_render(&qr_data, "glb", 1024)
            .unwrap()
            .starts_with(b"glTF"));
    }

    #[test]
    #[ignore = "contacts Nintendo's public Mii Studio renderer"]
    fn live_studio_fallback_accepts_charinfo() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/mii/Alphie.charinfo");
        let charinfo = fs::read(path).unwrap();
        assert!(request_studio_render(&charinfo, 1024)
            .unwrap()
            .starts_with(b"\x89PNG"));
    }
}
