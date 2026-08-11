use crate::Settings::{exe_relative_path, NO_WINDOW_FLAG};
use std::{
    io::{self, Write},
    os::windows::process::CommandExt,
    path::Path,
    process::{Command, Output, Stdio},
};

pub struct ExeWrapper {
    pub exe: String,
    pub args: Vec<String>,
}

impl ExeWrapper {
    pub fn new(exe: String, args: Vec<String>) -> Self {
        let exe = if Path::new(&exe).is_relative() {
            let release_path = exe_relative_path(&exe);
            if release_path.is_file() {
                release_path
            } else {
                Path::new(env!("CARGO_MANIFEST_DIR")).join(&exe)
            }
            .to_string_lossy()
            .into_owned()
        } else {
            exe
        };
        Self { exe, args }
    }

    pub fn binary_to_string(&self, data: &[u8], operation: String) -> io::Result<String> {
        let output = self.run(&operation, data)?;
        String::from_utf8(output.stdout)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn string_to_binary(&self, text: &str, operation: String) -> io::Result<Vec<u8>> {
        self.run(&operation, text.as_bytes())
            .map(|output| output.stdout)
    }

    fn run(&self, operation: &str, input: &[u8]) -> io::Result<Output> {
        if !Path::new(&self.exe).is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("converter executable is missing: {}", self.exe),
            ));
        }
        let mut child = Command::new(&self.exe)
            .creation_flags(NO_WINDOW_FLAG)
            .args(&self.args)
            .arg(operation)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::BrokenPipe, "converter stdin unavailable")
            })?
            .write_all(input)?;
        let output = child.wait_with_output()?;
        if output.status.success()
            && !output.stdout.starts_with(b"Error")
            && !output.stderr.to_ascii_lowercase().starts_with(b"error")
        {
            return Ok(output);
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(io::Error::other(format!(
            "converter {operation} failed: {}{}",
            stderr.trim(),
            stdout.trim()
        )))
    }
}
