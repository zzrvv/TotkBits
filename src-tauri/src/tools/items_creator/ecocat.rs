//! Optional enemy weapon distribution editing in the Bootup ecosystem catalogs.

use crate::{
    file_format::{BinTextFile::BymlFile, Pack::PackFile},
    Zstd::TotkZstd,
};
use roead::byml::Byml;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path, sync::Arc};

pub const BOOTUP_PACK: &str = "Pack/Bootup.Nin_NX_NVN.pack.zs";
const GROUND: &str = "Ecosystem/Ground.ecocat.byml";
const MINUS_FIELD: &str = "Ecosystem/MinusField.ecocat.byml";

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct WeaponEcocatPlacement {
    /// Integer probability written to the ecosystem weapon pool (0 through 100).
    pub chance_percent: u8,
    /// Only edit these enemy actor names. Empty means every enemy with a weapon pool.
    #[serde(default)]
    pub enemy_names: Vec<String>,
    /// Enemy actors to omit after applying `enemy_names`.
    #[serde(default)]
    pub exclude_enemy_names: Vec<String>,
    /// Only edit ecosystem areas with one of these AreaNumber values.
    #[serde(default)]
    pub area_numbers: Vec<i32>,
    /// Only edit areas whose Area path contains one of these strings.
    #[serde(default)]
    pub area_path_contains: Vec<String>,
    /// Also add the weapon to the area's unweighted `Weapon` discovery pool.
    #[serde(default)]
    pub include_area_weapon_pool: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct WeaponEcocatRequest {
    #[serde(alias = "name")]
    pub actor_name: String,
    /// Omit to leave Ground.ecocat.byml byte-for-byte unchanged.
    #[serde(default)]
    pub ground: Option<WeaponEcocatPlacement>,
    /// Omit to leave MinusField.ecocat.byml byte-for-byte unchanged.
    #[serde(default, alias = "minusfield")]
    pub minus_field: Option<WeaponEcocatPlacement>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeaponEcocatReport {
    pub environments_modified: usize,
    pub areas_modified: usize,
    pub enemy_pools_modified: usize,
    pub area_weapon_pools_modified: usize,
}

impl WeaponEcocatRequest {
    pub fn from_json(text: &str) -> io::Result<Self> {
        serde_json::from_str(text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    /// Clone the clean Bootup pack, edit selected ecosystem entries, and save under mod ROMFS.
    pub fn generate(
        &self,
        clean_romfs: &Path,
        output_romfs: &Path,
        zstd: Arc<TotkZstd<'_>>,
    ) -> io::Result<WeaponEcocatReport> {
        validate_actor_name(&self.actor_name)?;
        ensure_output_outside_romfs(clean_romfs, output_romfs)?;
        if self.ground.is_none() && self.minus_field.is_none() {
            return Ok(WeaponEcocatReport::default());
        }

        let source = clean_romfs.join(BOOTUP_PACK);
        let compressed = fs::read(&source)?;
        let pack = PackFile::from_binary(&compressed, zstd.clone())?;
        let mut replacements = std::collections::BTreeMap::new();
        let mut report = WeaponEcocatReport::default();

        for (path, placement) in [(GROUND, &self.ground), (MINUS_FIELD, &self.minus_field)] {
            let Some(placement) = placement else {
                continue;
            };
            validate_placement(placement)?;
            let source_byml = pack
                .sarc
                .get_data(path)
                .ok_or_else(|| invalid_data(format!("Bootup entry is missing: {path}")))?;
            let (rebuilt, edit) =
                edit_ecocat(source_byml, path, &self.actor_name, placement, zstd.clone())?;
            report.environments_modified +=
                usize::from(edit.enemy_pools_modified > 0 || edit.area_weapon_pools_modified > 0);
            report.areas_modified += edit.areas_modified;
            report.enemy_pools_modified += edit.enemy_pools_modified;
            report.area_weapon_pools_modified += edit.area_weapon_pools_modified;
            replacements.insert(path.to_owned(), rebuilt);
        }

        let entries = pack.sarc.files().map(|file| {
            let name = file.name().unwrap_or_default().to_owned();
            let data = replacements
                .remove(&name)
                .unwrap_or_else(|| file.data().to_vec());
            (name, data)
        });
        let rebuilt = pack.rebuild_binary(entries)?;
        let destination = output_romfs.join(BOOTUP_PACK);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(destination, rebuilt)?;
        Ok(report)
    }
}

#[derive(Default)]
struct EcocatEditReport {
    areas_modified: usize,
    enemy_pools_modified: usize,
    area_weapon_pools_modified: usize,
}

fn edit_ecocat(
    data: &[u8],
    path: &str,
    actor_name: &str,
    placement: &WeaponEcocatPlacement,
    zstd: Arc<TotkZstd<'_>>,
) -> io::Result<(Vec<u8>, EcocatEditReport)> {
    let mut file = BymlFile::from_binary(data, zstd, path)?;
    let root = &mut file.pio;
    let areas = root
        .as_mut_array()
        .map_err(|_| invalid_data("ecocat BYML root is not an array"))?;
    let mut report = EcocatEditReport::default();
    for area in areas {
        let map = area
            .as_mut_map()
            .map_err(|_| invalid_data("ecocat area is not a map"))?;
        if !area_matches(map, placement) {
            continue;
        }
        let mut area_changed = false;
        if let Some(Byml::Array(enemies)) = map.get_mut("Enemy") {
            for enemy in enemies {
                let Ok(enemy) = enemy.as_mut_map() else {
                    continue;
                };
                let Some(Byml::String(enemy_name)) = enemy.get("name") else {
                    continue;
                };
                if !enemy_matches(enemy_name, placement) {
                    continue;
                }
                let Some(Byml::Array(weapons)) = enemy.get_mut("weapons") else {
                    continue;
                };
                upsert_weighted_weapon(weapons, actor_name, placement.chance_percent);
                report.enemy_pools_modified += 1;
                area_changed = true;
            }
        }
        if placement.include_area_weapon_pool {
            if let Some(Byml::Array(weapons)) = map.get_mut("Weapon") {
                upsert_unweighted_weapon(weapons, actor_name);
                report.area_weapon_pools_modified += 1;
                area_changed = true;
            }
        }
        report.areas_modified += usize::from(area_changed);
    }
    let rebuilt = file.to_binary_preserving_header()?;
    Ok((rebuilt, report))
}

fn area_matches(area: &roead::byml::Map, placement: &WeaponEcocatPlacement) -> bool {
    let number_matches = placement.area_numbers.is_empty()
        || area
            .get("AreaNumber")
            .and_then(byml_i32)
            .is_some_and(|number| placement.area_numbers.contains(&number));
    let path_matches = placement.area_path_contains.is_empty()
        || area
            .get("Area")
            .and_then(|value| match value {
                Byml::String(path) => Some(path.as_str()),
                _ => None,
            })
            .is_some_and(|path| {
                placement
                    .area_path_contains
                    .iter()
                    .any(|needle| path.contains(needle))
            });
    number_matches && path_matches
}

fn enemy_matches(name: &str, placement: &WeaponEcocatPlacement) -> bool {
    (placement.enemy_names.is_empty() || placement.enemy_names.iter().any(|value| value == name))
        && !placement
            .exclude_enemy_names
            .iter()
            .any(|value| value == name)
}

fn upsert_weighted_weapon(weapons: &mut Vec<Byml>, actor_name: &str, chance: u8) {
    if let Some(existing) = weapons.iter_mut().find(|weapon| {
        weapon.as_map().is_ok_and(
            |map| matches!(map.get("name"), Some(Byml::String(name)) if name == actor_name),
        )
    }) {
        if let Ok(map) = existing.as_mut_map() {
            map.insert("prob".into(), Byml::I32(i32::from(chance)));
        }
        return;
    }
    let mut weapon = roead::byml::Map::default();
    weapon.insert("name".into(), Byml::String(actor_name.into()));
    weapon.insert("prob".into(), Byml::I32(i32::from(chance)));
    weapons.push(Byml::Map(weapon));
}

fn upsert_unweighted_weapon(weapons: &mut Vec<Byml>, actor_name: &str) {
    if weapons.iter().any(|weapon| {
        weapon.as_map().is_ok_and(
            |map| matches!(map.get("name"), Some(Byml::String(name)) if name == actor_name),
        )
    }) {
        return;
    }
    let mut weapon = roead::byml::Map::default();
    weapon.insert("name".into(), Byml::String(actor_name.into()));
    weapons.push(Byml::Map(weapon));
}

fn byml_i32(value: &Byml) -> Option<i32> {
    match value {
        Byml::I32(value) => Some(*value),
        Byml::U32(value) => (*value).try_into().ok(),
        _ => None,
    }
}

fn validate_placement(placement: &WeaponEcocatPlacement) -> io::Result<()> {
    if placement.chance_percent > 100 {
        return Err(invalid("chance_percent must be between 0 and 100"));
    }
    if placement
        .area_path_contains
        .iter()
        .any(|value| value.is_empty())
    {
        return Err(invalid("area_path_contains values must not be empty"));
    }
    Ok(())
}

fn validate_actor_name(actor: &str) -> io::Result<()> {
    if actor.is_empty()
        || !actor
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
    {
        return Err(invalid(format!("invalid actor name: {actor}")));
    }
    Ok(())
}

fn ensure_output_outside_romfs(clean_romfs: &Path, output: &Path) -> io::Result<()> {
    let clean = clean_romfs.canonicalize()?;
    let absolute = if output.is_absolute() {
        output.to_path_buf()
    } else {
        std::env::current_dir()?.join(output)
    };
    let output = super::assets::resolve_with_existing_ancestor(&absolute)?;
    if output == clean || output.starts_with(clean) {
        return Err(invalid("output Bootup pack must be outside clean ROMFS"));
    }
    Ok(())
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
    use crate::TotkConfig::TotkConfig;

    fn weighted(name: &str, chance: i32) -> Byml {
        let mut map = roead::byml::Map::default();
        map.insert("name".into(), Byml::String(name.into()));
        map.insert("prob".into(), Byml::I32(chance));
        Byml::Map(map)
    }

    #[test]
    fn parses_independent_ground_and_depths_options() {
        let request = WeaponEcocatRequest::from_json(
            r#"{
                "name":"Weapon_Lsword_900",
                "ground":{"chance_percent":15,"enemy_names":["Enemy_Bokoblin_Junior"]},
                "minusfield":{"chance_percent":40,"area_numbers":[3,12],"include_area_weapon_pool":true}
            }"#,
        )
        .unwrap();
        assert_eq!(request.ground.unwrap().chance_percent, 15);
        assert_eq!(request.minus_field.unwrap().area_numbers, [3, 12]);
    }

    #[test]
    fn weighted_weapon_upsert_is_idempotent() {
        let mut weapons = vec![weighted("Weapon_Sword_001", 90)];
        upsert_weighted_weapon(&mut weapons, "Weapon_Lsword_900", 10);
        upsert_weighted_weapon(&mut weapons, "Weapon_Lsword_900", 25);
        assert_eq!(weapons.len(), 2);
        let custom = weapons[1].as_map().unwrap();
        assert!(matches!(custom.get("prob"), Some(Byml::I32(25))));
    }

    #[test]
    #[ignore = "uses the inspected Weapon Restoration Bootup fixture"]
    fn edits_real_ground_ecocat_and_reopens_it() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/bootup_restoration/Ecosystem/Ground.ecocat.byml");
        if !path.is_file() {
            return;
        }
        let placement = WeaponEcocatPlacement {
            chance_percent: 37,
            enemy_names: vec!["Enemy_Lizalfos_Bone_Junior".into()],
            ..Default::default()
        };
        let zstd = Arc::new(TotkZstd::dictionaryless(
            Arc::new(TotkConfig::default()),
            crate::Zstd::TOTK_ZSTD_COMPRESSION_LEVEL,
        ));
        let (rebuilt, report) = edit_ecocat(
            &fs::read(&path).unwrap(),
            path.to_str().unwrap(),
            "Weapon_Lsword_900",
            &placement,
            zstd,
        )
        .unwrap();
        assert!(report.enemy_pools_modified > 0);
        Byml::from_binary(&rebuilt).unwrap();
    }
}
