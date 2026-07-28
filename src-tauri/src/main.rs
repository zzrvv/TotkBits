// Prevents additional console window on Windows in release, DO NOT REMOVE!!
// #![windows_subsystem = "windows"]
// #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(non_snake_case, non_camel_case_types)]
use std::fs;
use std::path::PathBuf;
use std::{env, io};
use tauri::Manager;
use Settings::BACKUP_UPDATER_NAME;
use Zstd::get_executable_dir;
mod Cli;
mod Comparer;
mod DocumentState;
mod InternalFile;
mod InternalFile_EX;
mod NestedSarc;
mod Open_and_Save;
mod Open_and_Save_EX;
pub mod utils;
pub use utils as Settings;
mod TauriCommands;
mod TotkApp;
mod TotkConfig;
mod Zstd;
mod compression;
mod file_format;
mod parser;
pub mod tools;
use crate::DocumentState::DocumentState as Documents;
use crate::Settings::{get_startup_data, StartupData};
use crate::TauriCommands::{
    add_click, add_empty_byml_file, add_files_from_dir_recursively, add_to_dir_click,
    build_physics_merge_graph, check_if_update_needed, clear_search_in_sarc,
    close_all_opened_files, close_document, commit_rebuilt_physics_document, compare_files,
    compare_internal_file_with_vanila, edit_config, edit_internal_file, edit_nested_sarc_file,
    exit_app, expand_nested_sarc, export_bfwav_node, export_image_png,
    extract_folder_from_opened_sarc, extract_internal_file, extract_nested_sarc_file,
    extract_opened_sarc, get_recent_files, get_toml_config, inspect_3d_model, inspect_bfres,
    list_bphcl_selectable_nodes, list_hkcl_selectable_nodes, list_open_bphcl_documents,
    list_open_bphhb_documents, list_open_hkcl_documents, merge_bphcl_nodes,
    merge_hkcl_nodes_into_bphcl, mutate_nested_archive, open_amta_node, open_audio_file_dialog,
    open_bfwav_node, open_bphcl_leaf, open_dir_dialog, open_file_dialog, open_file_from_path,
    open_file_struct, open_folder_struct, remove_bphcl_node, remove_internal_sarc_file,
    rename_bntx_texture, rename_internal_sarc_file, render_image, replace_bars_audio_from_folder,
    replace_bfwav_node, replace_bntx_image, replace_dds_image, restart_app, rstb_edit_entry,
    rstb_get_entries, rstb_remove_entry, save_as_click, save_file_struct, search_in_sarc,
    update_app, update_toml_config, validate_bphcl_merge_documents, validate_physics_merge_request,
};

fn main() -> io::Result<()> {
    let cli = Cli::CliCommand::from_env();
    if let Some(command) = cli {
        return command.execute().map_err(std::io::Error::other);
    }
    main_initialization()?;
    // test_case()?;
    // return Ok(());
    let startup_data = StartupData::new()?.to_json()?;
    // println!("{:?}", startup_data);
    let documents = Documents::default();
    if let Err(err) = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app_setup| {
            app_setup.manage(startup_data);

            Ok(())
        })
        .manage(documents)
        .invoke_handler(tauri::generate_handler![
            inspect_bfres,
            inspect_3d_model,
            render_image,
            rename_bntx_texture,
            replace_bntx_image,
            export_image_png,
            replace_dds_image,
            open_bfwav_node,
            replace_bfwav_node,
            replace_bars_audio_from_folder,
            open_amta_node,
            export_bfwav_node,
            open_audio_file_dialog,
            add_empty_byml_file,
            extract_opened_sarc,
            extract_folder_from_opened_sarc,
            get_toml_config,
            get_recent_files,
            update_toml_config,
            restart_app,
            edit_config,
            get_startup_data,
            open_file_struct,
            open_folder_struct,
            open_file_from_path,
            edit_internal_file,
            open_bphcl_leaf,
            list_open_bphcl_documents,
            list_bphcl_selectable_nodes,
            list_open_hkcl_documents,
            list_open_bphhb_documents,
            list_hkcl_selectable_nodes,
            validate_bphcl_merge_documents,
            merge_bphcl_nodes,
            merge_hkcl_nodes_into_bphcl,
            validate_physics_merge_request,
            build_physics_merge_graph,
            commit_rebuilt_physics_document,
            remove_bphcl_node,
            expand_nested_sarc,
            edit_nested_sarc_file,
            extract_nested_sarc_file,
            mutate_nested_archive,
            save_file_struct,
            save_as_click,
            add_click,
            add_to_dir_click,
            extract_internal_file,
            rename_internal_sarc_file,
            close_all_opened_files,
            close_document,
            remove_internal_sarc_file,
            exit_app,
            open_file_dialog,
            rstb_get_entries,
            rstb_edit_entry,
            rstb_remove_entry,
            search_in_sarc,
            clear_search_in_sarc,
            open_dir_dialog,
            add_files_from_dir_recursively,
            //COMPARER
            compare_files,
            compare_internal_file_with_vanila,
            check_if_update_needed,
            update_app
        ])
        .run(tauri::generate_context!())
    {
        rfd::MessageDialog::new()
            .set_buttons(rfd::MessageButtons::Ok)
            .set_title("Error while running tauri application")
            .set_description(format!("{:?}", err))
            .show();
    }
    Ok(())
}

fn main_initialization() -> io::Result<()> {
    #[allow(unused_variables)]
    let exe_cwd = get_executable_dir();
    if exe_cwd.len() > 0 {
        env::set_current_dir(&exe_cwd)?;
        let backup_updater = PathBuf::from(&exe_cwd).join(BACKUP_UPDATER_NAME);
        if backup_updater.exists() {
            if let Ok(_) = fs::remove_file(&backup_updater) {
                println!("[+] Removed {}", BACKUP_UPDATER_NAME);
            }
        }
    }
    let version = env!("CARGO_PKG_VERSION").to_string();
    println!("[+] Totkbits version: {}", &version);
    println!("[+] Current directory: {:?}", exe_cwd);
    // let installed_ver = TotkbitsVersion::from_str(&version);
    Ok(())
}
