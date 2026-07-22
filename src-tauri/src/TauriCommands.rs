//tauri commands
use crate::{
    DocumentState::DocumentState,
    Open_and_Save::SendData,
    Settings::{spawn_updater, NO_WINDOW_FLAG},
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
use updater::TotkbitsVersion::TotkbitsVersion;
use windows::{
    core::PCWSTR,
    Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_ICONWARNING, MB_OK},
};

#[derive(Serialize)]
pub struct BfwavPreview {
    pub path: String,
    pub data_url: String,
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
    let (internal_bfres, romfs) = documents.with(&documentId, |app| {
        (
            app.opened_file.bfres.clone(),
            app.zstd.totk_config.romfs.clone(),
        )
    });
    if let Some(bfres) = internal_bfres {
        let textures = resolve_bfres_textures(&bfres, Path::new(&romfs));
        let mut value = serde_json::to_value(bfres).map_err(|error| error.to_string())?;
        if let Some(object) = value.as_object_mut() {
            object.insert(
                "resolvedTextures".into(),
                serde_json::to_value(textures).map_err(|error| error.to_string())?,
            );
        }
        return Ok(value);
    }
    let data = std::fs::read(&path).map_err(|error| error.to_string())?;
    if data.starts_with(b"Kaydara FBX Binary") {
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
    } else {
        let bfres = crate::file_format::Model3D::bfres::BfresFile::from_path(path)
            .map_err(|error| error.to_string())?;
        let textures = resolve_bfres_textures(&bfres, Path::new(&romfs));
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

fn resolve_bfres_textures(
    bfres: &crate::file_format::Model3D::bfres::BfresFile,
    romfs: &Path,
) -> Vec<BfresResolvedTexture> {
    if !crate::TotkConfig::TotkConfig::check_for_zsdic(romfs) {
        return Vec::new();
    }
    let root = romfs.join("TexToGo");
    if !root.is_dir() {
        return Vec::new();
    }
    let names: HashSet<&str> = bfres
        .materials
        .iter()
        .flat_map(|material| material.texture_slots.iter().map(|slot| slot.name.as_str()))
        .collect();
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
    let mut textures = Vec::with_capacity(names.len());
    for name in names {
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
            data_url: rendered.data_url,
            width: rendered.width,
            height: rendered.height,
        });
    }
    textures.sort_by(|left, right| left.name.cmp(&right.name));
    textures
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

    let message: Vec<u16> = "Open at least two BPHCL documents before using Physics Merge."
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let title: Vec<u16> = "TotkBits - Physics Merge"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONWARNING,
        );
    }
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
        "Physics merge completed.\n\nMerged: {}\n{}\n\nSkipped: {}\n{}",
        result.imported_count, imported, result.skipped_count, skipped
    );
    let message: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let title: Vec<u16> = "TotkBits - Physics Merge"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONINFORMATION,
        );
    }
    Ok(result)
}

fn show_open_error(data: &SendData) {
    let message = if data.status_text.is_empty() {
        &data.text
    } else {
        &data.status_text
    };
    let message: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    let title: Vec<u16> = "TotkBits - Open error"
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR(message.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK,
        );
    }
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
    app_handle.state::<DocumentState>().open_internal_child(
        &parentDocumentId,
        &documentId,
        None,
        path,
    )
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
    app_handle
        .state::<DocumentState>()
        .open_internal_child(&parentDocumentId, &documentId, Some(outerPath), innerPath)
        .unwrap_or_else(|| {
            let mut data = SendData::default();
            data.tab = "ERROR".into();
            data.status_text = "Error: unsupported or missing nested entry".into();
            data
        })
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
pub fn update_toml_config(
    app_handle: tauri::AppHandle,
    documentId: String,
    new_config: HashMap<String, Value>,
) -> Result<SendData, String> {
    let mut config = with_document!(app_handle, documentId, app, (*app.zstd.totk_config).clone());
    config.update_from_json_data(new_config);
    let dictionaries_available = crate::TotkConfig::TotkConfig::check_for_zsdic(&config.romfs);
    config.save().map_err(|e| e.to_string())?;
    let mut data = SendData::default();
    data.status_text = if dictionaries_available {
        "ZSTD available. Restart to apply backend configuration changes.".to_string()
    } else {
        "Settings saved. Restart to use empty-dictionary ZSTD and Yaz0 only.".to_string()
    };
    Ok(data)
}

#[tauri::command]
pub fn save_as_click(
    app_handle: tauri::AppHandle,
    documentId: String,
    save_data: SaveData,
) -> Option<SendData> {
    with_document_mut!(app_handle, documentId, app, app.save_as(save_data))
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
    app_handle
        .state::<DocumentState>()
        .save_document(&documentId, save_data)
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
pub fn open_dir_dialog() -> Option<String> {
    match rfd::FileDialog::new().pick_folder() {
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
                let installed_ver = TotkbitsVersion::from_str(env!("CARGO_PKG_VERSION"));
                let latest_ver = TotkbitsVersion::from_str(release_info);
                if latest_ver > installed_ver {
                    return release_info.to_string();
                }
            }
        }
    }
    String::new()
}

#[tauri::command]
pub fn update_app(latestVer: String) -> String {
    if let Err(e) = spawn_updater(latestVer.as_str()) {
        return format!("Error spawning updater: {:?}", e);
    }
    // process::exit(1);
    String::new()
}

#[cfg(test)]
mod texture_resolution_tests {
    use super::*;

    #[test]
    fn resolves_only_material_referenced_textogo_files() {
        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let bfres = crate::file_format::Model3D::bfres::BfresFile::from_path(
            workspace.join("tmp/bfres/Animal_Bull.Bull.bfres"),
        )
        .expect("failed to parse BFRES texture fixture");
        let texture_name = bfres
            .materials
            .iter()
            .flat_map(|material| &material.texture_slots)
            .next()
            .expect("BFRES fixture has no texture slots")
            .name
            .clone();
        let root = std::env::temp_dir().join(format!(
            "totkbits-textogo-resolution-{}",
            std::process::id()
        ));
        let texture_root = root.join("TexToGo");
        std::fs::create_dir_all(root.join("Pack")).expect("failed to create ROMFS Pack folder");
        std::fs::create_dir_all(&texture_root).expect("failed to create TexToGo folder");
        std::fs::write(root.join("Pack/ZsDic.pack.zs"), []).expect("failed to create ROMFS marker");
        std::fs::copy(
            workspace.join("tmp/tex/Armor_1006_Lower_Alb.7.txtg"),
            texture_root.join(format!("{texture_name}.txtg")),
        )
        .expect("failed to stage TexToGo fixture");

        let textures = resolve_bfres_textures(&bfres, &root);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(textures.len(), 1);
        assert_eq!(textures[0].name, texture_name);
        assert!(textures[0].data_url.starts_with("data:image/png;base64,"));
    }
}
