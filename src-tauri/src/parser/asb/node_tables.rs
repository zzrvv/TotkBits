use super::parameter::{read_parameter, ParameterType};
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::io;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct X38Entry {
    #[serde(rename = "Type")]
    pub kind: u32,
    #[serde(rename = "GUID")]
    pub guid: String,
    #[serde(rename = "Entry")]
    pub entry: Value,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct X40Entry {
    #[serde(rename = "Unknown 1")]
    pub unknown_1: u32,
    #[serde(rename = "Angle")]
    pub angle: f64,
    #[serde(rename = "Type", skip_serializing_if = "Option::is_none")]
    pub kind: Option<u32>,
    #[serde(rename = "Unknown 2")]
    pub unknown_2: f64,
    #[serde(rename = "Rate")]
    pub rate: f64,
    #[serde(rename = "Unknown 3")]
    pub unknown_3: f64,
    #[serde(rename = "Min")]
    pub min: f64,
    #[serde(rename = "Max")]
    pub max: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BoneGroup {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Unknown")]
    pub unknown: u32,
    #[serde(rename = "Bones")]
    pub bones: Vec<Bone>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bone {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "Unknown")]
    pub unknown: f64,
}
pub fn guid(r: &mut BinaryReader<'_>) -> io::Result<String> {
    Ok(format!(
        "{:x}-{:x}-{:x}-{:x}-{}",
        r.read_u32()?,
        r.read_u16()?,
        r.read_u16()?,
        r.read_u16()?,
        r.read_bytes(6)?
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ))
}
pub fn read_x40(
    r: &mut BinaryReader<'_>,
    offset: u32,
    count: u32,
    version: u32,
) -> io::Result<Vec<X40Entry>> {
    r.seek(offset as usize)?;
    let mut out = Vec::new();
    for _ in 0..count {
        out.push(X40Entry {
            unknown_1: r.read_u32()?,
            angle: f64::from(r.read_f32()?),
            kind: if version == 0x417 {
                Some(r.read_u32()?)
            } else {
                None
            },
            unknown_2: f64::from(r.read_f32()?),
            rate: f64::from(r.read_f32()?),
            unknown_3: f64::from(r.read_f32()?),
            min: f64::from(r.read_f32()?),
            max: f64::from(r.read_f32()?),
        });
    }
    Ok(out)
}
pub fn read_x38(
    r: &mut BinaryReader<'_>,
    p: &BinaryReader<'_>,
    offset: u32,
    count: u32,
) -> io::Result<Vec<X38Entry>> {
    r.seek(offset as usize)?;
    let mut out = Vec::new();
    for _ in 0..count {
        let kind = r.read_u32()?;
        let at = r.read_u32()? as usize;
        let id = guid(r)?;
        let ret = r.position();
        r.seek(at)?;
        let mut m = Mapping::new();
        if kind == 0 {
            m.insert(
                Value::String("Start Frame".into()),
                read_parameter(r, p, ParameterType::Float)?,
            );
            m.insert(
                Value::String("Unknown 2".into()),
                Value::from(r.read_u32()?),
            );
        } else if kind == 1 {
            m.insert(
                Value::String("Start Frame".into()),
                read_parameter(r, p, ParameterType::Float)?,
            );
            m.insert(
                Value::String("End Frame".into()),
                read_parameter(r, p, ParameterType::Float)?,
            );
            m.insert(
                Value::String("Unknown 3".into()),
                read_parameter(r, p, ParameterType::Float)?,
            );
        } else if kind != 3 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid ASB 0x38 type",
            ));
        }
        r.seek(ret)?;
        out.push(X38Entry {
            kind,
            guid: id,
            entry: Value::Mapping(m),
        });
    }
    Ok(out)
}
pub fn read_bones(
    r: &mut BinaryReader<'_>,
    p: &BinaryReader<'_>,
    offset: u32,
    count: u32,
) -> io::Result<Vec<BoneGroup>> {
    r.seek(offset as usize)?;
    let mut out = Vec::new();
    for _ in 0..count {
        let at = r.read_u32()? as usize;
        let name = p.read_c_string_at(r.read_u32()? as usize)?;
        let n = r.read_u32()?;
        let unknown = r.read_u32()?;
        let ret = r.position();
        r.seek(at)?;
        let mut bones = Vec::new();
        for _ in 0..n {
            bones.push(Bone {
                name: p.read_c_string_at(r.read_u32()? as usize)?,
                unknown: f64::from(r.read_f32()?),
            });
        }
        r.seek(ret)?;
        out.push(BoneGroup {
            name,
            unknown,
            bones,
        });
    }
    Ok(out)
}
pub fn read_markings(
    r: &mut BinaryReader<'_>,
    p: &BinaryReader<'_>,
    offset: u32,
) -> io::Result<Vec<Vec<String>>> {
    r.seek(offset as usize)?;
    let n = r.read_u32()?;
    let mut out = Vec::new();
    for _ in 0..n {
        out.push(vec![
            p.read_c_string_at(r.read_u32()? as usize)?,
            p.read_c_string_at(r.read_u32()? as usize)?,
            p.read_c_string_at(r.read_u32()? as usize)?,
        ]);
    }
    Ok(out)
}
