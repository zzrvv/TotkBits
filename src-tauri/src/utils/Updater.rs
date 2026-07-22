use super::Pathlib;
use rfd::MessageDialog;
use std::{
    fs, io,
    path::Path,
    process::{self, Command},
};

pub const BACKUP_UPDATER_NAME: &str = "backup_updater.exe";
pub const NO_WINDOW_FLAG: u32 = 0x08000000;

pub fn spawn_updater(latest_ver: &str) -> io::Result<()> {
    let version = env!("CARGO_PKG_VERSION").to_string();
    if MessageDialog::new()
        .set_title("Update Available")
        .set_description(&format!("Update available: {version} -> {latest_ver}\nTotkBits will be closed, make sure to save all opened files.\nProceed?"))
        .set_buttons(rfd::MessageButtons::YesNo)
        .show()
        != rfd::MessageDialogResult::Yes
    {
        return Ok(());
    }

    let updater = if cfg!(debug_assertions) {
        "../ext_projects/updater/target/debug/updater.exe"
    } else {
        "updater.exe"
    };
    let updater = fs::canonicalize(updater)?;
    if !updater.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Updater executable not found: {}", updater.display()),
        ));
    }
    let updater = updater.to_string_lossy().replace("\\\\?\\", "");
    let backup =
        format!("{}\\{BACKUP_UPDATER_NAME}", Pathlib::new(&updater).parent).replace('/', "\\");
    if Path::new(&backup).exists() {
        fs::remove_file(&backup)?;
    }
    fs::copy(&updater, &backup)?;
    Command::new("cmd")
        .arg("/c")
        .arg("start")
        .arg(&backup)
        .arg(&version)
        .arg(latest_ver)
        .spawn()?;
    process::exit(0);
}
