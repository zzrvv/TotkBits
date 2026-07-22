#![allow(non_snake_case, non_camel_case_types)]
use super::BinTextFile::write_string_to_file;
use crate::file_format::BinTextFile::OpenedFile;
use crate::Open_and_Save::SendData;
use crate::Settings::{exe_relative_path, Pathlib, NO_WINDOW_FLAG};
use crate::Zstd::{is_asb, TotkFileType, TotkZstd};
use std::path::Path;
use std::sync::Arc;
use std::{
    io::{self, Read, Write},
    os::windows::process::CommandExt,
    process::{Command, Stdio},
};

pub struct Asb_py<'a> {
    pub zstd: Arc<TotkZstd<'a>>,
    pub python_exe: String,
    pub python_script: String,
    pub create_no_window: u32,
    pub data: Vec<u8>,
}

#[allow(dead_code, unused_variables)]
impl<'a> Asb_py<'a> {
    pub fn open_asb<P: AsRef<Path>>(
        path: P,
        zstd: Arc<TotkZstd<'a>>,
    ) -> Option<(
        super::BinTextFile::OpenedFile<'a>,
        crate::Open_and_Save::SendData,
    )> {
        let mut opened_file = OpenedFile::default();
        let path_ref = path.as_ref();
        let mut data = SendData::default();
        print!("Is {} a asb? ", &path_ref.display());
        if let Ok(asb) = Asb_py::from_binary_file(path_ref, zstd.clone()) {
            match asb.binary_to_text() {
                Ok(text) => {
                    println!(" yes!");
                    opened_file.path = Pathlib::new(path_ref);
                    opened_file.file_type = TotkFileType::ASB;
                    data.status_text = format!("Opened: {}", &opened_file.path.full_path);
                    data.path = Pathlib::new(path_ref);
                    data.text = text;
                    data.get_file_label(TotkFileType::ASB, Some(roead::Endian::Little));
                    return Some((opened_file, data));
                }
                Err(e) => {
                    println!(" yes but failed to open: {}", e);
                }
            }
            // if let Ok(text) = asb.binary_to_text() {
            //     println!(" yes!");
            //     opened_file.path = Pathlib::new(path_ref);
            //     opened_file.file_type = TotkFileType::ASB;
            //     data.status_text = format!("Opened: {}", &opened_file.path.full_path);
            //     data.path = Pathlib::new(path_ref);
            //     data.text = text;
            //     data.get_file_label(TotkFileType::ASB, Some(roead::Endian::Little));
            //     return Some((opened_file, data));
            // } else {
            //     println!("{} yes but failed to convert to text", &path_ref.display());
            // }
        }
        println!(" no");
        None
    }
    pub fn new(zstd: Arc<TotkZstd<'a>>) -> Asb_py<'a> {
        Self {
            zstd: zstd.clone(),
            python_exe: exe_relative_path("bin/winpython/python-3.11.8.amd64/python.exe").to_string_lossy().into_owned(),
            python_script: "totkbits.py".to_string(),
            create_no_window: NO_WINDOW_FLAG,
            data: Vec::new(),
        }
    }
    pub fn from_binary_file<P: AsRef<Path>>(
        file_path: P,
        zstd: Arc<TotkZstd<'a>>,
    ) -> io::Result<Asb_py<'a>> {
        let mut f_handle = std::fs::File::open(file_path)?; // Open the file
        let mut buffer = Vec::new(); // Create a buffer to store the data
        f_handle.read_to_end(&mut buffer)?; // Read the file into the buffer
        Self::from_binary(&buffer, zstd.clone())
    }
    pub fn from_binary(data: &Vec<u8>, zstd: Arc<TotkZstd<'a>>) -> io::Result<Asb_py<'a>> {
        let new_data = if !is_asb(data) {
            zstd.decompressor.decompress_zs(data)?
        } else {
            data.to_vec()
        };
        if !is_asb(&new_data) {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "Data is not an ASB file.",
            ));
        }
        Ok(Self {
            zstd: zstd.clone(),
            python_exe: exe_relative_path("bin/winpython/python-3.11.8.amd64/python.exe").to_string_lossy().into_owned(),
            python_script: "totkbits.py".to_string(),
            create_no_window: 0x08000000,
            data: new_data,
        })
    }

    pub fn binary_file_to_text(&mut self, file_path: &str) -> io::Result<String> {
        // env::set_var("PATH", self.newpath.clone());
        let mut f_handle = std::fs::File::open(file_path)?; // Open the file
        let mut buffer = Vec::new(); // Create a buffer to store the data
        f_handle.read_to_end(&mut buffer)?; // Read the file into the buffer
        if !is_asb(&buffer) {
            buffer = self.zstd.decompressor.decompress_zs(&buffer)?;
        }
        if !is_asb(&buffer) {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "File is not an ASB file.",
            ));
        }
        self.data = buffer;
        let text = self.binary_to_text()?;
        // env::set_var("PATH", self.original_path.clone());
        Ok(text)
    }

    pub fn text_file_to_binary(&self, file_path: &str) -> io::Result<Vec<u8>> {
        // env::set_var("PATH", self.newpath.clone());
        let mut f_handle = std::fs::File::open(file_path)?; // Open the file
        let mut buffer = Vec::new(); // Create a buffer to store the data
        f_handle.read_to_end(&mut buffer)?; // Read the file into the buffer
        let text = String::from_utf8_lossy(&buffer).into_owned();

        self.text_to_binary(&text)
    }

    pub fn text_to_binary_file(&self, text: &str, file_path: &str) -> io::Result<()> {
        let mut data = self.text_to_binary(&text)?;
        if file_path.to_lowercase().ends_with(".zs") {
            // data = self.zstd.compressor.compress_zs(&data)?;
            data = self.zstd.compress_zs(&data)?;
        }
        let mut f_handle = std::fs::File::create(file_path)?;
        f_handle.write_all(&data)?;
        Ok(())
    }

    pub fn binary_to_text(&self) -> io::Result<String> {
        let mut child = Command::new(&self.python_exe)
            .creation_flags(self.create_no_window)
            .arg(&self.python_script)
            .arg("asb_binary_to_text")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(ref mut stdin) = child.stdin.take() {
            stdin.write_all(&self.data)?;
        } // Dropping `stdin` here closes the pipe.

        let output = child.wait_with_output()?;
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        // write_string_to_file("stderr.log", &stderr)?;
        // write_string_to_file("stdout.log", &stdout)?;
        if stdout.to_lowercase().starts_with("error") {
            return Err(io::Error::new(io::ErrorKind::Other, stdout));
        }
        if stderr.to_lowercase().starts_with("error") {
            return Err(io::Error::new(io::ErrorKind::Other, stderr));
        }

        if output.status.success() {
            // println!("Script executed successfully.");
            eprintln!(
                "Script execution successfully. {:#?}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr).into_owned()
            );
        } else {
            eprintln!("Script execution failed. {:#?}\n{}", output.status, &stderr);
            // eprintln!("Data: {:?}", &stdout);
            let e = format!(
                "Script execution failed. Unable to convert asb binary to text. \n{:#?}\n{}",
                output.status, &stderr
            );
            return Err(io::Error::new(io::ErrorKind::Other, e));
        }
        Ok(stdout)
    }

    pub fn text_to_binary(&self, text: &str) -> io::Result<Vec<u8>> {
        let mut child = Command::new(&self.python_exe)
            .creation_flags(self.create_no_window)
            .arg(&self.python_script)
            .arg("asb_text_to_binary")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        if let Some(ref mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes())?;
        } // Dropping `stdin` here closes the pipe.

        let output = child.wait_with_output()?;
        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        // write_string_to_file("stderr.log", &stderr)?;

        if output.stdout.starts_with(b"Error") {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                String::from_utf8_lossy(&output.stdout).into_owned(),
            ));
        }
        if stderr.to_lowercase().starts_with("error") {
            return Err(io::Error::new(io::ErrorKind::Other, stderr));
        }

        // write_string_to_file("stdout.log", &stdout)?;
        if output.status.success() {
            println!("Script executed successfully.");
        } else {
            eprintln!("Script execution failed.");
            let e = format!(
                "Script execution failed. Unable to convert asb text to binary. \n{:#?}\n{}",
                output.status, &stderr
            );
            return Err(io::Error::new(io::ErrorKind::Other, e));
        }
        Ok(output.stdout)
    }

    pub fn test_winpython(&self) -> io::Result<()> {
        // env::set_var("PATH", self.newpath.clone());
        let output = Command::new(&self.python_exe)
            .arg(&self.python_script)
            .creation_flags(self.create_no_window)
            // .arg("-V")
            .output()?;
        if output.status.success() {
            println!("Script executed successfully.");
        } else {
            eprintln!(
                "Script execution failed. {:#?}\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr).into_owned()
            );
        }
        let text = String::from_utf8_lossy(&output.stdout);
        // env::set_var("PATH", self.original_path.clone());
        println!("Test response from winpython: {}", text);
        Ok(())
    }
}
