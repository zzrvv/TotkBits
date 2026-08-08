//! Whole-mod RESTBL estimation and versioned table generation.

use crate::{parser::rstb::ResourceSizeTable, tools::RstbEstimate::RstbEstimator, Zstd::TotkZstd};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

const PRODUCT_PREFIX: &str = "ResourceSizeTable.Product.";
const PRODUCT_SUFFIX: &str = ".rsizetable.zs";

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RstbGenerationReport {
    pub output: PathBuf,
    pub yaml: PathBuf,
    pub product_version: String,
    pub entries: BTreeMap<String, u32>,
}

pub struct ModRstbProcessor<'a> {
    clean_romfs: PathBuf,
    output_romfs: PathBuf,
    zstd: Arc<TotkZstd<'a>>,
}

impl<'a> ModRstbProcessor<'a> {
    pub fn new(clean_romfs: &Path, output_romfs: &Path, zstd: Arc<TotkZstd<'a>>) -> Self {
        Self {
            clean_romfs: clean_romfs.to_path_buf(),
            output_romfs: output_romfs.to_path_buf(),
            zstd,
        }
    }

    /// Estimates every generated resource below mod ROMFS, updates the user's versioned
    /// ResourceSizeTable, and emits `rstb.yaml` with unhashed resource paths for review.
    pub fn generate(&self) -> io::Result<RstbGenerationReport> {
        super::assets::ensure_output_outside_romfs(&self.clean_romfs, &self.output_romfs)?;
        if !self.output_romfs.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "mod ROMFS directory is missing: {}",
                    self.output_romfs.display()
                ),
            ));
        }
        let clean_resource = self.clean_romfs.join("System/Resource");
        let (product_version, source) =
            super::version::discover_product_file(&clean_resource, PRODUCT_PREFIX, PRODUCT_SUFFIX)?;

        let mut estimator = RstbEstimator::new(self.zstd.clone());
        estimator.set_vanilla_romfs(&self.clean_romfs);
        estimator
            .estimate_folder(&self.output_romfs)
            .map_err(io::Error::other)?;
        let estimated_entries: BTreeMap<String, u32> = estimator
            .entries
            .iter()
            .map(|(path, value)| (path.replace('\\', "/"), *value))
            .collect();
        let source_bytes = fs::read(&source)?;
        let (raw, dictionary) = self.zstd.try_decompress_for_path(&source, &source_bytes)?;
        let mut table = ResourceSizeTable::from_bytes(&raw).map_err(io::Error::other)?;
        let mut entries = BTreeMap::new();
        for (path, estimated) in estimated_entries {
            let vanilla = table.get(path.clone()).copied();
            let Some(value) = changed_mod_value(
                estimated,
                vanilla,
                estimator.modified_sarc_entries.contains(&path),
            )?
            else {
                continue;
            };
            table.set(path.clone(), value);
            entries.insert(path, value);
        }
        estimator.entries = entries
            .iter()
            .map(|(path, value)| (path.clone(), *value))
            .collect();
        let yaml = self.output_romfs.join("rstb.yaml");
        estimator.save_yaml(&yaml).map_err(io::Error::other)?;
        let rebuilt = table.to_bytes().map_err(io::Error::other)?;
        let file_name = source
            .file_name()
            .ok_or_else(|| invalid_data("ResourceSizeTable filename is missing"))?;
        let output = self.output_romfs.join("System/Resource").join(file_name);
        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(
            &output,
            self.zstd.compress_with_dictionary(&rebuilt, dictionary)?,
        )?;

        let (verified, _) = self
            .zstd
            .try_decompress_for_path(&output, &fs::read(&output)?)?;
        let verified = ResourceSizeTable::from_bytes(&verified).map_err(io::Error::other)?;
        for (path, expected) in &entries {
            if verified.get(path.clone()) != Some(expected) {
                return Err(invalid_data(format!(
                    "generated ResourceSizeTable entry is missing: {path}"
                )));
            }
        }
        Ok(RstbGenerationReport {
            output,
            yaml,
            product_version,
            entries,
        })
    }
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Returns the value that must be written for a mod resource. New resource
/// paths are always retained. Existing paths are retained only when the safe
/// estimate exceeds the vanilla allocation; equal or smaller estimates keep
/// the original table unchanged and are omitted from the review YAML.
fn changed_mod_value(
    estimated: u32,
    vanilla: Option<u32>,
    modified_binary: bool,
) -> io::Result<Option<u32>> {
    if modified_binary {
        if let Some(original) = vanilla {
            if estimated <= original {
                return original
                    .checked_add(0x20)
                    .map(Some)
                    .ok_or_else(|| invalid_data("modified resource RSTB value overflow"));
            }
        }
    }
    let value = vanilla.map_or(estimated, |original| estimated.max(original));
    Ok((vanilla != Some(value)).then_some(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        tools::items_creator::{vendor::VendorProcessor, VendorTarget},
        TotkConfig::TotkConfig,
        Zstd::TOTK_ZSTD_COMPRESSION_LEVEL,
    };

    #[test]
    fn yaml_value_filter_retains_only_custom_or_changed_resources() {
        assert_eq!(changed_mod_value(120, None, false).unwrap(), Some(120));
        assert_eq!(changed_mod_value(120, Some(100), false).unwrap(), Some(120));
        assert_eq!(changed_mod_value(100, Some(100), false).unwrap(), None);
        assert_eq!(changed_mod_value(80, Some(100), false).unwrap(), None);
        assert_eq!(changed_mod_value(100, Some(100), true).unwrap(), Some(132));
        assert_eq!(changed_mod_value(80, Some(100), true).unwrap(), Some(132));
    }

    #[test]
    #[ignore = "requires a configured clean ROMFS"]
    fn real_mod_estimates_pack_and_only_changed_internal_entries() {
        let clean_romfs = Path::new("E:/TOTK_modding/0100F2C0115B6000/romfs");
        let Ok((version, clean_rstb)) = super::super::version::discover_product_file(
            &clean_romfs.join("System/Resource"),
            PRODUCT_PREFIX,
            PRODUCT_SUFFIX,
        ) else {
            return;
        };
        if !clean_romfs
            .join("Pack/Actor/Npc_TripMaster_00.pack.zs")
            .is_file()
        {
            return;
        }
        let clean_rstb_bytes = fs::read(&clean_rstb).unwrap();
        let mut config = TotkConfig::default();
        config.romfs = clean_romfs.to_string_lossy().into_owned();
        let zstd = Arc::new(TotkZstd::new(Arc::new(config), TOTK_ZSTD_COMPRESSION_LEVEL).unwrap());
        let output_romfs =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/rstb_generated_romfs");
        let vendor = VendorTarget {
            actor_name: "Npc_TripMaster_00".into(),
            buying_price: None,
            selling_price: None,
            quantity: 2,
        };
        VendorProcessor::new(clean_romfs, &output_romfs, zstd.clone())
            .add_weapon("Weapon_Lsword_900", &vendor)
            .unwrap();

        let report = ModRstbProcessor::new(clean_romfs, &output_romfs, zstd.clone())
            .generate()
            .unwrap();
        assert_eq!(report.product_version, version);
        assert!(report
            .entries
            .contains_key("Pack/Actor/Npc_TripMaster_00.pack"));
        assert!(report.entries.contains_key(
            "Component/ShopParam/Npc_TripMaster_00.game__component__ShopParam.bgyml"
        ));
        assert!(!report
            .entries
            .contains_key("Actor/Npc_TripMaster_00.engine__actor__ActorParam.bgyml"));
        assert!(!report.entries.contains_key("rstb.yaml"));
        let yaml: BTreeMap<String, u32> =
            serde_yaml::from_slice(&fs::read(&report.yaml).unwrap()).unwrap();
        assert_eq!(yaml, report.entries);
        assert_eq!(fs::read(clean_rstb).unwrap(), clean_rstb_bytes);

        let (raw, _) = zstd
            .try_decompress_for_path(&report.output, &fs::read(&report.output).unwrap())
            .unwrap();
        let table = ResourceSizeTable::from_bytes(&raw).unwrap();
        for (path, value) in &report.entries {
            assert_eq!(table.get(path.clone()), Some(value));
        }
        fs::remove_dir_all(output_romfs).unwrap();
    }
}
