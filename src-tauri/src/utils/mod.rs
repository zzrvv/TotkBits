mod AppPaths;
mod FileUtilities;
mod Startup;
mod ValueUtilities;

pub const NO_WINDOW_FLAG: u32 = 0x08000000;

pub use AppPaths::{exe_relative_path, running_exe_dir, Pathlib};
pub use FileUtilities::{
    list_files_recursively, makedirs, read_string_from_file, write_string_to_file,
};
pub use Startup::{get_startup_data, StartupData};
pub use ValueUtilities::{process_inline_content, update_json};
