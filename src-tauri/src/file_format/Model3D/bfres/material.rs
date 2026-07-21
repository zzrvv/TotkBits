use super::{read_string, u64_at, BfresSection, Endian};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BfresTextureSlot {
    pub index: usize,
    pub name: String,
    pub texture_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BfresMaterial {
    pub name: String,
    pub offset: u64,
    pub texture_slots: Vec<BfresTextureSlot>,
}

pub fn parse_materials(
    data: &[u8],
    sections: &[BfresSection],
    endian: Endian,
    version_major: u8,
) -> Vec<BfresMaterial> {
    sections
        .iter()
        .filter(|section| &section.signature == b"FMAT")
        .map(|section| {
            let offset = section.offset as usize;
            let (names_pointer_offset, count_offset) = if version_major >= 10 {
                (32, 163)
            } else {
                (48, 179)
            };
            let names = u64_at(data, offset + names_pointer_offset, endian).unwrap_or(0) as usize;
            let count = data.get(offset + count_offset).copied().unwrap_or(0) as usize;
            let texture_slots = (0..count)
                .filter_map(|index| {
                    let name = read_string(data, u64_at(data, names + index * 8, endian).ok()?)?;
                    Some(BfresTextureSlot {
                        index,
                        texture_type: classify_texture(&name).into(),
                        name,
                    })
                })
                .collect();
            BfresMaterial {
                name: section
                    .name
                    .clone()
                    .unwrap_or_else(|| "Unnamed material".into()),
                offset: section.offset,
                texture_slots,
            }
        })
        .collect()
}

fn classify_texture(name: &str) -> &'static str {
    let value = name.to_ascii_lowercase();
    if value.contains("normal") || value.contains("_nrm") || value.ends_with("_n") {
        "Normal"
    } else if value.contains("rough") || value.contains("metal") || value.contains("mra") {
        "Material parameters"
    } else if value.contains("emit") || value.contains("emiss") {
        "Emission"
    } else if value.contains("spec") {
        "Specular"
    } else if value.contains("mask") || value.contains("alpha") {
        "Mask"
    } else if value.contains("alb") || value.contains("diff") || value.contains("base") {
        "Base color"
    } else {
        "Texture"
    }
}
