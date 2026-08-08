//! BFRES and TexToGo asset generation for custom weapons.

use crate::{
    file_format::{
        Image::{BntxReplacementReport, ImageDocument},
        Model3D::bfres::BfresFile,
    },
    Zstd::TotkZstd,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WeaponModelAssetsRequest {
    /// Substring used by the vanilla model and its texture names.
    #[serde(alias = "base")]
    pub base_name: String,
    /// New model/texture substring and BFRES model name.
    #[serde(alias = "name")]
    pub new_name: String,
    /// BFRES or BFRES.MC source, relative to clean ROMFS unless absolute.
    pub model_source: PathBuf,
    /// Destination relative to the mod ROMFS. Defaults to the source path with
    /// every occurrence of `base_name` replaced by `new_name`.
    #[serde(default)]
    pub model_destination: Option<PathBuf>,
    /// Optional FBX whose polygon meshes replace BFRES geometry. FBX materials
    /// and non-mesh objects are ignored; the BFRES first material is retained.
    #[serde(default, alias = "fbx")]
    pub fbx_path: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WeaponBntxAssetRequest {
    /// BNTX or BNTX.ZS source, relative to clean ROMFS unless absolute.
    pub texture_source: PathBuf,
    /// Custom PNG supplied by the user.
    pub png_source: PathBuf,
    /// New internal name for the sole BNTX texture.
    #[serde(alias = "name")]
    pub new_name: String,
    /// BNTX destination relative to the mod ROMFS.
    pub texture_destination: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GeneratedWeaponAssets {
    pub model: PathBuf,
    pub textures: Vec<PathBuf>,
}

impl WeaponModelAssetsRequest {
    pub fn from_json(text: &str) -> io::Result<Self> {
        serde_json::from_str(text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn generate(
        &self,
        clean_romfs: &Path,
        output_romfs: &Path,
        zstd: Arc<TotkZstd<'_>>,
    ) -> io::Result<GeneratedWeaponAssets> {
        validate_name(&self.base_name, "base_name")?;
        validate_name(&self.new_name, "new_name")?;
        ensure_output_outside_romfs(clean_romfs, output_romfs)?;
        let source = resolve_source(clean_romfs, &self.model_source)?;
        let source_bytes = fs::read(&source)?;
        let raw = if crate::Settings::Magic::is_bfres(&source_bytes) {
            source_bytes
        } else if crate::Settings::Magic::is_mcpk(&source_bytes) {
            zstd.decompress_mcpk(&source_bytes).map_err(|error| {
                invalid_data(format!(
                    "failed to MCPK-decompress base BFRES {}: {error}",
                    source.display()
                ))
            })?
        } else {
            zstd.try_decompress_for_path(&source, &source_bytes)
                .map(|(data, _)| data)
                .map_err(|error| {
                    invalid_data(format!(
                        "failed to decompress base BFRES {}: {error}",
                        source.display()
                    ))
                })?
        };
        let base_bfres = BfresFile::from_bytes(&raw).map_err(io::Error::other)?;
        let major_version = base_bfres
            .header
            .version
            .get(2)
            .copied()
            .ok_or_else(|| invalid_data("BFRES version header is truncated"))?;
        if major_version != 10 {
            return Err(invalid(
                "custom weapon MCPK output requires BFRES version 10",
            ));
        }

        let texture_names: BTreeSet<String> = base_bfres
            .materials
            .iter()
            .flat_map(|material| &material.texture_slots)
            .map(|slot| slot.name.clone())
            .collect();
        let texture_sources = index_textures(&clean_romfs.join("TexToGo"))?;
        let texture_output = output_romfs.join("TexToGo");
        fs::create_dir_all(&texture_output)?;
        let mut copied = Vec::with_capacity(texture_names.len());
        for old_name in texture_names {
            let logical = old_name
                .strip_suffix(".txtg")
                .unwrap_or(&old_name)
                .to_ascii_lowercase();
            let source_texture = texture_sources.get(&logical).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("BFRES texture is missing from clean TexToGo: {old_name}"),
                )
            })?;
            let file_name = source_texture
                .file_name()
                .and_then(|value| value.to_str())
                .ok_or_else(|| invalid("texture filename is not UTF-8"))?;
            let destination_name = replace_file_stem(file_name, &self.base_name, &self.new_name)?;
            let destination = texture_output.join(destination_name);
            fs::copy(source_texture, &destination)?;
            copied.push(destination);
        }

        let container_name = format!("{}.{}", self.new_name, self.new_name);
        let renamed =
            BfresFile::rename_first_model_and_container(&raw, &self.new_name, &container_name)
                .map_err(io::Error::other)?;
        let mut renamed =
            BfresFile::rename_material_texture_slots(&renamed, &self.base_name, &self.new_name)
                .map_err(io::Error::other)?;
        if let Some(fbx_path) = &self.fbx_path {
            let fbx_path = if fbx_path.is_absolute() {
                fbx_path.clone()
            } else {
                std::env::current_dir()?.join(fbx_path)
            };
            if !fbx_path.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("custom FBX is missing: {}", fbx_path.display()),
                ));
            }
            renamed = BfresFile::replace_geometry_from_fbx(&renamed, &fs::read(fbx_path)?)
                .map_err(io::Error::other)?;
        }
        let verified = BfresFile::from_bytes(&renamed).map_err(io::Error::other)?;
        if verified.materials.iter().any(|material| {
            material
                .texture_slots
                .iter()
                .any(|slot| slot.name.contains(&self.base_name))
        }) {
            return Err(invalid(
                "generated BFRES still references a base texture name",
            ));
        }

        let relative_destination = weapon_model_destination(
            self.model_destination.as_deref(),
            &self.model_source,
            &self.new_name,
        );
        validate_relative_path(&relative_destination)?;
        let destination = output_romfs.join(relative_destination);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        // Custom weapon models are always emitted through TotkBits' MeshCodec writer.
        let compressed = zstd
            .compress_mcpk(&renamed)
            .map_err(|error| invalid_data(format!("failed to MCPK-compress BFRES: {error}")))?;
        fs::write(&destination, compressed)?;
        Ok(GeneratedWeaponAssets {
            model: destination,
            textures: copied,
        })
    }
}

impl WeaponBntxAssetRequest {
    pub fn from_json(text: &str) -> io::Result<Self> {
        serde_json::from_str(text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn generate(
        &self,
        clean_romfs: &Path,
        output_romfs: &Path,
        zstd: Arc<TotkZstd<'_>>,
    ) -> io::Result<BntxReplacementReport> {
        validate_name(&self.new_name, "new_name")?;
        validate_relative_path(&self.texture_destination)?;
        ensure_output_outside_romfs(clean_romfs, output_romfs)?;

        let source = resolve_source(clean_romfs, &self.texture_source)?;
        if !self.png_source.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("custom PNG is missing: {}", self.png_source.display()),
            ));
        }
        let destination = output_romfs.join(&self.texture_destination);
        ImageDocument::replace_single_bntx_from_png(
            source,
            destination,
            &self.png_source,
            &self.new_name,
            &zstd,
        )
    }
}

fn index_textures(root: &Path) -> io::Result<BTreeMap<String, PathBuf>> {
    let mut textures = BTreeMap::new();
    for entry in fs::read_dir(root)? {
        let path = entry?.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let lowercase = name.to_ascii_lowercase();
        let logical = lowercase
            .strip_suffix(".txtg.zs")
            .or_else(|| lowercase.strip_suffix(".txtg"));
        if let Some(logical) = logical {
            textures.insert(logical.to_owned(), path);
        }
    }
    Ok(textures)
}

fn replace_file_stem(file_name: &str, base: &str, new_name: &str) -> io::Result<String> {
    let (stem, suffix) = if let Some(stem) = file_name.strip_suffix(".txtg.zs") {
        (stem, ".txtg.zs")
    } else if let Some(stem) = file_name.strip_suffix(".txtg") {
        (stem, ".txtg")
    } else {
        return Err(invalid(format!(
            "unsupported TexToGo filename: {file_name}"
        )));
    };
    Ok(format!("{}{suffix}", stem.replace(base, new_name)))
}

fn weapon_model_destination(requested: Option<&Path>, source: &Path, new_name: &str) -> PathBuf {
    let parent = requested
        .and_then(Path::parent)
        .or_else(|| (!source.is_absolute()).then(|| source.parent()).flatten())
        .unwrap_or_else(|| Path::new("Model"));
    parent.join(format!("{new_name}.{new_name}.bfres.mc"))
}

fn resolve_source(clean_romfs: &Path, source: &Path) -> io::Result<PathBuf> {
    let resolved = if source.is_absolute() {
        source.to_path_buf()
    } else {
        clean_romfs.join(source)
    };
    if !resolved.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("source asset is missing: {}", resolved.display()),
        ));
    }
    Ok(resolved)
}

fn validate_relative_path(path: &Path) -> io::Result<()> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(invalid("asset destination must be a safe relative path"));
    }
    Ok(())
}

fn validate_name(value: &str, field: &str) -> io::Result<()> {
    if value.is_empty() || value.contains(['/', '\\', '\0']) {
        return Err(invalid(format!("invalid {field}")));
    }
    Ok(())
}

pub(super) fn ensure_output_outside_romfs(clean_romfs: &Path, output: &Path) -> io::Result<()> {
    let clean = clean_romfs.canonicalize()?;
    let absolute = if output.is_absolute() {
        output.to_path_buf()
    } else {
        std::env::current_dir()?.join(output)
    };
    let output = resolve_with_existing_ancestor(&absolute)?;
    if output == clean || output.starts_with(&clean) {
        return Err(invalid("output must be outside clean ROMFS"));
    }
    Ok(())
}

/// Canonicalize the closest existing ancestor, then restore the not-yet-created suffix.
/// This catches Windows junctions/symlinks without requiring the output to exist already.
pub(super) fn resolve_with_existing_ancestor(path: &Path) -> io::Result<PathBuf> {
    let mut ancestor = path;
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        let name = ancestor
            .file_name()
            .ok_or_else(|| invalid("output path has no existing ancestor"))?;
        suffix.push(name.to_owned());
        ancestor = ancestor
            .parent()
            .ok_or_else(|| invalid("output path has no existing ancestor"))?;
    }
    let mut resolved = ancestor.canonicalize()?;
    for component in suffix.into_iter().rev() {
        resolved.push(component);
    }
    Ok(resolved)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TotkConfig::TotkConfig, Zstd::TOTK_ZSTD_COMPRESSION_LEVEL};

    #[test]
    fn replaces_only_the_texture_file_stem() {
        assert_eq!(
            replace_file_stem(
                "Weapon_Sword_019_Alb.txtg.zs",
                "Weapon_Sword_019",
                "Weapon_Sword_900"
            )
            .unwrap(),
            "Weapon_Sword_900_Alb.txtg.zs"
        );
    }

    #[test]
    fn weapon_bfres_destination_is_always_actor_dot_actor() {
        assert_eq!(
            weapon_model_destination(
                Some(Path::new("Model/ignored_name.bfres.mc")),
                Path::new("Model/Base.Base.bfres.mc"),
                "Weapon_Sword_900",
            ),
            Path::new("Model/Weapon_Sword_900.Weapon_Sword_900.bfres.mc")
        );
        assert_eq!(
            weapon_model_destination(
                None,
                Path::new("Model/Base.Base.bfres.mc"),
                "Weapon_Sword_900",
            ),
            Path::new("Model/Weapon_Sword_900.Weapon_Sword_900.bfres.mc")
        );
    }

    #[test]
    fn parses_bntx_asset_request_from_json() {
        let request = WeaponBntxAssetRequest::from_json(
            r#"{
                "texture_source": "UI/Tex/Icon/Weapon_Sword_019.bntx.zs",
                "png_source": "custom.png",
                "name": "Weapon_Sword_900",
                "texture_destination": "UI/Tex/Icon/Weapon_Sword_900.bntx.zs"
            }"#,
        )
        .unwrap();
        assert_eq!(request.new_name, "Weapon_Sword_900");
        assert!(request.texture_destination.is_relative());
    }

    #[test]
    #[ignore = "requires a configured clean ROMFS"]
    fn copies_textures_rewrites_slots_and_emits_mcpk() {
        let clean_romfs = Path::new("E:/TOTK_modding/0100F2C0115B6000/romfs");
        let model = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/bfres/Weapon_Sword_019.Weapon_Sword_019.bfres");
        if !model.is_file() {
            return;
        }
        let mut config = TotkConfig::default();
        config.romfs = clean_romfs.to_string_lossy().into_owned();
        let zstd = Arc::new(
            TotkZstd::new(Arc::new(config), TOTK_ZSTD_COMPRESSION_LEVEL)
                .expect("load ROMFS dictionaries"),
        );
        let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/assets_generated_romfs");
        let request = WeaponModelAssetsRequest {
            base_name: "Weapon_Sword_019".into(),
            new_name: "Weapon_Sword_900".into(),
            model_source: model,
            model_destination: Some("Model/Weapon_Sword_900.Weapon_Sword_900.bfres.mc".into()),
            fbx_path: None,
        };
        let generated = request
            .generate(clean_romfs, &output, zstd.clone())
            .unwrap();
        assert!(!generated.textures.is_empty());
        let compressed = fs::read(&generated.model).unwrap();
        assert!(crate::Settings::Magic::is_mcpk(&compressed));
        let raw = zstd.decompress_mcpk(&compressed).unwrap();
        let bfres = BfresFile::from_bytes(&raw).unwrap();
        assert_eq!(bfres.name.as_deref(), Some("Weapon_Sword_900"));
        assert!(bfres.materials.iter().all(|material| material
            .texture_slots
            .iter()
            .all(|slot| !slot.name.contains("Weapon_Sword_019"))));
        fs::remove_dir_all(output).unwrap();
    }

    #[test]
    #[ignore = "requires the configured clean ROMFS and supplied weapon FBX"]
    fn saves_supplied_fbx_as_custom_mcpk_bfres() {
        let clean_romfs = Path::new("E:/TOTK_modding/0100F2C0115B6000/romfs");
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp");
        let model = root.join("bfres/Weapon_Sword_022.Weapon_Sword_022.bfres");
        let fbx = root.join("Weapon_Sword_022.fbx");
        if !clean_romfs.is_dir() || !model.is_file() || !fbx.is_file() {
            return;
        }
        let mut config = TotkConfig::default();
        config.romfs = clean_romfs.to_string_lossy().into_owned();
        let zstd = Arc::new(
            TotkZstd::new(Arc::new(config), TOTK_ZSTD_COMPRESSION_LEVEL)
                .expect("load ROMFS dictionaries"),
        );
        let output = root.join("test_sic/romfs");
        let request = WeaponModelAssetsRequest {
            base_name: "Weapon_Sword_022".into(),
            new_name: "Weapon_Lsword_005".into(),
            model_source: model,
            model_destination: Some("Model/Weapon_Lsword_005.Weapon_Lsword_005.bfres.mc".into()),
            fbx_path: Some(fbx.clone()),
        };
        let imported =
            crate::parser::fbx::import::import_for_bfres(&fs::read(fbx).unwrap()).unwrap();
        let generated = request
            .generate(clean_romfs, &output, zstd.clone())
            .unwrap();
        let compressed = fs::read(&generated.model).unwrap();
        assert!(crate::Settings::Magic::is_mcpk(&compressed));
        let raw = zstd.decompress_mcpk(&compressed).unwrap();
        let source_flags = fs::read(&request.model_source).unwrap();
        assert_eq!(&raw[0xEE..0xF0], &source_flags[0xEE..0xF0]);
        let saved = BfresFile::from_bytes(&raw).unwrap();
        assert_eq!(
            saved.name.as_deref(),
            Some("Weapon_Lsword_005.Weapon_Lsword_005")
        );
        assert_eq!(
            saved
                .sections_with_signature(b"FMDL")
                .next()
                .and_then(|model| model.name.as_deref()),
            Some("Weapon_Lsword_005")
        );
        assert_eq!(
            saved
                .render
                .meshes
                .iter()
                .map(|mesh| mesh.positions.len())
                .sum::<usize>(),
            imported
                .meshes
                .iter()
                .map(|mesh| mesh.positions.len())
                .sum::<usize>()
        );
        let blade = saved
            .render
            .meshes
            .iter()
            .find(|mesh| mesh.name.to_ascii_lowercase().contains("blade_hide"))
            .expect("saved blade-hide mesh");
        assert!(blade
            .positions
            .iter()
            .flatten()
            .all(|value| value.is_finite()));
        assert_eq!(
            saved
                .render
                .meshes
                .iter()
                .map(|mesh| mesh.indices.len())
                .sum::<usize>(),
            imported
                .meshes
                .iter()
                .map(|mesh| mesh.indices.len())
                .sum::<usize>()
        );
        let blade_material = saved
            .materials
            .iter()
            .position(|material| material.name.to_ascii_lowercase().contains("blade_hide"))
            .map(|index| index as u16)
            .unwrap_or(0);
        let default_material = u16::from(saved.materials.len() > 1);
        let mut expected_materials: Vec<_> = imported
            .meshes
            .iter()
            .map(|mesh| {
                if mesh.name.to_ascii_lowercase().contains("blade_hide") {
                    blade_material
                } else {
                    default_material
                }
            })
            .collect();
        let mut actual_materials: Vec<_> = saved
            .render
            .meshes
            .iter()
            .map(|mesh| mesh.material_index)
            .collect();
        expected_materials.sort_unstable();
        actual_materials.sort_unstable();
        assert_eq!(actual_materials, expected_materials);
    }
}
