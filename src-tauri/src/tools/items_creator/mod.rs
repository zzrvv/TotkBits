//! Planning and validation primitives for creating custom weapon mods.
//!
//! Binary mutation is intentionally split from planning. A weapon mod touches several
//! global databases, so callers should validate a manifest and review the generated
//! plan before any output files are written.

use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

pub mod actor_pack;
pub mod assets;
pub mod ecocat;
pub mod gamedata;
pub mod messages;
pub mod rsdb;
pub mod rstb;
pub mod sharp_info;
pub mod vendor;
mod version;

const SHARP_INFO: &str = "GameParameter/SharpInfo/Default.game__weapon__SharpInfoTable.bgyml";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WeaponKind {
    Sword,
    LargeSword,
    Spear,
    Bow,
    Shield,
}

impl WeaponKind {
    fn actor_prefix(self) -> &'static str {
        match self {
            Self::Sword => "Weapon_Sword_",
            Self::LargeSword => "Weapon_Lsword_",
            Self::Spear => "Weapon_Spear_",
            Self::Bow => "Weapon_Bow_",
            Self::Shield => "Weapon_Shield_",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct VendorTarget {
    #[serde(alias = "name", alias = "vendor")]
    /// Existing vanilla actor, for example `Npc_TripMaster_00`.
    pub actor_name: String,
    #[serde(default)]
    pub buying_price: Option<i32>,
    #[serde(default)]
    pub selling_price: Option<i32>,
    #[serde(default = "default_quantity", alias = "stock")]
    pub quantity: u32,
}

fn default_quantity() -> u32 {
    1
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WeaponAssets {
    /// Optional custom FBX. The vanilla BFRES is resolved from `template_actor`.
    #[serde(default)]
    pub fbx: Option<PathBuf>,
    /// `.txtg` files copied into `romfs/TexToGo`.
    #[serde(default)]
    pub textures: Vec<PathBuf>,
    /// Optional replacement for the inventory icon image.
    #[serde(default)]
    pub icon_png: Option<PathBuf>,
    /// Optional replacement for the Hyrule Compendium small image.
    #[serde(default)]
    pub picture_book_icon_png: Option<PathBuf>,
    /// Optional replacement for the Hyrule Compendium detail image.
    #[serde(default)]
    pub picture_book_detail_png: Option<PathBuf>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WeaponSpec {
    pub actor_name: String,
    pub kind: WeaponKind,
    /// Existing actor pack used as the structural template.
    pub template_actor: String,
    /// Model project/FMDB name. This normally equals `actor_name`.
    #[serde(default)]
    pub model_name: String,
    /// Common values written into WeaponParam, LifeParameters, and AttachmentParam.
    #[serde(default)]
    pub weapon_parameters: actor_pack::WeaponParameterOverrides,
    /// Explicit internal renames and typed parameter changes for the cloned actor pack.
    /// When empty, the standard six actor-specific files are specialized automatically.
    #[serde(default)]
    pub actor_pack: actor_pack::ActorPackPolicy,
    /// Optional standalone or vanilla-actor SLink parameter source.
    #[serde(default)]
    pub sound: Option<actor_pack::LinkParameterSource>,
    /// Optional standalone or vanilla-actor ELink parameter source.
    #[serde(default)]
    pub effect: Option<actor_pack::LinkParameterSource>,
    /// Existing vanilla actor whose Phive and Physics entries should be reused.
    #[serde(default, alias = "physics_actor")]
    pub physics: Option<String>,
    /// Existing vanilla actor whose Chemical entries should be reused.
    #[serde(default, alias = "chemical_actor")]
    pub chemical: Option<String>,
    /// Actor assigned to the first ShootableActorSettings entry.
    #[serde(default)]
    pub shootable: Option<String>,
    pub display_name: String,
    pub description: String,
    pub assets: WeaponAssets,
    /// Existing travelling merchants. New vendor creation is out of scope.
    #[serde(default)]
    pub vendors: Vec<VendorTarget>,
}

impl WeaponSpec {
    pub fn from_json(text: &str) -> io::Result<Self> {
        serde_json::from_str(text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn from_toml(text: &str) -> io::Result<Self> {
        toml::from_str(text).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    pub fn effective_model_name(&self) -> &str {
        if self.model_name.is_empty() {
            &self.actor_name
        } else {
            &self.model_name
        }
    }

    /// Builds this weapon's actor pack from its vanilla template into a mod ROMFS tree.
    pub fn clone_actor_pack(
        &self,
        clean_romfs: &Path,
        output_romfs: &Path,
        zstd: std::sync::Arc<crate::Zstd::TotkZstd<'_>>,
    ) -> io::Result<PathBuf> {
        actor_pack::validate_weapon_template_category(
            clean_romfs,
            &self.template_actor,
            zstd.clone(),
        )?;
        let output = output_romfs
            .join("Pack/Actor")
            .join(format!("{}.pack.zs", self.actor_name));
        let generated_policy;
        let policy = if self.actor_pack == actor_pack::ActorPackPolicy::default() {
            let mut parameters = self.weapon_parameters.clone();
            parameters.model_name = Some(self.effective_model_name().to_owned());
            if rsdb::template_is_shield(clean_romfs, &self.template_actor, zstd.clone())? {
                parameters.shield_bash_damage = None;
            }
            generated_policy = actor_pack::ActorPackPolicy::standard_weapon_clone(
                &self.template_actor,
                &self.actor_name,
                parameters,
            )?;
            &generated_policy
        } else {
            &self.actor_pack
        };
        actor_pack::clone_vanilla_actor_pack_with_links(
            clean_romfs,
            &self.template_actor,
            &self.actor_name,
            &output,
            policy,
            self.sound.as_ref(),
            self.effect.as_ref(),
            self.physics.as_deref(),
            self.chemical.as_deref(),
            self.shootable.as_deref(),
            zstd,
        )?;
        Ok(output)
    }

    /// Generates the weapon RSDB rows, including optional vendor buying/selling prices.
    pub fn generate_rsdb(
        &self,
        clean_romfs: &Path,
        output_romfs: &Path,
        zstd: std::sync::Arc<crate::Zstd::TotkZstd<'_>>,
    ) -> io::Result<Vec<PathBuf>> {
        let request = rsdb::WeaponRsdbRequest {
            actor_name: self.actor_name.clone(),
            template_actor: self.template_actor.clone(),
            model_name: Some(self.effective_model_name().to_owned()),
            max_life: self.weapon_parameters.max_life,
            equipment_performance: self.weapon_parameters.base_attack,
            buying_price: self.vendors.first().and_then(|vendor| vendor.buying_price),
            selling_price: self.vendors.first().and_then(|vendor| vendor.selling_price),
            attachment_damage: self.weapon_parameters.additional_damage,
            shield_bash_damage: self.weapon_parameters.shield_bash_damage,
            overrides: rsdb::WeaponRsdbOverrides::default(),
        };
        request.generate(clean_romfs, output_romfs, zstd)
    }

    /// Adds this weapon to the modifier table used when weapon instances are created.
    pub fn generate_sharp_info(
        &self,
        clean_romfs: &Path,
        output_romfs: &Path,
        zstd: std::sync::Arc<crate::Zstd::TotkZstd<'_>>,
    ) -> io::Result<PathBuf> {
        sharp_info::generate_weapon_sharp_info(
            clean_romfs,
            output_romfs,
            &self.actor_name,
            &self.template_actor,
            zstd,
        )
    }

    /// Adds this weapon to the selected existing vendor and writes only to mod ROMFS.
    pub fn generate_vendor_pack(
        &self,
        clean_romfs: &Path,
        output_romfs: &Path,
        zstd: std::sync::Arc<crate::Zstd::TotkZstd<'_>>,
    ) -> io::Result<Vec<vendor::VendorPackReport>> {
        let processor = vendor::VendorProcessor::new(clean_romfs, output_romfs, zstd);
        self.vendors
            .iter()
            .map(|target| processor.add_weapon(&self.actor_name, target))
            .collect()
    }

    /// Generates both the vendor ShopParam pack and matching priced RSDB weapon rows.
    pub fn generate_vendor(
        &self,
        clean_romfs: &Path,
        output_romfs: &Path,
        zstd: std::sync::Arc<crate::Zstd::TotkZstd<'_>>,
    ) -> io::Result<Option<vendor::VendorGenerationReport>> {
        if self.vendors.is_empty() {
            return Ok(None);
        }
        let rsdb_outputs = self.generate_rsdb(clean_romfs, output_romfs, zstd.clone())?;
        let processor = vendor::VendorProcessor::new(clean_romfs, output_romfs, zstd);
        let vendor_packs = self
            .vendors
            .iter()
            .map(|target| processor.add_weapon(&self.actor_name, target))
            .collect::<io::Result<Vec<_>>>()?;
        Ok(Some(vendor::VendorGenerationReport {
            vendor_packs,
            rsdb_outputs,
        }))
    }

    /// Finalizes a completed mod ROMFS by estimating all generated resources and updating RSTB.
    /// Call this after actor packs, assets, RSDB, messages, GameDataList, and vendor files exist.
    pub fn generate_rstb(
        &self,
        clean_romfs: &Path,
        output_romfs: &Path,
        zstd: std::sync::Arc<crate::Zstd::TotkZstd<'_>>,
    ) -> io::Result<rstb::RstbGenerationReport> {
        rstb::ModRstbProcessor::new(clean_romfs, output_romfs, zstd).generate()
    }

    pub fn validate(&self, asset_root: &Path) -> io::Result<()> {
        if !self.actor_name.starts_with(self.kind.actor_prefix()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "actor {} does not match {:?} prefix {}",
                    self.actor_name,
                    self.kind,
                    self.kind.actor_prefix()
                ),
            ));
        }
        if self.actor_name == self.template_actor {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "custom actor and template actor must differ",
            ));
        }
        if self.display_name.trim().is_empty() || self.description.trim().is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "display name and description are required",
            ));
        }
        for vendor in &self.vendors {
            if !vendor.actor_name.starts_with("Npc_TripMaster_") {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "only existing Npc_TripMaster_* vendors are supported initially",
                ));
            }
            if vendor.quantity == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "vendor quantity must be greater than zero",
                ));
            }
            if vendor.buying_price.is_some_and(|price| price < 0)
                || vendor.selling_price.is_some_and(|price| price < 0)
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "vendor buying and selling prices cannot be negative",
                ));
            }
        }

        for source in self.asset_sources() {
            let resolved = if source.is_absolute() {
                source.to_path_buf()
            } else {
                asset_root.join(source)
            };
            if !resolved.is_file() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    format!("required source asset is missing: {}", resolved.display()),
                ));
            }
        }
        Ok(())
    }

    fn asset_sources(&self) -> Vec<&Path> {
        let mut result = [
            self.assets.fbx.as_deref(),
            self.assets.icon_png.as_deref(),
            self.assets.picture_book_icon_png.as_deref(),
            self.assets.picture_book_detail_png.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        result.extend(self.assets.textures.iter().map(PathBuf::as_path));
        result
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanAction {
    CloneActorPack,
    CopyAsset,
    PatchByml,
    PatchTagProduct,
    PatchMessages,
    PatchRstb,
    PatchVendorPack,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PlannedFile {
    pub relative_path: PathBuf,
    pub action: PlanAction,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct GenerationPlan {
    pub files: Vec<PlannedFile>,
}

impl GenerationPlan {
    pub fn for_weapon(spec: &WeaponSpec, clean_romfs: &Path) -> io::Result<Self> {
        let (version, _) = version::discover_product_file(
            &clean_romfs.join("RSDB"),
            "ActorInfo.Product.",
            ".rstbl.byml.zs",
        )?;
        Self::for_weapon_version(spec, &version)
    }

    pub fn for_weapon_version(spec: &WeaponSpec, version: &str) -> io::Result<Self> {
        let actor = &spec.actor_name;
        let mut files = vec![
            planned(
                format!("Pack/Actor/{actor}.pack.zs"),
                PlanAction::CloneActorPack,
                format!("clone and specialize {} actor pack", spec.template_actor),
            ),
            planned(
                format!("Model/{actor}.{actor}.bfres.mc"),
                PlanAction::CopyAsset,
                "custom render model",
            ),
            planned(
                format!("UI/Tex/Icon/{actor}.bntx.zs"),
                PlanAction::CopyAsset,
                "inventory icon",
            ),
            planned(
                format!("UI/Tex/PictureBook/{actor}_Icon.bntx.zs"),
                PlanAction::CopyAsset,
                "compendium icon",
            ),
            planned(
                format!("UI/Tex/PictureBook/{actor}_Detail.bntx.zs"),
                PlanAction::CopyAsset,
                "compendium detail image",
            ),
        ];
        files.extend(spec.assets.textures.iter().filter_map(|source| {
            source.file_name().map(|name| PlannedFile {
                relative_path: Path::new("TexToGo").join(name),
                action: PlanAction::CopyAsset,
                reason: "model texture".into(),
            })
        }));
        for product in [
            "ActorInfo",
            "AttachmentActorInfo",
            "GameActorInfo",
            "PouchActorInfo",
        ] {
            let name = version::product_name(
                &format!("RSDB/{product}.Product."),
                version,
                ".rstbl.byml.zs",
            )?;
            files.push(planned(
                name,
                PlanAction::PatchByml,
                "register custom weapon row",
            ));
        }
        files.push(planned(
            SHARP_INFO,
            PlanAction::PatchByml,
            "register custom weapon row",
        ));
        files.push(planned(
            version::product_name("RSDB/Tag.Product.", version, ".rstbl.byml.zs")?,
            PlanAction::PatchTagProduct,
            "register actor tag bitset and path",
        ));
        files.push(planned(
            version::product_name("Mals/USen.Product.", version, ".sarc.zs")?,
            PlanAction::PatchMessages,
            "add name, description, attachment, and compendium messages",
        ));
        files.push(planned(
            version::product_name(
                "System/Resource/ResourceSizeTable.Product.",
                version,
                ".rsizetable.zs",
            )?,
            PlanAction::PatchRstb,
            "add sizes for every new resource and update modified resources",
        ));
        for vendor in &spec.vendors {
            files.push(planned(
                format!("Pack/Actor/{}.pack.zs", vendor.actor_name),
                PlanAction::PatchVendorPack,
                "extend an existing travelling merchant selling list",
            ));
        }
        Ok(Self { files })
    }

    /// Creates only the output directories. It never copies or mutates game files.
    pub fn prepare_output_layout(&self, output_romfs: &Path) -> io::Result<()> {
        for file in &self.files {
            if file.relative_path.is_absolute()
                || file
                    .relative_path
                    .components()
                    .any(|part| matches!(part, std::path::Component::ParentDir))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unsafe output path: {}", file.relative_path.display()),
                ));
            }
            if let Some(parent) = output_romfs.join(&file.relative_path).parent() {
                fs::create_dir_all(parent)?;
            }
        }
        Ok(())
    }
}

fn planned(path: impl Into<PathBuf>, action: PlanAction, reason: impl Into<String>) -> PlannedFile {
    PlannedFile {
        relative_path: path.into(),
        action,
        reason: reason.into(),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DifferenceKind {
    Added,
    Modified,
    Identical,
}

/// Compares a mod tree with clean ROMFS without parsing or changing either tree.
pub fn audit_mod_tree(
    mod_romfs: &Path,
    clean_romfs: &Path,
) -> io::Result<BTreeMap<String, DifferenceKind>> {
    let mut files = Vec::new();
    collect_files(mod_romfs, mod_romfs, &mut files)?;
    let mut result = BTreeMap::new();
    for (relative, mod_path) in files {
        let clean_path = clean_romfs.join(&relative);
        let kind = if !clean_path.is_file() {
            DifferenceKind::Added
        } else if fs::read(&mod_path)? == fs::read(clean_path)? {
            DifferenceKind::Identical
        } else {
            DifferenceKind::Modified
        };
        result.insert(relative.to_string_lossy().replace('\\', "/"), kind);
    }
    Ok(result)
}

fn collect_files(
    root: &Path,
    current: &Path,
    output: &mut Vec<(PathBuf, PathBuf)>,
) -> io::Result<()> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(root, &path, output)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
                .to_path_buf();
            output.push((relative, path));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "writes the complete real Weapon_Lsword_005 integration fixture"]
    fn generates_complete_weapon_lsword_005_comparison_mod() {
        use crate::{TotkConfig::TotkConfig, Zstd::TOTK_ZSTD_COMPRESSION_LEVEL};
        use std::sync::Arc;

        let clean_romfs = Path::new("E:/TOTK_modding/0100F2C0115B6000/romfs");
        let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp");
        let restoration =
            fixture_root.join("BotW Weapon Restoration/romfs/Pack/Actor/Weapon_Lsword_005.pack.zs");
        if !clean_romfs.is_dir() || !restoration.is_file() {
            return;
        }
        let output_root = fixture_root.join("test_sic");
        let output_romfs = output_root.join("romfs");
        if output_root.is_dir() {
            fs::remove_dir_all(&output_root).expect("remove previous test_sic output");
        }
        fs::create_dir_all(&output_romfs).expect("create test_sic ROMFS");

        let mut config = TotkConfig::default();
        config.romfs = clean_romfs.to_string_lossy().into_owned();
        let zstd = Arc::new(
            crate::Zstd::TotkZstd::new(Arc::new(config), TOTK_ZSTD_COMPRESSION_LEVEL)
                .expect("load ROMFS dictionaries"),
        );
        let restored_info = actor_pack::load_weapon_actor_info(
            &fs::read(&restoration).expect("read restoration actor pack"),
            "Weapon_Lsword_005",
            zstd.clone(),
        )
        .expect("read restoration weapon values");
        let custom_png = fixture_root.join("BotW Weapon Restoration/Weapon_Lsword_002.png");
        let input_json = serde_json::json!({
            "actor_name": "Weapon_Lsword_005",
            "kind": "large_sword",
            "template_actor": "Weapon_Lsword_108",
            "model_name": "Weapon_Lsword_005",
            "weapon_parameters": {
                "base_attack": restored_info.base_attack,
                "max_life": restored_info.durability,
                "additional_damage": restored_info.attachment.additional_damage,
                "shield_bash_damage": restored_info.attachment.shield_bash_damage
            },
            "sound": {"source": "vanilla_actor", "actor_name": "Weapon_Lsword_103"},
            "effect": {"source": "vanilla_actor", "actor_name": "Weapon_Lsword_103"},
            "physics": "Weapon_Lsword_108",
            "display_name": "Spiked Boko Bat",
            "description": "After much consideration by Bokoblins on how to improve the Boko bat, they simply attached sharp spikes to it.",
            "assets": {
                "fbx": fixture_root.join("untitled.fbx"),
                "icon_png": &custom_png,
                "picture_book_icon_png": &custom_png,
                "picture_book_detail_png": &custom_png
            },
            "vendors": [
                {
                    "actor_name": "Npc_TripMaster_00",
                    "buying_price": 500,
                    "selling_price": 125,
                    "quantity": 3
                }
            ]
        });
        let input_text = serde_json::to_string_pretty(&input_json).expect("serialize input JSON");
        let spec = WeaponSpec::from_json(&input_text).expect("parse complete item-creator JSON");
        fs::write(output_root.join("items_creator_input.json"), &input_text)
            .expect("write item-creator input JSON");
        fs::write(
            fixture_root.join("test_sic_items_creator_input.json"),
            &input_text,
        )
        .expect("write persistent item-creator input JSON");

        let pack_json = serde_json::json!({
            "name": "Weapon_Lsword_005",
            "base": "Weapon_Lsword_108",
            "model_name": "Weapon_Lsword_005",
            "attack": restored_info.base_attack,
            "dur": restored_info.durability,
            "attachment_damage": restored_info.attachment.additional_damage,
            "shield_bash_damage": restored_info.attachment.shield_bash_damage,
            "sound": {"source": "vanilla_actor", "name": "Weapon_Lsword_103"},
            "effect": {"source": "vanilla_actor", "name": "Weapon_Lsword_103"},
            "physics_actor": "Weapon_Lsword_108",
            "shootable": null,
            "extra_edits": []
        });
        actor_pack::WeaponPackRequest::from_json(&pack_json.to_string())
            .expect("parse exhaustive actor-pack JSON")
            .generate_pack(
                clean_romfs,
                &output_romfs.join("Pack/Actor/Weapon_Lsword_005.pack.zs"),
                zstd.clone(),
            )
            .expect("generate actor pack");

        let model_source = fixture_root.join("works/Weapon_Lsword_005.Weapon_Lsword_005.bfres.mc");
        let model_destination =
            output_romfs.join("Model/Weapon_Lsword_005.Weapon_Lsword_005.bfres.mc");
        fs::create_dir_all(model_destination.parent().expect("model parent"))
            .expect("create model directory");
        let model_source_raw = zstd
            .decompress_mcpk(&fs::read(&model_source).expect("read model base"))
            .expect("decompress model base");
        let custom_fbx = fixture_root.join("untitled.fbx");
        let replaced_model =
            crate::file_format::Model3D::bfres::BfresFile::replace_geometry_from_fbx(
                &model_source_raw,
                &fs::read(&custom_fbx).expect("read custom FBX"),
            )
            .expect("replace BFRES geometry");
        fs::write(
            &model_destination,
            zstd.compress_mcpk(&replaced_model)
                .expect("compress custom BFRES"),
        )
        .expect("write custom BFRES");
        let generated_model_assets = assets::GeneratedWeaponAssets {
            model: model_destination,
            textures: Vec::new(),
            texture_names: Vec::new(),
        };
        let texture_output = output_romfs.join("TexToGo");
        fs::create_dir_all(&texture_output).expect("create TexToGo directory");
        for name in [
            "Weapon_Lsword_005_Alb.txtg",
            "Weapon_Lsword_005_Nrm.txtg",
            "Weapon_Lsword_005_Spm.txtg",
        ] {
            fs::copy(
                fixture_root
                    .join("BotW Weapon Restoration/romfs/TexToGo")
                    .join(name),
                texture_output.join(name),
            )
            .expect("copy Weapon_Lsword_005 TexToGo texture");
        }
        let generated_model_raw = zstd
            .decompress_mcpk(
                &fs::read(&generated_model_assets.model).expect("read generated custom model"),
            )
            .expect("decompress generated custom model");
        let generated_model =
            crate::file_format::Model3D::bfres::BfresFile::from_bytes(&generated_model_raw)
                .expect("parse generated custom model");
        let required_texture_names: std::collections::BTreeSet<String> = generated_model
            .materials
            .iter()
            .flat_map(|material| &material.texture_slots)
            .map(|slot| slot.name.clone())
            .collect();
        let mut placeholder_textures = Vec::new();
        assets::ensure_material_textures(
            &output_romfs.join("TexToGo"),
            &required_texture_names,
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("misc/placeholder_tex.txtg"),
            &mut placeholder_textures,
        )
        .expect("ensure every BFRES material texture exists");
        let imported_fbx = crate::parser::fbx::import::import_for_bfres(
            &fs::read(&custom_fbx).expect("read custom FBX"),
        )
        .expect("parse custom FBX");
        assert_eq!(generated_model.render.meshes.len(), 2);
        assert_eq!(imported_fbx.meshes.len(), 2);

        let mut asset_report = format!(
            "custom_fbx={}\nmodel={}\nmesh_count={}\nvertex_count={}\ntexture_names={}\nplaceholder_textures={}\n",
            custom_fbx.display(),
            generated_model_assets.model.display(),
            generated_model.render.meshes.len(),
            generated_model
                .render
                .meshes
                .iter()
                .map(|mesh| mesh.positions.len())
                .sum::<usize>(),
            required_texture_names.iter().cloned().collect::<Vec<_>>().join(","),
            placeholder_textures
                .iter()
                .map(|path| path.file_name().unwrap().to_string_lossy())
                .collect::<Vec<_>>()
                .join(",")
        );
        for (source, destination, name) in [
            (
                "UI/Tex/Icon/Weapon_Lsword_005.bntx.zs",
                "UI/Tex/Icon/Weapon_Lsword_005.bntx.zs",
                "Weapon_Lsword_005",
            ),
            (
                "UI/Tex/PictureBook/Weapon_Lsword_005_Icon.bntx.zs",
                "UI/Tex/PictureBook/Weapon_Lsword_005_Icon.bntx.zs",
                "Weapon_Lsword_005_Icon",
            ),
            (
                "UI/Tex/PictureBook/Weapon_Lsword_005_Detail.bntx.zs",
                "UI/Tex/PictureBook/Weapon_Lsword_005_Detail.bntx.zs",
                "Weapon_Lsword_005_Detail",
            ),
        ] {
            let replacement = assets::WeaponBntxAssetRequest::from_json(
                &serde_json::json!({
                    "texture_source": fixture_root.join("BotW Weapon Restoration/romfs").join(source),
                    "png_source": &custom_png,
                    "name": name,
                    "texture_destination": destination
                }).to_string(),
            )
            .expect("parse BNTX request")
            .generate(clean_romfs, &output_romfs, zstd.clone())
            .expect("generate BNTX asset");
            use std::fmt::Write as _;
            writeln!(
                asset_report,
                "bntx={destination} name={} format={} size={}x{} similarity={:.6}",
                replacement.name,
                replacement.format,
                replacement.width,
                replacement.height,
                replacement.similarity
            )
            .expect("write BNTX report");
        }
        fs::write(output_root.join("custom_asset_report.txt"), asset_report)
            .expect("write custom asset report");

        messages::WeaponMessageRequest::from_json(
            &serde_json::json!({
                "name": "Weapon_Lsword_005",
                "display_name": "Spiked Boko Bat",
                "description": "After much consideration by Bokoblins on how \nto improve the Boko bat, they simply attached \nsharp spikes to it. A skilled fighter can cause \nimmense damage with this.",
                "base_name": "Bat",
                "attachment_name": "Spiked-Bat",
                "picture_book_name": "Spiked Boko Bat",
                "picture_book_caption": "After much consideration by Bokoblins on how \nto improve the Boko bat, they simply attached \nsharp spikes to it. A skilled fighter can cause \nimmense damage with this."
            })
            .to_string(),
        )
        .expect("parse message JSON")
        .generate_to_mod_romfs(clean_romfs, &output_romfs, zstd.clone())
        .expect("generate MALS");

        rsdb::WeaponRsdbRequest::from_json(
            &serde_json::json!({
                "name": "Weapon_Lsword_005",
                "base": "Weapon_Lsword_108",
                "model_name": "Weapon_Lsword_005",
                "life": 34,
                "attack": restored_info.base_attack,
                "buying_price": 500,
                "selling_price": 125,
                "attachment_damage": restored_info.attachment.additional_damage,
                "shield_bash_damage": restored_info.attachment.shield_bash_damage,
                "overrides": {
                    "actor_info": {},
                    "attachment_actor_info": {},
                    "game_actor_info": {},
                    "pouch_actor_info": {}
                }
            })
            .to_string(),
        )
        .expect("parse RSDB JSON")
        .generate(clean_romfs, &output_romfs, zstd.clone())
        .expect("generate RSDB");

        sharp_info::generate_weapon_sharp_info(
            clean_romfs,
            &output_romfs,
            "Weapon_Lsword_005",
            "Weapon_Lsword_103",
            zstd.clone(),
        )
        .expect("generate SharpInfo");

        gamedata::WeaponGameDataRequest::from_json(
            r#"{"name":"Weapon_Lsword_005","picture_book":true,"inventory_flags":true}"#,
        )
        .expect("parse GameData JSON")
        .generate(clean_romfs, &output_romfs, zstd.clone())
        .expect("generate GameDataList");

        for vendor in &spec.vendors {
            vendor::VendorProcessor::new(clean_romfs, &output_romfs, zstd.clone())
                .add_weapon("Weapon_Lsword_005", vendor)
                .expect("generate vendor pack");
        }

        rstb::ModRstbProcessor::new(clean_romfs, &output_romfs, zstd)
            .generate()
            .expect("generate RSTB");
    }

    #[test]
    #[ignore = "compares tmp/test_sic with the Weapon Restoration fixture"]
    fn compares_weapon_lsword_005_generated_mod_semantics() {
        use crate::{
            file_format::{
                BinTextFile::BymlFile, Model3D::bfres::BfresFile, Pack::PackFile,
                TagProduct::TagProduct,
            },
            parser::{bntx::BntxFile, msbt::Msbt, rstb::ResourceSizeTable},
            TotkConfig::TotkConfig,
            Zstd::TOTK_ZSTD_COMPRESSION_LEVEL,
        };
        use roead::byml::Byml;
        use std::{fmt::Write as _, sync::Arc};

        let clean_romfs = Path::new("E:/TOTK_modding/0100F2C0115B6000/romfs");
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp");
        let generated = root.join("test_sic/romfs");
        let reference = root.join("BotW Weapon Restoration/romfs");
        if !generated.is_dir() || !reference.is_dir() {
            return;
        }
        let mut config = TotkConfig::default();
        config.romfs = clean_romfs.to_string_lossy().into_owned();
        let zstd = Arc::new(
            crate::Zstd::TotkZstd::new(Arc::new(config), TOTK_ZSTD_COMPRESSION_LEVEL)
                .expect("load dictionaries"),
        );
        let mut report = String::new();
        let mut errors = 0usize;

        let actor_relative = Path::new("Pack/Actor/Weapon_Lsword_005.pack.zs");
        let generated_pack = PackFile::from_binary(
            &fs::read(generated.join(actor_relative)).expect("read generated actor pack"),
            zstd.clone(),
        )
        .expect("open generated actor pack");
        let reference_pack = PackFile::from_binary(
            &fs::read(reference.join(actor_relative)).expect("read reference actor pack"),
            zstd.clone(),
        )
        .expect("open reference actor pack");
        let generated_names: BTreeMap<_, _> = generated_pack
            .sarc
            .files()
            .filter_map(|file| {
                file.name()
                    .map(|name| (name.to_owned(), file.data().to_vec()))
            })
            .collect();
        let reference_names: BTreeMap<_, _> = reference_pack
            .sarc
            .files()
            .filter_map(|file| {
                file.name()
                    .map(|name| (name.to_owned(), file.data().to_vec()))
            })
            .collect();
        writeln!(report, "ACTOR PACK").expect("write report");
        for name in generated_names
            .keys()
            .filter(|name| name.contains("Weapon_Lsword_005"))
        {
            match reference_names.get(name) {
                None => {
                    errors += 1;
                    writeln!(report, "ERROR missing reference internal path: {name}")
                        .expect("write report");
                }
                Some(reference_data) if name.ends_with("bgyml") => {
                    let generated_byml = BymlFile::from_binary(
                        generated_names.get(name).expect("generated entry"),
                        zstd.clone(),
                        name,
                    )
                    .expect("parse generated pack BYML");
                    let reference_byml = BymlFile::from_binary(reference_data, zstd.clone(), name)
                        .expect("parse reference pack BYML");
                    let mut normalized_generated = generated_byml.pio.clone();
                    let mut normalized_reference = reference_byml.pio.clone();
                    if name.starts_with("Actor/") {
                        for root in [&mut normalized_generated, &mut normalized_reference] {
                            if let Ok(root) = root.as_mut_map() {
                                if let Some(components) = root
                                    .get_mut("Components")
                                    .and_then(|value| value.as_mut_map().ok())
                                {
                                    components.remove("ChemicalRef");
                                }
                            }
                        }
                    }
                    if normalized_generated != normalized_reference {
                        errors += 1;
                        writeln!(
                            report,
                            "DIFF BYML: {name}\n  generated={}\n  reference={}",
                            generated_byml.pio.to_text(),
                            reference_byml.pio.to_text()
                        )
                        .expect("write report");
                    } else {
                        writeln!(report, "MATCH BYML: {name} (optional chemical normalized)")
                            .expect("write report");
                    }
                }
                Some(_) => {}
            }
        }

        writeln!(report, "\nRSDB").expect("write report");
        for product in [
            "ActorInfo",
            "AttachmentActorInfo",
            "GameActorInfo",
            "PouchActorInfo",
        ] {
            let name = format!("RSDB/{product}.Product.121.rstbl.byml.zs");
            let row = |base: &Path| {
                let file = BymlFile::new(base.join(&name), zstd.clone()).expect("open RSDB");
                file.pio
                    .as_array()
                    .expect("RSDB array")
                    .iter()
                    .find(|row| {
                        row.as_map()
                            .ok()
                            .and_then(|map| map.get("__RowId"))
                            .and_then(|value| value.as_string().ok())
                            .is_some_and(|value| value == "Weapon_Lsword_005")
                    })
                    .cloned()
            };
            let mut generated_row = row(&generated);
            let mut reference_row = row(&reference);
            if product == "PouchActorInfo" {
                for row in [&mut generated_row, &mut reference_row] {
                    if let Some(map) = row.as_mut().and_then(|value| value.as_mut_map().ok()) {
                        map.remove("BuyingPrice");
                        map.remove("SellingPrice");
                    }
                }
            }
            match (generated_row, reference_row) {
                (Some(left), Some(right)) if left == right => {
                    writeln!(report, "MATCH ROW: {product}").expect("write report")
                }
                (Some(_), Some(_)) => {
                    errors += 1;
                    let mut left = row(&generated).expect("generated row");
                    if product == "PouchActorInfo" {
                        if let Ok(map) = left.as_mut_map() {
                            map.remove("BuyingPrice");
                            map.remove("SellingPrice");
                        }
                    }
                    let right = row(&reference).expect("reference row");
                    writeln!(
                        report,
                        "DIFF ROW: {product}\n  generated={}\n  reference={}",
                        left.to_text(),
                        right.to_text()
                    )
                    .expect("write report")
                }
                values => {
                    errors += 1;
                    writeln!(report, "ERROR ROW: {product}: {values:?}").expect("write report")
                }
            }
        }

        let sharp_row = |base: &Path| {
            BymlFile::new(base.join(SHARP_INFO), zstd.clone()).and_then(|file| {
                file.pio
                    .as_map()
                    .ok()
                    .and_then(|root| root.get("SharpInfoList"))
                    .and_then(|rows| rows.as_array().ok())
                    .and_then(|rows| {
                        rows.iter()
                            .find(|row| {
                                row.as_map()
                                    .ok()
                                    .and_then(|row| row.get("ActorName"))
                                    .and_then(|value| value.as_string().ok())
                                    .is_some_and(|value| value == "Weapon_Lsword_005")
                            })
                            .cloned()
                    })
            })
        };
        match (sharp_row(&generated), sharp_row(&reference)) {
            (Some(left), Some(right)) if left == right => {
                writeln!(report, "MATCH SharpInfo row").expect("write report")
            }
            values => {
                errors += 1;
                writeln!(report, "DIFF SharpInfo row: {values:?}").expect("write report")
            }
        }

        let tag_name = Path::new("RSDB/Tag.Product.121.rstbl.byml.zs");
        let read_tag = |base: &Path| {
            let bytes = fs::read(base.join(tag_name)).expect("read Tag.Product");
            TagProduct::from_binary(&bytes, tag_name, zstd.clone())
        };
        let generated_tag = read_tag(&generated).expect("parse generated Tag.Product");
        let reference_tag = read_tag(&reference);
        let actor_tag_path = "Work/Actor/|Weapon_Lsword_005|.engine__actor__ActorParam.gyml";
        if generated_tag.actor_tag_data.contains_key(actor_tag_path) {
            writeln!(
                report,
                "VALID TAG PRODUCT entry: generated={:?}, reference={:?}",
                generated_tag.actor_tag_data.get(actor_tag_path),
                reference_tag
                    .as_ref()
                    .and_then(|tag| tag.actor_tag_data.get(actor_tag_path))
            )
            .expect("write report");
            if reference_tag.is_none() {
                writeln!(report, "REFERENCE WARNING Tag.Product does not parse")
                    .expect("write report");
            }
        } else {
            errors += 1;
            writeln!(report, "ERROR generated TAG PRODUCT entry is missing").expect("write report");
        }

        writeln!(report, "\nMALS/MSBT").expect("write report");
        let mals_name = Path::new("Mals/USen.Product.121.sarc.zs");
        let open_mals = |base: &Path| {
            PackFile::from_binary(
                &fs::read(base.join(mals_name)).expect("read MALS"),
                zstd.clone(),
            )
            .expect("open MALS")
        };
        let generated_mals = open_mals(&generated);
        let reference_mals = open_mals(&reference);
        for file in generated_mals.sarc.files() {
            let Some(name) = file.name() else { continue };
            if !name.ends_with(".msbt") {
                continue;
            }
            let generated_msbt = Msbt::from_bytes(file.data()).expect("parse generated MSBT");
            let Some(reference_data) = reference_mals.sarc.get_data(name) else {
                continue;
            };
            let reference_msbt = Msbt::from_bytes(reference_data).expect("parse reference MSBT");
            for message in generated_msbt.messages.iter().filter(|message| {
                message
                    .label
                    .as_deref()
                    .is_some_and(|label| label.contains("Weapon_Lsword_005"))
            }) {
                let matching = reference_msbt
                    .messages
                    .iter()
                    .find(|candidate| candidate.label == message.label);
                if matching == Some(message) {
                    writeln!(
                        report,
                        "MATCH MESSAGE: {name}:{}",
                        message.label.as_deref().unwrap_or("")
                    )
                    .expect("write report");
                } else {
                    errors += 1;
                    writeln!(
                        report,
                        "DIFF MESSAGE: {name}:{}\n  generated={:?}\n  reference={:?}",
                        message.label.as_deref().unwrap_or(""),
                        message.parts,
                        matching.map(|value| &value.parts)
                    )
                    .expect("write report");
                }
            }
        }

        writeln!(report, "\nBFRES").expect("write report");
        let model_name = Path::new("Model/Weapon_Lsword_005.Weapon_Lsword_005.bfres.mc");
        let parse_model = |base: &Path| {
            let compressed = fs::read(base.join(model_name)).expect("read model");
            let raw = zstd.decompress_mcpk(&compressed).expect("decompress model");
            BfresFile::from_bytes(&raw).expect("parse model")
        };
        let generated_model = parse_model(&generated);
        let reference_model = parse_model(&reference);
        writeln!(
            report,
            "generated name={:?}, reference name={:?}",
            generated_model.name, reference_model.name
        )
        .expect("write report");
        if generated_model.name != reference_model.name {
            errors += 1;
        }
        let generated_slots: Vec<_> = generated_model
            .materials
            .iter()
            .flat_map(|material| material.texture_slots.iter().map(|slot| slot.name.clone()))
            .collect();
        if generated_slots
            .iter()
            .any(|name| name.contains("Weapon_Sword_022"))
        {
            errors += 1;
            writeln!(report, "ERROR BFRES retains base texture names").expect("write report");
        }
        writeln!(
            report,
            "generated meshes={}, reference meshes={}",
            generated_model.render.meshes.len(),
            reference_model.render.meshes.len()
        )
        .expect("write report");

        writeln!(report, "\nBNTX").expect("write report");
        for relative in [
            "UI/Tex/Icon/Weapon_Lsword_005.bntx.zs",
            "UI/Tex/PictureBook/Weapon_Lsword_005_Icon.bntx.zs",
            "UI/Tex/PictureBook/Weapon_Lsword_005_Detail.bntx.zs",
        ] {
            if !generated.join(relative).is_file() {
                writeln!(report, "SKIPPED BNTX (not modified): {relative}").expect("write report");
                continue;
            }
            let parse = |base: &Path| {
                let compressed = fs::read(base.join(relative)).expect("read BNTX");
                let (raw, _) = zstd
                    .try_decompress_for_path(Path::new(relative), &compressed)
                    .expect("decompress BNTX");
                BntxFile::parse(&raw).expect("parse BNTX")
            };
            let left = parse(&generated);
            let right = parse(&reference);
            let left_texture = left.textures.first().expect("generated texture");
            let right_texture = right.textures.first().expect("reference texture");
            let expected_name = Path::new(relative)
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".bntx.zs"))
                .expect("BNTX expected name");
            if left_texture.name != expected_name
                || left_texture.format != right_texture.format
                || left_texture.width != right_texture.width
                || left_texture.height != right_texture.height
            {
                errors += 1;
                writeln!(report, "DIFF BNTX: {relative}: generated={left_texture:?}, reference={right_texture:?}").expect("write report");
            } else {
                writeln!(report, "MATCH BNTX metadata: {relative}").expect("write report");
                if right_texture.name != expected_name {
                    writeln!(
                        report,
                        "REFERENCE WARNING stale internal BNTX name: {}",
                        right_texture.name
                    )
                    .expect("write report");
                }
            }
        }

        writeln!(report, "\nGLOBAL OUTPUTS").expect("write report");
        let game_data = generated.join("GameData/GameDataList.Product.110.byml.zs");
        if BymlFile::new(&game_data, zstd.clone()).is_some() {
            writeln!(report, "VALID GameDataList reopens after hash insertion")
                .expect("write report");
        } else {
            errors += 1;
            writeln!(report, "ERROR GameDataList does not reopen").expect("write report");
        }
        let generated_game_data =
            BymlFile::new(&game_data, zstd.clone()).expect("open generated GameDataList");
        let reference_game_data = BymlFile::new(
            reference.join("GameData/GameDataList.Product.110.byml.zs"),
            zstd.clone(),
        )
        .expect("open reference GameDataList");
        let game_data_entry = |root: &Byml, hash: u32| {
            root.as_map()
                .ok()
                .and_then(|root| root.get("Data"))
                .and_then(|data| data.as_map().ok())
                .and_then(|data| {
                    ["Bool", "Enum", "Struct"].iter().find_map(|kind| {
                        data.get(*kind)
                            .and_then(|entries| entries.as_array().ok())
                            .and_then(|entries| {
                                entries.iter().find(|entry| {
                                    entry.as_map().ok().is_some_and(|entry| {
                                        entry.get("Hash") == Some(&Byml::U32(hash))
                                    })
                                })
                            })
                    })
                })
                .cloned()
        };
        for name in [
            "IsGet.Weapon_Lsword_005",
            "IsGetAnyway.Weapon_Lsword_005",
            "PictureBookData.Weapon_Lsword_005",
            "PictureBookData.Weapon_Lsword_005.IsNew",
            "PictureBookData.Weapon_Lsword_005.State",
        ] {
            let hash = gamedata::murmur3_hash(name);
            let left = game_data_entry(&generated_game_data.pio, hash);
            let right = game_data_entry(&reference_game_data.pio, hash);
            if left == right {
                writeln!(report, "MATCH GameDataList: {name} ({hash:#010X})")
                    .expect("write report");
            } else {
                errors += 1;
                writeln!(
                    report,
                    "DIFF GameDataList: {name} ({hash:#010X})\n  generated={left:?}\n  reference={right:?}"
                )
                .expect("write report");
            }
        }

        let generated_bootup_path = generated.join(ecocat::BOOTUP_PACK);
        if generated_bootup_path.is_file() {
            let generated_bootup = PackFile::from_binary(
                &fs::read(&generated_bootup_path).expect("read generated Bootup"),
                zstd.clone(),
            )
            .expect("open generated Bootup");
            let reference_bootup = PackFile::from_binary(
                &fs::read(reference.join(ecocat::BOOTUP_PACK)).expect("read reference Bootup"),
                zstd.clone(),
            )
            .expect("open reference Bootup");
            for path in [
                "Ecosystem/Ground.ecocat.byml",
                "Ecosystem/MinusField.ecocat.byml",
            ] {
                let generated_count = generated_bootup
                    .byml_file(path)
                    .expect("parse generated ecocat")
                    .pio
                    .to_text()
                    .matches("Weapon_Lsword_005")
                    .count();
                let reference_count = reference_bootup
                    .byml_file(path)
                    .expect("parse reference ecocat")
                    .pio
                    .to_text()
                    .matches("Weapon_Lsword_005")
                    .count();
                writeln!(report, "VALID {path}: generated occurrences={generated_count}, reference occurrences={reference_count}")
                    .expect("write report");
            }
        } else {
            writeln!(
                report,
                "SKIPPED Ecocat: generated mod does not modify Bootup"
            )
            .expect("write report");
        }

        let vendor_pack = PackFile::from_binary(
            &fs::read(generated.join("Pack/Actor/Npc_TripMaster_00.pack.zs"))
                .expect("read generated vendor"),
            zstd.clone(),
        )
        .expect("open generated vendor");
        let vendor_contains_weapon = vendor_pack.sarc.files().any(|file| {
            file.name().is_some_and(|name| name.contains("ShopParam"))
                && BymlFile::from_binary(file.data(), zstd.clone(), "vendor-shop")
                    .is_ok_and(|file| file.pio.to_text().contains("Weapon_Lsword_005"))
        });
        if vendor_contains_weapon {
            writeln!(report, "VALID vendor ShopParam contains Weapon_Lsword_005")
                .expect("write report");
        } else {
            errors += 1;
            writeln!(
                report,
                "ERROR vendor ShopParam is missing Weapon_Lsword_005"
            )
            .expect("write report");
        }

        let rstb_path =
            generated.join("System/Resource/ResourceSizeTable.Product.121.rsizetable.zs");
        let (rstb_raw, _) = zstd
            .try_decompress_for_path(&rstb_path, &fs::read(&rstb_path).expect("read RSTB"))
            .expect("decompress RSTB");
        let rstb = ResourceSizeTable::from_bytes(&rstb_raw).expect("parse RSTB");
        for path in [
            "Pack/Actor/Weapon_Lsword_005.pack",
            "Model/Weapon_Lsword_005.Weapon_Lsword_005.bfres",
        ] {
            if rstb.get(path.to_owned()).is_some() {
                writeln!(report, "VALID RSTB entry: {path}").expect("write report");
            } else {
                errors += 1;
                writeln!(report, "ERROR missing RSTB entry: {path}").expect("write report");
            }
        }

        writeln!(report, "\nTOTAL ERRORS: {errors}").expect("write report");
        fs::write(root.join("test_sic/comparison_report.txt"), report).expect("save report");
    }

    #[test]
    #[ignore = "diagnostic vendor pack inspection"]
    fn inspect_vendor_pack_entries() {
        use crate::{
            file_format::Pack::PackFile,
            TotkConfig::TotkConfig,
            Zstd::{TotkZstd, TOTK_ZSTD_COMPRESSION_LEVEL},
        };
        use roead::byml::Byml;
        use std::sync::Arc;

        let romfs = Path::new("E:/TOTK_modding/0100F2C0115B6000/romfs");
        let mut config = TotkConfig::default();
        config.romfs = romfs.to_string_lossy().into_owned();
        let zstd = Arc::new(TotkZstd::new(Arc::new(config), TOTK_ZSTD_COMPRESSION_LEVEL).unwrap());
        for (label, path) in [
            (
                "modified trip",
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../tmp/BotW Weapon Restoration/Npc_TripMaster_00.pack.zs"),
            ),
            (
                "clean trip",
                romfs.join("Pack/Actor/Npc_TripMaster_00.pack.zs"),
            ),
            (
                "restoration vendor",
                Path::new(env!("CARGO_MANIFEST_DIR")).join(
                    "../tmp/BotW Weapon Restoration/romfs/Pack/Actor/Npc_WeaponVendor001.pack.zs",
                ),
            ),
        ] {
            if !path.is_file() {
                continue;
            }
            let pack = PackFile::from_binary(&fs::read(&path).unwrap(), zstd.clone()).unwrap();
            let _ = label;
            for file in pack.sarc.files() {
                let Some(name) = file.name() else {
                    continue;
                };
                if name.contains("Shop") || name.contains("ActorParam") {
                    if name.ends_with(".bgyml") {
                        if let Ok(value) = Byml::from_binary(file.data()) {
                            let _ = value;
                        }
                    }
                }
            }
        }
        for product in ["ActorInfo", "GameActorInfo", "PouchActorInfo"] {
            for (label, root) in [
                (
                    "restoration",
                    Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../tmp/BotW Weapon Restoration/romfs/RSDB"),
                ),
                ("clean", romfs.join("RSDB")),
            ] {
                let path = root.join(format!("{product}.Product.121.rstbl.byml.zs"));
                let raw = zstd.decompress_zs(&fs::read(path).unwrap()).unwrap();
                let rows = Byml::from_binary(&raw).unwrap();
                for actor in [
                    "Weapon_Lsword_005",
                    "Weapon_Lsword_108",
                    "Item_Weapon_032",
                    "Weapon_Sword_042",
                    "Weapon_Lsword_032",
                ] {
                    if let Some(row) = rows.as_array().unwrap().iter().find(|row| {
                        row.as_map()
                            .ok()
                            .and_then(|map| map.get("__RowId"))
                            .and_then(|value| value.as_string().ok())
                            .is_some_and(|value| value.as_str() == actor)
                    }) {
                        let _ = (label, product, actor, row);
                    }
                }
            }
        }
        for path in [
            romfs.join("Pack/Actor/Weapon_Lsword_108.pack.zs"),
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tmp/BotW Weapon Restoration/romfs/Pack/Actor/Weapon_Lsword_005.pack.zs"),
        ] {
            let pack = PackFile::from_binary(&fs::read(&path).unwrap(), zstd.clone()).unwrap();
            for file in pack.sarc.files() {
                let Some(name) = file.name() else {
                    continue;
                };
                let Ok(value) = Byml::from_binary(file.data()) else {
                    continue;
                };
                let text = value.to_text();
                if text.contains("Price") {
                    let _ = (name, text);
                }
            }
        }
    }

    #[test]
    #[ignore = "diagnostic SharpInfo row inspection"]
    fn inspect_weapon_sharp_info_rows() {
        use crate::{
            file_format::BinTextFile::BymlFile,
            TotkConfig::TotkConfig,
            Zstd::{TotkZstd, TOTK_ZSTD_COMPRESSION_LEVEL},
        };
        use roead::byml::Byml;
        use std::sync::Arc;

        fn find_actor(value: &Byml, actor: &str, output: &mut Vec<Byml>) {
            match value {
                Byml::Map(map) => {
                    if map
                        .values()
                        .any(|value| value.as_string().is_ok_and(|value| value.as_str() == actor))
                    {
                        output.push(value.clone());
                    }
                    for value in map.values() {
                        find_actor(value, actor, output);
                    }
                }
                Byml::Array(values) => {
                    for value in values {
                        find_actor(value, actor, output);
                    }
                }
                _ => {}
            }
        }

        let clean_romfs = Path::new("E:/TOTK_modding/0100F2C0115B6000/romfs");
        let mut config = TotkConfig::default();
        config.romfs = clean_romfs.to_string_lossy().into_owned();
        let zstd = Arc::new(TotkZstd::new(Arc::new(config), TOTK_ZSTD_COMPRESSION_LEVEL).unwrap());
        for (label, path, actor) in [
            ("clean", clean_romfs.join(SHARP_INFO), "Weapon_Lsword_108"),
            (
                "reference",
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../tmp/BotW Weapon Restoration/romfs")
                    .join(SHARP_INFO),
                "Weapon_Lsword_005",
            ),
        ] {
            let file = BymlFile::new(path, zstd.clone()).unwrap();
            if let Ok(map) = file.pio.as_map() {
                println!("SHARP ROOT KEYS: {:?}", map.keys().collect::<Vec<_>>());
            }
            let mut rows = Vec::new();
            find_actor(&file.pio, actor, &mut rows);
            println!("SHARP {label} {actor}: {}", rows.len());
            for row in rows {
                println!("{}", row.to_text());
            }
        }
    }

    fn spec() -> WeaponSpec {
        WeaponSpec {
            actor_name: "Weapon_Lsword_900".into(),
            kind: WeaponKind::LargeSword,
            template_actor: "Weapon_Lsword_060".into(),
            model_name: String::new(),
            weapon_parameters: actor_pack::WeaponParameterOverrides::default(),
            actor_pack: actor_pack::ActorPackPolicy::default(),
            sound: None,
            effect: None,
            physics: None,
            chemical: None,
            shootable: None,
            display_name: "Test Sword".into(),
            description: "A test weapon".into(),
            assets: WeaponAssets {
                fbx: Some("model.fbx".into()),
                textures: vec!["blade_Alb.txtg".into()],
                icon_png: Some("icon.png".into()),
                picture_book_icon_png: Some("book_icon.png".into()),
                picture_book_detail_png: Some("book_detail.png".into()),
            },
            vendors: vec![VendorTarget {
                actor_name: "Npc_TripMaster_00".into(),
                buying_price: None,
                selling_price: None,
                quantity: 1,
            }],
        }
    }

    #[test]
    fn plan_contains_core_registration_files() {
        let plan = GenerationPlan::for_weapon_version(&spec(), "112").unwrap();
        for path in [
            "RSDB/ActorInfo.Product.112.rstbl.byml.zs",
            "RSDB/PouchActorInfo.Product.112.rstbl.byml.zs",
            "RSDB/Tag.Product.112.rstbl.byml.zs",
            "Mals/USen.Product.112.sarc.zs",
            "System/Resource/ResourceSizeTable.Product.112.rsizetable.zs",
        ] {
            assert!(plan
                .files
                .iter()
                .any(|file| file.relative_path == Path::new(path)));
        }
        assert!(plan.files.iter().any(|file| {
            file.relative_path == Path::new("Pack/Actor/Npc_TripMaster_00.pack.zs")
                && file.action == PlanAction::PatchVendorPack
        }));
    }

    #[test]
    fn rejects_new_vendor_actor_names() {
        let mut value = spec();
        value.vendors[0].actor_name = "Npc_CustomVendor".into();
        assert!(value.validate(Path::new("missing-assets")).is_err());
    }
}
