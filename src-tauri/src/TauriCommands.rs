//tauri commands
use crate::{
    DocumentState::DocumentState,
    Open_and_Save::SendData,
    Settings::{spawn_updater, NO_WINDOW_FLAG},
    TotkApp::SaveData,
};
use reqwest::blocking::{get, Client};
use rfd::MessageDialog;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::{
    env,
    error::Error,
    os::windows::process::CommandExt,
    path::Path,
    process::{self, Command},
};
use tauri::Manager;
use updater::TotkbitsVersion::TotkbitsVersion;
use windows::{
    core::PCWSTR,
    Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_OK},
};

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
        Err(_) => {
            return None;
        }
    };
    // process::exit(0);
    // #[allow(unreachable_code)]
    Some(())
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
    path: String,
) -> Option<SendData> {
    with_document_mut!(app_handle, documentId, app, app.edit_internal_file(path))
}

#[tauri::command]
pub fn expand_nested_sarc(
    app_handle: tauri::AppHandle,
    documentId: String,
    outerPath: String,
) -> SendData {
    with_document_mut!(
        app_handle,
        documentId,
        app,
        app.expand_nested_sarc(outerPath)
    )
}

#[tauri::command]
pub fn edit_nested_sarc_file(
    app_handle: tauri::AppHandle,
    documentId: String,
    outerPath: String,
    innerPath: String,
) -> SendData {
    with_document_mut!(
        app_handle,
        documentId,
        app,
        app.edit_nested_sarc_file(outerPath, innerPath)
    )
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
    if !crate::TotkConfig::TotkConfig::check_for_zsdic(&config.romfs) {
        return Err("ZSTD dictionary pack was not found in the selected romfs".to_string());
    }
    config.save().map_err(|e| e.to_string())?;
    let mut data = SendData::default();
    data.status_text =
        "ZSTD available. Restart to apply backend configuration changes.".to_string();
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
pub fn open_file_from_path(
    app_handle: tauri::AppHandle,
    documentId: String,
    path: String,
) -> Option<SendData> {
    let result = with_document_mut!(
        app_handle,
        documentId,
        app,
        app.open_from_path(path.replace("\\", "/"))
    );
    if let Some(data) = &result {
        if data.tab == "ERROR" {
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
    with_document_mut!(app_handle, documentId, app, app.save(save_data))
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
    with_document_mut!(
        app_handle,
        documentId,
        app,
        app.compare_internal_file_with_original(internal_path, is_from_sarc)
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
