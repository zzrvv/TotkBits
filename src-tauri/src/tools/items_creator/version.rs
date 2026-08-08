//! Discovery of versioned Product resources from a user's clean ROMFS.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub fn discover_product_file(
    directory: &Path,
    prefix: &str,
    suffix: &str,
) -> io::Result<(String, PathBuf)> {
    let mut matches = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let Some(version) = name
            .strip_prefix(prefix)
            .and_then(|value| value.strip_suffix(suffix))
        else {
            continue;
        };
        if path.is_file()
            && !version.is_empty()
            && version.bytes().all(|value| value.is_ascii_digit())
        {
            matches.push((version.to_owned(), path));
        }
    }
    matches.sort_by(|left, right| left.0.cmp(&right.0));
    match matches.len() {
        0 => Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "no {prefix}<version>{suffix} file found in {}",
                directory.display()
            ),
        )),
        1 => Ok(matches.remove(0)),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "multiple game versions found for {prefix} in {}: {}",
                directory.display(),
                matches
                    .iter()
                    .map(|item| item.0.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        )),
    }
}

pub fn product_name(prefix: &str, version: &str, suffix: &str) -> io::Result<String> {
    if version.is_empty() || !version.bytes().all(|value| value.is_ascii_digit()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid game version",
        ));
    }
    Ok(format!("{prefix}{version}{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_versioned_product_name_without_a_fixed_version() {
        assert_eq!(
            product_name("ActorInfo.Product.", "112", ".rstbl.byml.zs").unwrap(),
            "ActorInfo.Product.112.rstbl.byml.zs"
        );
        assert!(product_name("ActorInfo.Product.", "1.2.1", ".zs").is_err());
    }
}
