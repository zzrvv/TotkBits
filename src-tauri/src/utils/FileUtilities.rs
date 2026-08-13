use std::{
    fs,
    io::{self, BufWriter, Read, Write},
    path::{Path, PathBuf},
};

pub fn write_string_to_file(path: &str, content: &str) -> io::Result<()> {
    let mut writer = BufWriter::new(fs::File::create(path)?);
    writer.write_all(content.as_bytes())?;
    writer.flush()
}

#[allow(dead_code)]
pub fn read_string_from_file(path: &str) -> io::Result<String> {
    let mut contents = String::new();
    fs::File::open(path)?.read_to_string(&mut contents)?;
    Ok(contents)
}

pub fn makedirs(path: &PathBuf) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

pub fn list_files_recursively<T: AsRef<Path>>(path: &T) -> Vec<String> {
    let mut files = Vec::new();
    let Ok(entries) = fs::read_dir(path) else {
        return files;
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        if entry_path.is_file() {
            if let Some(path) = entry_path.to_str() {
                files.push(path.replace('\\', "/"));
            }
        } else if entry_path.is_dir() {
            files.extend(list_files_recursively(&entry_path));
        }
    }
    files
}
