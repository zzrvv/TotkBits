//tauri commands
use crate::{
    DocumentState::DocumentState, Open_and_Save::SendData, Settings::NO_WINDOW_FLAG,
    TotkApp::SaveData,
};
use reqwest::blocking::Client;
use rfd::MessageDialog;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::{
    env, fs,
    os::windows::process::CommandExt,
    path::Path,
    process::{self, Command},
};
use tauri::Manager;

#[derive(Serialize)]
pub struct BfwavPreview {
    pub path: String,
    pub data_url: String,
    pub size: usize,
    pub sample_rate: u32,
    pub channels: usize,
    pub samples: usize,
    pub looping: bool,
}

#[derive(Serialize)]
pub struct BfwavReplacement {
    pub old_size: usize,
    pub new_size: usize,
    pub increased: bool,
    pub compressed: bool,
    pub sample_rate: u32,
}

#[derive(Serialize)]
pub struct BarsFolderReplacement {
    pub replaced: Vec<String>,
    pub skipped: Vec<String>,
    pub failed: Vec<String>,
    pub oversized: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BfresResolvedTexture {
    name: String,
    path: String,
    source: String,
    data_url: String,
    width: u32,
    height: u32,
}

#[tauri::command]
pub fn inspect_bfres(
    path: String,
) -> Result<crate::file_format::Model3D::bfres::BfresFile, String> {
    require_experimental_visuals()?;
    crate::file_format::Model3D::bfres::BfresFile::from_path(path)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn inspect_3d_model(
    app_handle: tauri::AppHandle,
    documentId: String,
    path: String,
) -> Result<serde_json::Value, String> {
    require_experimental_visuals()?;
    let documents = app_handle.state::<DocumentState>();
    let (internal_bfres, internal_bfres_data, romfs, aoc_path) =
        documents.with(&documentId, |app| {
            (
                app.opened_file.bfres.clone(),
                app.opened_file.bfres_data.clone(),
                app.zstd.totk_config.romfs.clone(),
                app.zstd.totk_config.aoc_path.clone(),
            )
        });
    if let Some(bfres) = internal_bfres {
        let textures = resolve_bfres_textures(
            &bfres,
            Path::new(&path),
            internal_bfres_data.as_deref(),
            Path::new(&romfs),
        );
        let mut value = serde_json::to_value(bfres).map_err(|error| error.to_string())?;
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "resolvedTextures".into(),
                serde_json::to_value(textures).map_err(|error| error.to_string())?,
            );
        }
        return Ok(value);
    }
    let source_data = std::fs::read(&path).map_err(|error| error.to_string())?;
    let data = documents.with(&documentId, |app| {
        app.zstd.try_decompress_safe(&source_data)
    });
    if crate::Settings::Magic::is_fbx(&data) {
        serde_json::to_value(
            crate::parser::fbx::FbxFile::parse(
                &data,
                std::path::Path::new(&path)
                    .file_stem()
                    .and_then(|v| v.to_str())
                    .unwrap_or("FBX"),
            )
            .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())
    } else if crate::Settings::Magic::is_g1m(&data) {
        let g1m = crate::parser::AOC::g1m::G1mFile::parse(
            &data,
            Path::new(&path)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("G1M"),
        )
        .map_err(|error| error.to_string())?;
        let texture_resolution = g1m.resolve_textures(Path::new(&path), Path::new(&aoc_path));
        let mut value = serde_json::to_value(g1m).map_err(|error| error.to_string())?;
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "resolvedTextures".into(),
                serde_json::to_value(texture_resolution.textures)
                    .map_err(|error| error.to_string())?,
            );
            object.insert(
                "textureStats".into(),
                serde_json::json!({
                    "total": texture_resolution.total,
                    "skipped": texture_resolution.skipped,
                }),
            );
        }
        Ok(value)
    } else {
        let bfres = crate::file_format::Model3D::bfres::BfresFile::from_bytes(&data)
            .map_err(|error| error.to_string())?;
        let textures = resolve_bfres_textures(&bfres, Path::new(&path), None, Path::new(&romfs));
        let mut value = serde_json::to_value(bfres).map_err(|error| error.to_string())?;
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "resolvedTextures".into(),
                serde_json::to_value(textures).map_err(|error| error.to_string())?,
            );
        }
        Ok(value)
    }
}

#[tauri::command]
pub fn export_g1m_fbx(
    app_handle: tauri::AppHandle,
    documentId: String,
    source_paths: Vec<String>,
    output: String,
    texture_format: String,
) -> Result<String, String> {
    require_experimental_visuals()?;
    if source_paths.is_empty() {
        return Err("no G1M source paths were supplied".into());
    }
    let documents = app_handle.state::<DocumentState>();
    let aoc_path = documents.with(&documentId, |app| app.zstd.totk_config.aoc_path.clone());
    let mut parsed = Vec::with_capacity(source_paths.len());
    for source in &source_paths {
        let path = Path::new(source);
        let bytes = fs::read(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let model = crate::parser::AOC::g1m::G1mFile::parse_for_export(
            &bytes,
            path.file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("G1M"),
        )
        .map_err(|error| format!("{}: {error}", path.display()))?;
        let textures = model.resolve_textures(path, Path::new(&aoc_path)).textures;
        let prefix = if source_paths.len() > 1 {
            format!(
                "{}: ",
                path.file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or("model")
            )
        } else {
            String::new()
        };
        parsed.push((model, textures, prefix));
    }
    let borrowed: Vec<_> = parsed
        .iter()
        .map(|(model, textures, prefix)| (model, textures.as_slice(), prefix.clone()))
        .collect();
    let armature_name = Path::new(&source_paths[0])
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("G1M");
    crate::parser::fbx::export_g1m(
        &borrowed,
        Path::new(&output),
        crate::parser::fbx::TextureExportFormat::parse(&texture_format)
            .map_err(|error| error.to_string())?,
        armature_name,
    )
    .map_err(|error| error.to_string())?;
    Ok(output)
}

#[tauri::command]
pub fn export_viewport_png(output: String, data_url: String) -> Result<String, String> {
    use base64::Engine;

    let encoded = data_url
        .strip_prefix("data:image/png;base64,")
        .ok_or_else(|| "invalid viewport PNG data URL".to_string())?;
    let png = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| error.to_string())?;
    if !crate::Settings::Magic::is_png(&png) {
        return Err("viewport render did not produce a PNG".into());
    }
    if let Some(parent) = Path::new(&output).parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(&output, png).map_err(|error| error.to_string())?;
    Ok(output)
}

#[tauri::command]
pub async fn inspect_batch_g1m(
    path: String,
) -> crate::file_format::Model3D::BatchRender::BatchG1mInspection {
    crate::file_format::Model3D::BatchRender::inspect_batch_g1m(path).await
}

#[tauri::command]
pub fn list_batch_render_files(
    app_handle: tauri::AppHandle,
    documentId: String,
    source_root: String,
    output_root: String,
    existing_png: String,
    model_kind: String,
) -> Result<Vec<crate::file_format::Model3D::BatchRender::BatchRenderFile>, String> {
    crate::file_format::Model3D::BatchRender::list_batch_render_files(
        app_handle,
        documentId,
        source_root,
        output_root,
        existing_png,
        model_kind,
    )
}

fn resolve_bfres_textures(
    bfres: &crate::file_format::Model3D::bfres::BfresFile,
    source: &Path,
    source_data: Option<&[u8]>,
    romfs: &Path,
) -> Vec<BfresResolvedTexture> {
    let names: HashSet<&str> = bfres
        .materials
        .iter()
        .flat_map(|material| material.texture_slots.iter().map(|slot| slot.name.as_str()))
        .collect();
    let mut textures = resolve_embedded_bntx_textures(source, source_data, &names);
    let resolved_names: HashSet<String> = textures
        .iter()
        .map(|texture| texture.name.to_ascii_lowercase())
        .collect();
    if !crate::TotkConfig::TotkConfig::check_for_zsdic(romfs) {
        return textures;
    }
    let root = romfs.join("TexToGo");
    if !root.is_dir() {
        return textures;
    }
    let files: HashMap<String, std::path::PathBuf> = std::fs::read_dir(&root)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_file() {
                return None;
            }
            let file_name = path.file_name()?.to_str()?.to_ascii_lowercase();
            let logical_name = file_name
                .strip_suffix(".txtg.zs")
                .or_else(|| file_name.strip_suffix(".txtg"))?;
            Some((logical_name.to_owned(), path))
        })
        .collect();
    for name in names {
        if resolved_names.contains(&name.to_ascii_lowercase()) {
            continue;
        }
        let lowercase_name = name.to_ascii_lowercase();
        let logical_name = lowercase_name
            .strip_suffix(".txtg")
            .unwrap_or(&lowercase_name);
        let Some(path) = files.get(logical_name) else {
            continue;
        };
        let Ok(rendered) = crate::file_format::Image::ImageDocument::render_path(path) else {
            continue;
        };
        textures.push(BfresResolvedTexture {
            name: name.to_owned(),
            path: path.to_string_lossy().into_owned(),
            source: "textogo".into(),
            data_url: rendered.data_url,
            width: rendered.width,
            height: rendered.height,
        });
    }
    textures.sort_by(|left, right| left.name.cmp(&right.name));
    textures
}

fn resolve_embedded_bntx_textures(
    source: &Path,
    source_data: Option<&[u8]>,
    referenced_names: &HashSet<&str>,
) -> Vec<BfresResolvedTexture> {
    let disk_data;
    let source_data = match source_data {
        Some(data) => data,
        None => {
            let Ok(data) = std::fs::read(source) else {
                return Vec::new();
            };
            disk_data = data;
            &disk_data
        }
    };
    let Some(offset) = source_data.windows(4).position(|bytes| bytes == b"BNTX") else {
        return Vec::new();
    };
    let data = &source_data[offset..];
    let Ok(bntx) = crate::parser::bntx::BntxFile::parse(data) else {
        return Vec::new();
    };
    let referenced: HashSet<String> = referenced_names
        .iter()
        .map(|name| name.to_ascii_lowercase())
        .collect();
    bntx.textures
        .iter()
        .enumerate()
        .filter(|(_, texture)| referenced.contains(&texture.name.to_ascii_lowercase()))
        .filter_map(|(index, texture)| {
            let rendered =
                crate::file_format::Image::ImageDocument::render_bntx_bytes(data, index).ok()?;
            Some(BfresResolvedTexture {
                name: texture.name.clone(),
                path: format!("{}#{}", source.display(), texture.name),
                source: "embedded".into(),
                data_url: rendered.data_url,
                width: rendered.width,
                height: rendered.height,
            })
        })
        .collect()
}

#[tauri::command]
pub fn render_image(
    app_handle: tauri::AppHandle,
    documentId: String,
    path: String,
    texture_index: Option<usize>,
    array_index: Option<u32>,
    mip_index: Option<u32>,
) -> Result<crate::file_format::Image::RenderedImage, String> {
    require_experimental_visuals()?;
    let documents = app_handle.state::<DocumentState>();
    documents.with(&documentId, |app| {
        crate::file_format::Image::ImageDocument::render_path_selection_with_zstd(
            path,
            texture_index.unwrap_or(0),
            array_index.unwrap_or(0),
            mip_index.unwrap_or(0),
            Some(&app.zstd),
        )
        .map_err(|error| error.to_string())
    })
}

#[tauri::command]
pub fn export_image_png(
    app_handle: tauri::AppHandle,
    documentId: String,
    source: String,
    output: String,
    texture_index: Option<usize>,
    array_index: Option<u32>,
    mip_index: Option<u32>,
) -> Result<(), String> {
    require_experimental_visuals()?;
    let documents = app_handle.state::<DocumentState>();
    documents.with(&documentId, |app| {
        let rendered = crate::file_format::Image::ImageDocument::render_path_selection_with_zstd(
            source,
            texture_index.unwrap_or(0),
            array_index.unwrap_or(0),
            mip_index.unwrap_or(0),
            Some(&app.zstd),
        )
        .map_err(|error| error.to_string())?;
        crate::file_format::Image::ImageDocument::export_rendered_png(&rendered, output)
            .map_err(|error| error.to_string())
    })
}

#[tauri::command]
pub fn replace_dds_image(
    target: String,
    png: String,
    ddsType: String,
    mipCount: u32,
    array_index: Option<u32>,
    mip_index: Option<u32>,
    replacement_format: Option<String>,
) -> Result<(), String> {
    require_experimental_visuals()?;
    let _ = (ddsType, mipCount);
    crate::file_format::Image::ImageDocument::replace_dds_surface(
        target,
        png,
        array_index.unwrap_or(0),
        mip_index.unwrap_or(0),
        replacement_format.as_deref(),
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn rename_bntx_texture(
    app_handle: tauri::AppHandle,
    documentId: String,
    path: String,
    texture_index: usize,
    new_name: String,
) -> Result<(), String> {
    require_experimental_visuals()?;
    let documents = app_handle.state::<DocumentState>();
    documents.with(&documentId, |app| {
        crate::file_format::Image::ImageDocument::rename_bntx_texture(
            path,
            texture_index,
            &new_name,
            &app.zstd,
        )
        .map_err(|error| error.to_string())
    })
}

#[tauri::command]
pub fn replace_bntx_image(
    app_handle: tauri::AppHandle,
    documentId: String,
    target: String,
    png: String,
    texture_index: usize,
    array_index: u32,
    mip_index: u32,
    replacement_format: Option<String>,
) -> Result<(), String> {
    require_experimental_visuals()?;
    let documents = app_handle.state::<DocumentState>();
    documents.with(&documentId, |app| {
        crate::file_format::Image::ImageDocument::replace_bntx_surface(
            target,
            png,
            texture_index,
            array_index,
            mip_index,
            replacement_format.as_deref(),
            &app.zstd,
        )
        .map_err(|error| error.to_string())
    })
}

fn require_experimental_visuals() -> Result<(), String> {
    return Ok(());
    if cfg!(debug_assertions) {
        Ok(())
    } else {
        Err("Experimental 3D and image features are disabled in release builds".into())
    }
}

#[tauri::command]
pub fn list_open_bphcl_documents(
    app_handle: tauri::AppHandle,
) -> Vec<crate::DocumentState::OpenBphclDocument> {
    app_handle.state::<DocumentState>().open_bphcl_documents()
}

#[tauri::command]
pub fn list_bphcl_selectable_nodes(
    app_handle: tauri::AppHandle,
    documentId: String,
) -> Result<Vec<crate::DocumentState::BphclSelectableNode>, String> {
    app_handle
        .state::<DocumentState>()
        .bphcl_selectable_nodes(&documentId)
}

#[tauri::command]
pub fn list_open_hkcl_documents(
    app_handle: tauri::AppHandle,
) -> Vec<crate::DocumentState::OpenHkclDocument> {
    app_handle.state::<DocumentState>().open_hkcl_documents()
}

#[tauri::command]
pub fn list_open_bphhb_documents(
    app_handle: tauri::AppHandle,
) -> Vec<crate::DocumentState::OpenBphhbDocument> {
    app_handle.state::<DocumentState>().open_bphhb_documents()
}

#[tauri::command]
pub fn list_hkcl_selectable_nodes(
    app_handle: tauri::AppHandle,
    documentId: String,
) -> Result<Vec<crate::DocumentState::HkclSelectableNode>, String> {
    app_handle
        .state::<DocumentState>()
        .hkcl_selectable_nodes(&documentId)
}

#[tauri::command]
pub fn validate_physics_merge_request(
    app_handle: tauri::AppHandle,
    request: crate::DocumentState::PhysicsMergeRequest,
) -> crate::DocumentState::PhysicsMergeValidation {
    app_handle
        .state::<DocumentState>()
        .validate_physics_merge_request(&request)
}

#[tauri::command]
pub fn build_physics_merge_graph(
    app_handle: tauri::AppHandle,
    request: crate::DocumentState::PhysicsMergeRequest,
) -> Result<crate::DocumentState::PhysicsGraphMergeResult, String> {
    app_handle
        .state::<DocumentState>()
        .build_physics_merge_graph(&request)
}

#[tauri::command]
pub fn merge_hkcl_nodes_into_bphcl(
    app_handle: tauri::AppHandle,
    request: crate::DocumentState::PhysicsMergeRequest,
) -> Result<crate::DocumentState::BphclMergeResult, String> {
    app_handle
        .state::<DocumentState>()
        .merge_hkcl_nodes_into_bphcl(&request)
}

#[tauri::command]
pub fn commit_rebuilt_physics_document(
    app_handle: tauri::AppHandle,
    request: crate::DocumentState::RebuiltPhysicsDocument,
) -> Result<crate::DocumentState::PhysicsDocumentUpdateResult, String> {
    app_handle
        .state::<DocumentState>()
        .commit_rebuilt_physics_document(request)
}

#[tauri::command]
pub fn remove_bphcl_node(
    app_handle: tauri::AppHandle,
    documentId: String,
    path: String,
) -> Result<crate::DocumentState::BphclMutationResult, String> {
    app_handle
        .state::<DocumentState>()
        .remove_bphcl_node(&documentId, &path)
}

#[tauri::command]
pub fn validate_bphcl_merge_documents(app_handle: tauri::AppHandle) -> bool {
    if app_handle
        .state::<DocumentState>()
        .open_bphcl_documents()
        .len()
        >= 2
    {
        return true;
    }

    MessageDialog::new()
        .set_title("TotkBits - Physics Merge")
        .set_description("Open at least two BPHCL documents before using Physics Merge.")
        .set_level(rfd::MessageLevel::Warning)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
    false
}

#[tauri::command]
pub fn merge_bphcl_nodes(
    app_handle: tauri::AppHandle,
    targetDocumentId: String,
    sourceDocumentId: String,
    nodeIds: Vec<String>,
) -> Result<crate::DocumentState::BphclMergeResult, String> {
    let result = app_handle.state::<DocumentState>().merge_bphcl_nodes(
        &targetDocumentId,
        &sourceDocumentId,
        &nodeIds,
    )?;
    let imported = if result.imported.is_empty() {
        "  (none)".into()
    } else {
        result
            .imported
            .iter()
            .map(|name| format!("  - {name}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let skipped = if result.skipped.is_empty() {
        "  (none)".into()
    } else {
        result
            .skipped
            .iter()
            .map(|item| format!("  - {} — {}", item.name, item.reason))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let text = format!(
        "Physics merge completed.\n\nNodes added: {} ({} cloth, {} collidables)\nSelections merged: {}\n{}\n\nSkipped selections: {}\n{}",
        result.imported_count,
        result.added_cloth_count,
        result.added_collidable_count,
        result.imported_selection_count,
        imported,
        result.skipped_count,
        skipped
    );
    MessageDialog::new()
        .set_title("TotkBits - Physics Merge")
        .set_description(text)
        .set_level(rfd::MessageLevel::Info)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
    Ok(result)
}

fn show_open_error(data: &SendData) {
    let message = if data.status_text.is_empty() {
        &data.text
    } else {
        &data.status_text
    };
    MessageDialog::new()
        .set_title("TotkBits - Open error")
        .set_description(message)
        .set_level(rfd::MessageLevel::Error)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

fn report_monaco_save_error(tab: &str, result: &Option<SendData>, report_missing: bool) {
    if !matches!(tab.to_ascii_uppercase().as_str(), "TEXT" | "YAML") {
        return;
    }
    let message = match result {
        Some(data)
            if data.tab != "ERROR"
                && !data
                    .status_text
                    .trim_start()
                    .to_ascii_lowercase()
                    .starts_with("error") =>
        {
            return;
        }
        Some(data) if !data.status_text.trim().is_empty() => data.status_text.clone(),
        Some(data) if !data.text.trim().is_empty() => data.text.clone(),
        None if !report_missing => return,
        _ => format!("Failed to save the {tab} document. No error details were returned."),
    };
    MessageDialog::new()
        .set_title("TotkBits - Save error")
        .set_description(message)
        .set_level(rfd::MessageLevel::Error)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

macro_rules! with_document_mut {
    ($handle:expr, $id:expr, $app:ident, $body:expr) => {{
        let documents = $handle.state::<DocumentState>();
        documents.with_mut(&$id, |$app| $body)
    }};
}

macro_rules! with_document {
    ($handle:expr, $id:expr, $app:ident, $body:expr) => {{
        let documents = $handle.state::<DocumentState>();
        documents.with(&$id, |$app| $body)
    }};
}

#[tauri::command]
pub fn open_bfwav_node(
    app_handle: tauri::AppHandle,
    documentId: String,
    path: String,
) -> Result<BfwavPreview, String> {
    with_document!(app_handle, documentId, app, app.open_bfwav_node(&path))
}

#[tauri::command]
pub fn replace_bfwav_node(
    app_handle: tauri::AppHandle,
    documentId: String,
    path: String,
    sourcePath: String,
) -> Result<BfwavReplacement, String> {
    with_document_mut!(
        app_handle,
        documentId,
        app,
        app.replace_bfwav_node(&path, Path::new(&sourcePath), false, None, false)
    )
}

#[tauri::command]
pub fn replace_bars_audio_from_folder(
    app_handle: tauri::AppHandle,
    documentId: String,
    folderPath: String,
) -> Result<BarsFolderReplacement, String> {
    with_document_mut!(app_handle, documentId, app, {
        app.replace_bars_audio_from_folder(Path::new(&folderPath), false, false)
    })
}

#[tauri::command]
pub fn open_amta_node(
    app_handle: tauri::AppHandle,
    documentId: String,
    parentDocumentId: String,
    path: String,
) -> Result<SendData, String> {
    app_handle
        .state::<DocumentState>()
        .open_amta_node(&parentDocumentId, &documentId, path)
}

#[tauri::command]
pub fn open_audio_file_dialog() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("Audio", &["wav", "mp3"])
        .pick_file()
        .map(|path| path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn export_bfwav_node(
    app_handle: tauri::AppHandle,
    documentId: String,
    path: String,
    format: String,
) -> Result<Option<String>, String> {
    let format = format.to_ascii_lowercase();
    if format != "wav" && format != "mp3" {
        return Err("audio export format must be WAV or MP3".into());
    }
    let encoded = with_document!(app_handle, documentId, app, {
        let bytes = app
            .archive
            .as_ref()
            .and_then(|archive| archive.get(&path))
            .ok_or_else(|| format!("archive entry not found: {path}"))?;
        if format == "wav" {
            crate::file_format::Audio::to_wav(bytes)
        } else {
            crate::file_format::Audio::to_mp3(bytes)
        }
    })?;
    let stem = Path::new(&path)
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("audio");
    let Some(output) = rfd::FileDialog::new()
        .add_filter(format.to_ascii_uppercase(), &[format.as_str()])
        .set_file_name(format!("{stem}.{format}"))
        .save_file()
    else {
        return Ok(None);
    };
    fs::write(&output, encoded).map_err(|error| format!("failed to export audio: {error}"))?;
    Ok(Some(output.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn restart_app() -> Option<()> {
    let totkbits_exe = env::current_exe().ok()?;
    let no_window_flag = NO_WINDOW_FLAG;
    if let rfd::MessageDialogResult::No = MessageDialog::new()
        .set_title("Warning")
        .set_description("Totkbits will be restarted, all unsaved progress will be lost. Proceed?")
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
    {
        return Some(());
    }
    // let _ = Command::new(totkbits_exe)
    let p = Command::new("cmd")
        .creation_flags(no_window_flag)
        .args([
            "/C",
            "start",
            "",
            &totkbits_exe.to_string_lossy().into_owned(),
        ])
        .spawn();
    // .map(|_| ())?;
    // .ok()?;
    match p {
        Ok(_) => process::exit(0),
        Err(_) => None,
    }
}

#[tauri::command]
pub fn edit_config(app_handle: tauri::AppHandle, documentId: String) -> Option<()> {
    let no_window_flag = NO_WINDOW_FLAG;
    let file_path = with_document!(
        app_handle,
        documentId,
        app,
        app.zstd.totk_config.config_path.clone()
    );
    let os_type = env::consts::OS;

    let result = match os_type {
        "windows" => Command::new("cmd")
            .creation_flags(no_window_flag)
            .args(["/C", "start", "", &file_path])
            .status(),
        "macos" => Command::new("open")
            .creation_flags(no_window_flag)
            .arg(file_path)
            .status(),
        "linux" => Command::new("xdg-open")
            .creation_flags(no_window_flag)
            .arg(file_path)
            .status(),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Unsupported OS",
        )),
    };

    let _ = result.map(|exit_status| {
        if exit_status.success() {
            return Some(());
        } else {
            return None;
        }
    });
    None
}

#[tauri::command]
pub fn extract_internal_file(
    app_handle: tauri::AppHandle,
    documentId: String,
    internalPath: String,
) -> Option<SendData> {
    with_document_mut!(app_handle, documentId, app, app.extract_file(internalPath))
}

#[tauri::command]
pub fn add_empty_byml_file(
    app_handle: tauri::AppHandle,
    documentId: String,
    path: String,
) -> Option<SendData> {
    with_document_mut!(app_handle, documentId, app, app.add_empty_byml(path))
}

#[tauri::command]
pub fn edit_internal_file(
    app_handle: tauri::AppHandle,
    documentId: String,
    parentDocumentId: String,
    path: String,
) -> Option<SendData> {
    let result = app_handle.state::<DocumentState>().open_internal_child(
        &parentDocumentId,
        &documentId,
        None,
        path.clone(),
    );
    let result = result.or_else(|| {
        let mut data = SendData::default();
        data.tab = "ERROR".into();
        data.status_text = format!(
            "Error: unable to open archive entry {path}. The entry is missing, unsupported, or invalid."
        );
        Some(data)
    });
    if let Some(data) = &result {
        if data.tab == "ERROR" {
            show_open_error(data);
        }
    }
    result
}

#[tauri::command]
pub fn open_bphcl_leaf(
    app_handle: tauri::AppHandle,
    documentId: String,
    parentDocumentId: String,
    path: String,
) -> Option<SendData> {
    app_handle
        .state::<DocumentState>()
        .open_bphcl_leaf(&parentDocumentId, &documentId, path)
}

#[tauri::command]
pub fn expand_nested_sarc(
    app_handle: tauri::AppHandle,
    documentId: String,
    outerPath: String,
) -> SendData {
    let result = with_document_mut!(
        app_handle,
        documentId,
        app,
        app.expand_nested_sarc(outerPath)
    );
    if result.tab == "ERROR" && result.status_text.contains("WinRAR rar.exe was not found") {
        show_open_error(&result);
    }
    result
}

#[tauri::command]
pub fn edit_nested_sarc_file(
    app_handle: tauri::AppHandle,
    documentId: String,
    parentDocumentId: String,
    outerPath: String,
    innerPath: String,
) -> SendData {
    let display_path = format!("{outerPath}::{innerPath}");
    let result = app_handle
        .state::<DocumentState>()
        .open_internal_child(&parentDocumentId, &documentId, Some(outerPath), innerPath)
        .unwrap_or_else(|| {
            let mut data = SendData::default();
            data.tab = "ERROR".into();
            data.status_text = format!(
                "Error: unable to open archive entry {display_path}. The entry is missing, unsupported, or invalid."
            );
            data
        });
    if result.tab == "ERROR" {
        show_open_error(&result);
    }
    result
}

#[tauri::command]
pub fn extract_nested_sarc_file(
    app_handle: tauri::AppHandle,
    documentId: String,
    outerPath: String,
    innerPath: String,
) -> Option<SendData> {
    with_document_mut!(
        app_handle,
        documentId,
        app,
        app.extract_nested_sarc_file(outerPath, innerPath)
    )
}

#[tauri::command]
pub fn mutate_nested_archive(
    app_handle: tauri::AppHandle,
    documentId: String,
    chain: String,
    path: String,
    action: String,
    newPath: Option<String>,
    sourcePath: Option<String>,
) -> SendData {
    with_document_mut!(
        app_handle,
        documentId,
        app,
        app.mutate_nested_archive(chain, path, action, newPath, sourcePath)
    )
}

#[tauri::command]
pub fn extract_opened_sarc(app_handle: tauri::AppHandle, documentId: String) -> Option<SendData> {
    with_document!(app_handle, documentId, app, app.extract_opened_sarc())
}

#[tauri::command]
pub fn extract_folder_from_opened_sarc(
    app_handle: tauri::AppHandle,
    documentId: String,
    source_folder: String,
) -> Option<SendData> {
    with_document_mut!(
        app_handle,
        documentId,
        app,
        app.extract_folder_from_opened_sarc(source_folder)
    )
}

#[tauri::command]
pub fn get_toml_config(app_handle: tauri::AppHandle, documentId: String) -> Result<Value, String> {
    with_document!(
        app_handle,
        documentId,
        app,
        app.zstd.totk_config.to_json().map_err(|e| e.to_string())
    )
}

#[tauri::command]
pub fn get_recent_files() -> Result<Vec<String>, String> {
    crate::TotkConfig::TotkConfig::safe_new(false)
        .map(|config| config.recent_files)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_aoc_model_catalog() -> Result<Option<HashMap<String, String>>, String> {
    let config =
        crate::TotkConfig::TotkConfig::safe_new(false).map_err(|error| error.to_string())?;
    let dump_path = Path::new(&config.aoc_path);
    if config.aoc_path.is_empty()
        || !dump_path
            .join("KIDSSystemResource")
            .join("kidsobjdb")
            .join("CharacterEditor.kidsobjdb")
            .is_file()
    {
        return Ok(None);
    }

    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("bin")
        .join("AOC_names.json");
    let contents = fs::read_to_string(manifest_path).or_else(|_| {
        let executable = env::current_exe()?;
        let parent = executable.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "executable directory is missing",
            )
        })?;
        fs::read_to_string(parent.join("bin").join("AOC_names.json"))
    });

    let mut catalog = contents
        .ok()
        .and_then(|value| serde_json::from_str::<HashMap<String, String>>(&value).ok())
        .unwrap_or_default();
    if catalog.is_empty() {
        return Ok(None);
    }
    let pairs_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("bin")
        .join("G1M_to_G1T_pairs.json");
    let pairs = fs::read_to_string(pairs_path).or_else(|_| {
        let executable = env::current_exe()?;
        let parent = executable.parent().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "executable directory is missing",
            )
        })?;
        fs::read_to_string(parent.join("bin").join("G1M_to_G1T_pairs.json"))
    });
    if let Some(pair_keys) = pairs
        .ok()
        .and_then(|value| serde_json::from_str::<HashMap<String, serde_json::Value>>(&value).ok())
    {
        for hash in pair_keys.into_keys() {
            catalog.entry(hash).or_default();
        }
    }
    Ok(Some(catalog))
}

#[tauri::command]
pub fn preview_aoc_model(hash: String) -> Result<Option<String>, String> {
    if hash.len() != 8 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("invalid AOC model hash".into());
    }
    let config =
        crate::TotkConfig::TotkConfig::safe_new(false).map_err(|error| error.to_string())?;
    let dump_path = Path::new(&config.aoc_path);
    let filename = format!("{}.g1m", hash.to_ascii_lowercase());
    let model_path = [
        dump_path.join(&filename),
        dump_path
            .join("CharacterEditor")
            .join("g1m")
            .join(&filename),
        dump_path.join("MaterialEditor").join("g1m").join(&filename),
        dump_path.join("FieldEditor4").join("g1m").join(&filename),
        dump_path
            .join("CharacterEditor")
            .join("g1m_merged")
            .join(hash.to_ascii_lowercase())
            .join(&filename),
        dump_path.join("g1m").join(&filename),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .or_else(|| {
        fs::read_dir(dump_path)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("g1m").join(&filename))
            .find(|path| path.is_file())
    });
    let Some(model_path) = model_path else {
        MessageDialog::new()
            .set_title("AOC model not found")
            .set_description(format!(
                "The model {filename} does not exist in the configured AOC dump path."
            ))
            .set_level(rfd::MessageLevel::Error)
            .set_buttons(rfd::MessageButtons::Ok)
            .show();
        return Ok(None);
    };
    Ok(Some(model_path.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn get_viewport_brightness() -> Result<f64, String> {
    crate::TotkConfig::TotkConfig::safe_new(false)
        .map(|config| config.viewport_brightness)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn set_viewport_brightness(brightness: f64) -> Result<(), String> {
    if !brightness.is_finite() {
        return Err("invalid 3D viewport brightness".into());
    }
    let mut config =
        crate::TotkConfig::TotkConfig::safe_new(false).map_err(|error| error.to_string())?;
    config.viewport_brightness = brightness.clamp(0.3, 3.0);
    config.save().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_toml_config(
    app_handle: tauri::AppHandle,
    documentId: String,
    new_config: HashMap<String, Value>,
) -> Result<SendData, String> {
    let mut config = with_document!(app_handle, documentId, app, (*app.zstd.totk_config).clone());
    config.update_from_json_data(new_config);
    config.save().map_err(|e| e.to_string())?;
    let status_text = app_handle
        .state::<DocumentState>()
        .update_runtime_config(config);
    let mut data = SendData::default();
    data.status_text = status_text;
    Ok(data)
}

#[tauri::command]
pub fn save_as_click(
    app_handle: tauri::AppHandle,
    documentId: String,
    save_data: SaveData,
) -> Option<SendData> {
    let tab = save_data.tab.clone();
    let result = with_document_mut!(app_handle, documentId, app, app.save_as(save_data));
    // `None` also represents the user cancelling the Save As dialog.
    report_monaco_save_error(&tab, &result, false);
    result
}

#[tauri::command]
pub fn add_click(
    app_handle: tauri::AppHandle,
    documentId: String,
    internalPath: String,
    path: String,
    overwrite: bool,
) -> Option<SendData> {
    println!("internal_path: {}", internalPath);
    with_document_mut!(
        app_handle,
        documentId,
        app,
        app.add_internal_file_from_path(internalPath, path, overwrite)
    )
}

#[tauri::command]
pub fn add_files_from_dir_recursively(
    app_handle: tauri::AppHandle,
    documentId: String,
    internalPath: String,
    path: String,
) -> Option<SendData> {
    println!("internal_path: {}", internalPath);
    // if path_
    with_document_mut!(
        app_handle,
        documentId,
        app,
        app.add_dir_to_sarc(internalPath, path)
    )
}

#[tauri::command]
pub fn add_to_dir_click(
    app_handle: tauri::AppHandle,
    documentId: String,
    internalPath: String,
    path: String,
) -> Option<SendData> {
    println!("internal_path: {}", internalPath);
    // if path_
    with_document_mut!(
        app_handle,
        documentId,
        app,
        app.add_internal_file_to_dir(internalPath, path)
    )
}

// #[tauri::command]
// pub fn get_status_text(app: tauri::State<'_, TotkBitsApp>) -> String {
//     let result = panic::catch_unwind(AssertUnwindSafe(|| {
//         app.inner().send_status_text();
//     }));
//     if result.is_err() {
//         return "Error".to_string();
//     }
//     app.status_text.clone()
// }

#[tauri::command]
pub fn open_file_struct(
    app_handle: tauri::AppHandle,
    documentId: String,
    _window: tauri::Window,
) -> Option<SendData> {
    let result = with_document_mut!(app_handle, documentId, app, app.open());
    if let Some(data) = &result {
        if data.tab == "ERROR" {
            show_open_error(data);
        } else if !data.path.full_path.is_empty() {
            let _ = crate::TotkConfig::TotkConfig::remember_recent_file(&data.path.full_path);
        }
    }
    result
}

#[tauri::command]
pub fn open_folder_struct(app_handle: tauri::AppHandle, documentId: String) -> Option<SendData> {
    let result = with_document_mut!(app_handle, documentId, app, app.open_folder());
    if let Some(data) = &result {
        if data.tab == "ERROR" {
            show_open_error(data);
        }
    }
    result
}

#[tauri::command]
pub fn open_file_from_path(
    app_handle: tauri::AppHandle,
    documentId: String,
    path: String,
    suppressErrorDialog: bool,
) -> Option<SendData> {
    let result = with_document_mut!(
        app_handle,
        documentId,
        app,
        app.open_from_path(path.replace("\\", "/"))
    );
    if let Some(data) = &result {
        if data.tab == "ERROR" && !suppressErrorDialog {
            show_open_error(data);
        } else if data.tab != "ERROR" && !data.path.full_path.is_empty() {
            let _ = crate::TotkConfig::TotkConfig::remember_recent_file(&data.path.full_path);
        }
    }
    result
}

#[tauri::command]
pub fn remove_internal_sarc_file(
    app_handle: tauri::AppHandle,
    documentId: String,
    internalPath: String,
) -> Option<SendData> {
    with_document_mut!(
        app_handle,
        documentId,
        app,
        app.remove_internal_elem(internalPath)
    )
}

#[tauri::command]
pub fn save_file_struct(
    app_handle: tauri::AppHandle,
    documentId: String,
    save_data: SaveData,
) -> Option<SendData> {
    let tab = save_data.tab.clone();
    let result = app_handle
        .state::<DocumentState>()
        .save_document(&documentId, save_data);
    report_monaco_save_error(&tab, &result, true);
    result
}
#[tauri::command]
pub fn rename_internal_sarc_file(
    app_handle: tauri::AppHandle,
    documentId: String,
    internalPath: String,
    newInternalPath: String,
) -> Option<SendData> {
    with_document_mut!(
        app_handle,
        documentId,
        app,
        app.rename_internal_file_from_path(internalPath, newInternalPath)
    )
}

#[tauri::command]
pub fn close_all_opened_files(
    app_handle: tauri::AppHandle,
    documentId: String,
) -> Option<SendData> {
    let documents = app_handle.state::<DocumentState>();
    let response = documents.with_mut(&documentId, |app| app.close_all_click());
    documents.close_all();
    response
}

#[tauri::command]
pub fn close_document(app_handle: tauri::AppHandle, documentId: String) -> bool {
    app_handle.state::<DocumentState>().close(&documentId)
}

#[tauri::command]
pub fn exit_app() {
    if MessageDialog::new()
        .set_title("Warning")
        .set_description("The program will be closed, all unsaved progress will be lost. Proceed?")
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        == rfd::MessageDialogResult::Yes
    {
        process::exit(0); // Replace 0 with the desired exit code
    }
}

#[tauri::command]
pub fn open_file_dialog() -> Option<String> {
    match rfd::FileDialog::new().pick_file() {
        Some(path) => Some(path.to_string_lossy().to_string().replace("\\", "/")),
        None => None,
    }
}

#[tauri::command]
pub fn open_dir_dialog(title: Option<String>) -> Option<String> {
    let mut dialog = rfd::FileDialog::new();
    if let Some(title) = title {
        dialog = dialog.set_title(title);
    }
    match dialog.pick_folder() {
        Some(path) => Some(path.to_string_lossy().to_string().replace("\\", "/")),
        None => None,
    }
}

#[tauri::command]
pub fn rstb_get_entries(
    app_handle: tauri::AppHandle,
    documentId: String,
    entry: String,
) -> Option<SendData> {
    with_document_mut!(
        app_handle,
        documentId,
        app,
        app.get_rstb_entries_by_query(entry)
    )
}

#[tauri::command]
pub fn rstb_edit_entry(
    app_handle: tauri::AppHandle,
    documentId: String,
    entry: String,
    val: String,
) -> Option<SendData> {
    with_document_mut!(app_handle, documentId, app, app.rstb_edit_entry(entry, val))
}

#[tauri::command]
pub fn rstb_remove_entry(
    app_handle: tauri::AppHandle,
    documentId: String,
    entry: String,
) -> Option<SendData> {
    with_document_mut!(app_handle, documentId, app, app.rstb_remove_entry(entry))
}

#[tauri::command]
pub fn search_in_sarc(
    app_handle: tauri::AppHandle,
    documentId: String,
    query: String,
) -> Option<SendData> {
    with_document_mut!(app_handle, documentId, app, app.search_in_sarc(query))
}

#[tauri::command]
pub fn clear_search_in_sarc(app_handle: tauri::AppHandle, documentId: String) -> Option<SendData> {
    with_document_mut!(app_handle, documentId, app, app.clear_search_in_sarc())
}

//COMPARE stuff
#[tauri::command]
pub fn compare_files(
    app_handle: tauri::AppHandle,
    documentId: String,
    isFromDisk: bool,
) -> Option<SendData> {
    with_document!(app_handle, documentId, app, app.compare_files(isFromDisk))
}

#[tauri::command]
pub fn compare_internal_file_with_vanila(
    app_handle: tauri::AppHandle,
    documentId: String,
    internal_path: String,
    is_from_sarc: bool,
) -> Option<SendData> {
    app_handle.state::<DocumentState>().compare_internal_file(
        &documentId,
        internal_path,
        is_from_sarc,
    )
}

#[tauri::command]
pub fn check_if_update_needed() -> String {
    let repo_owner = "SolidLink95".to_string();
    let repo_name = "TotkBits".to_string();
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        repo_owner, repo_name
    );
    println!("Checking for updates...");
    let client = Client::new();
    let response = client.get(&url).header("User-Agent", "MyAppName").send();

    if let Ok(response) = response {
        // println!("Response: {:?}", response);

        if let Ok(json_value) = response.json::<serde_json::Value>() {
            // println!("\n\nJson value: {:?}", json_value);
            if let Some(release_info) = json_value["tag_name"].as_str() {
                // println!("\n\nRelease info: {}", release_info);
                let installed_ver = parse_release_version(env!("CARGO_PKG_VERSION"));
                let latest_ver = parse_release_version(release_info);
                if latest_ver.is_some() && latest_ver > installed_ver {
                    return release_info.to_string();
                }
            }
        }
    }
    String::new()
}

fn parse_release_version(version: &str) -> Option<(u32, u32, u32)> {
    let version = version.trim().trim_start_matches(['v', 'V']);
    let version = version.split_once('-').map_or(version, |(core, _)| core);
    let mut parts = version.split('.');
    let parsed = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(parsed)
}

#[cfg(test)]
mod update_check_tests {
    use super::parse_release_version;

    #[test]
    fn parses_github_release_tags() {
        assert_eq!(parse_release_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_release_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_release_version("not-a-version"), None);
    }
}
