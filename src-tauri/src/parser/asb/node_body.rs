use super::{
    event::AsbEvent,
    node_connections::NodeConnections,
    node_tables::BoneGroup,
    parameter::{read_parameter, ParameterType},
    x2c::X2cEntry,
};
use crate::parser::binary::BinaryReader;
use serde_yaml::{Mapping, Value};
use std::io;
pub fn read(
    kind: &str,
    r: &mut BinaryReader<'_>,
    p: &BinaryReader<'_>,
    version: u32,
    x2c: &[X2cEntry],
    events: &[AsbEvent],
    bone_groups: &[BoneGroup],
) -> io::Result<Option<Value>> {
    let mut m = Mapping::new();
    match kind {
        "SkeletalAnimation" => {
            put(
                &mut m,
                "Animation",
                read_parameter(r, p, ParameterType::String)?,
            );
            put(&mut m, "Unknown 1", r.read_u32()?);
            put(&mut m, "Unknown 2", r.read_u32()?);
            put(
                &mut m,
                "Unknown 3",
                read_parameter(r, p, ParameterType::Bool)?,
            );
            put(
                &mut m,
                "Unknown 4",
                read_parameter(r, p, ParameterType::Float)?,
            );
            finish(&mut m, r, version, x2c)?;
        }
        "Sequential" => {
            put(
                &mut m,
                "Unknown 1",
                read_parameter(r, p, ParameterType::Bool)?,
            );
            put(
                &mut m,
                "Unknown 2",
                read_parameter(r, p, ParameterType::Int)?,
            );
            put(
                &mut m,
                "Unknown 3",
                read_parameter(r, p, ParameterType::Int)?,
            );
            finish(&mut m, r, version, x2c)?;
        }
        "Simultaneous" => {
            put(&mut m, "Unknown", r.read_u32()?);
            finish(&mut m, r, version, x2c)?;
        }
        "Event" => {
            let i = r.read_u32()? as usize;
            put(
                &mut m,
                "Event",
                events.get(i).ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "ASB event index exceeds table")
                })?,
            );
            finish(&mut m, r, version, x2c)?;
        }
        "MaterialAnimation" => {
            if version == 0x417 {
                put(&mut m, "Unknown 1", r.read_u32()?);
            }
            put(
                &mut m,
                "Animation",
                read_parameter(r, p, ParameterType::String)?,
            );
            put(
                &mut m,
                "Unknown 2",
                read_parameter(r, p, ParameterType::Bool)?,
            );
            finish(&mut m, r, version, x2c)?;
        }
        "DummyAnimation" => {
            put(&mut m, "Frame", read_parameter(r, p, ParameterType::Float)?);
            put(
                &mut m,
                "Unknown",
                read_parameter(r, p, ParameterType::Bool)?,
            );
            finish(&mut m, r, version, x2c)?;
        }
        "OneDimensionalBlender" => {
            put(
                &mut m,
                "Parameter",
                read_parameter(r, p, ParameterType::Float)?,
            );
            put(&mut m, "Unknown", r.read_u32()?);
            let c = NodeConnections::read(r, version, x2c)?;
            let mut children = Vec::new();
            for at in &c.child_offsets {
                r.seek(*at as usize)?;
                let mut x = Mapping::new();
                put(
                    &mut x,
                    "Condition Min",
                    read_parameter(r, p, ParameterType::Float)?,
                );
                put(
                    &mut x,
                    "Condition Max",
                    read_parameter(r, p, ParameterType::Float)?,
                );
                put(&mut x, "Node Index", r.read_u32()?);
                children.push(Value::Mapping(x));
            }
            if !children.is_empty() {
                put(&mut m, "Child Nodes", children);
            }
            merge(&mut m, c)?;
        }
        "RandomSelector" => {
            put(&mut m, "Unknown 1", r.read_u32()?);
            put(
                &mut m,
                "Unknown 2",
                read_parameter(r, p, ParameterType::Bool)?,
            );
            put(
                &mut m,
                "Unknown 3",
                read_parameter(r, p, ParameterType::Int)?,
            );
            put(&mut m, "Unknown 4", r.read_u32()? != 0);
            let c = NodeConnections::read(r, version, x2c)?;
            let mut children = Vec::new();
            for at in &c.child_offsets {
                r.seek(*at as usize)?;
                let mut x = Mapping::new();
                put(
                    &mut x,
                    "Weight",
                    read_parameter(r, p, ParameterType::Float)?,
                );
                put(&mut x, "Node Index", r.read_u32()?);
                children.push(Value::Mapping(x));
            }
            if !children.is_empty() {
                put(&mut m, "Child Nodes", children);
            }
            merge(&mut m, c)?;
        }
        "FrameController" => {
            for (name, ty) in [
                ("Animation Rate", ParameterType::Float),
                ("Start Frame", ParameterType::Float),
                ("End Frame", ParameterType::Float),
            ] {
                put(&mut m, name, read_parameter(r, p, ty)?);
            }
            put(&mut m, "Unknown Flag", r.read_u32()?);
            for (name, ty) in [
                ("Loop Cancel Flag", ParameterType::Bool),
                ("Unknown 2", ParameterType::Bool),
                ("Unknown 3", ParameterType::Int),
                ("Unknown 4", ParameterType::Int),
                ("Unknown 5", ParameterType::Bool),
                ("Unknown 6", ParameterType::Float),
                ("Unknown 7", ParameterType::Float),
                ("Unknown 8", ParameterType::Float),
            ] {
                put(&mut m, name, read_parameter(r, p, ty)?);
            }
            put(&mut m, "Unknown 9", r.read_u32()? != 0);
            put(
                &mut m,
                "Unknown 10",
                read_parameter(r, p, ParameterType::Float)?,
            );
            if version == 0x417 {
                put(
                    &mut m,
                    "Unknown 11",
                    read_parameter(r, p, ParameterType::Bool)?,
                );
            }
            put(&mut m, "Unknown 12", r.read_u32()?);
            put(&mut m, "Unknown 13", r.read_u32()?);
            finish(&mut m, r, version, x2c)?;
        }
        "BoneAnimation" => {
            put(
                &mut m,
                "Animation",
                read_parameter(r, p, ParameterType::String)?,
            );
            put(
                &mut m,
                "Unknown 1",
                read_parameter(r, p, ParameterType::Bool)?,
            );
            put(&mut m, "Unknown 2", r.read_u32()?);
            put(&mut m, "Unknown 3", r.read_u32()?);
            put(
                &mut m,
                "Unknown 4",
                read_parameter(r, p, ParameterType::Float)?,
            );
            finish(&mut m, r, version, x2c)?;
        }
        "Alert" => {
            put(
                &mut m,
                "Message",
                read_parameter(r, p, ParameterType::String)?,
            );
            finish(&mut m, r, version, x2c)?;
        }
        "ShapeAnimation" => {
            put(
                &mut m,
                "Animation",
                read_parameter(r, p, ParameterType::String)?,
            );
            finish(&mut m, r, version, x2c)?;
        }
        "StringSelector" => {
            put(
                &mut m,
                "Parameter",
                read_parameter(r, p, ParameterType::String)?,
            );
            put(
                &mut m,
                "Unknown 1",
                read_parameter(r, p, ParameterType::Bool)?,
            );
            put(&mut m, "Unknown 2", r.read_u32()? != 0);
            selector_children(&mut m, r, p, version, x2c, ParameterType::String)?;
        }
        "FloatSelector" => {
            put(
                &mut m,
                "Parameter",
                read_parameter(r, p, ParameterType::Float)?,
            );
            put(
                &mut m,
                "Unknown 1",
                read_parameter(r, p, ParameterType::Bool)?,
            );
            put(&mut m, "Unknown 2", r.read_u32()? != 0);
            let c = NodeConnections::read(r, version, x2c)?;
            let len = c.child_offsets.len();
            let mut v = Vec::new();
            for (i, at) in c.child_offsets.iter().enumerate() {
                r.seek(*at as usize)?;
                let mut x = Mapping::new();
                if i + 1 == len {
                    put(
                        &mut x,
                        "Default Condition",
                        read_parameter(r, p, ParameterType::String)?,
                    );
                    r.skip(8)?;
                } else {
                    put(
                        &mut x,
                        "Condition Min",
                        read_parameter(r, p, ParameterType::Float)?,
                    );
                    put(
                        &mut x,
                        "Condition Max",
                        read_parameter(r, p, ParameterType::Float)?,
                    );
                }
                put(&mut x, "Node Index", r.read_u32()?);
                v.push(Value::Mapping(x));
            }
            if !v.is_empty() {
                put(&mut m, "Child Nodes", v);
            }
            merge(&mut m, c)?;
        }
        "IntSelector" => {
            put(
                &mut m,
                "Parameter",
                read_parameter(r, p, ParameterType::Int)?,
            );
            put(
                &mut m,
                "Unknown 1",
                read_parameter(r, p, ParameterType::Bool)?,
            );
            put(&mut m, "Unknown 2", r.read_u32()? != 0);
            let c = NodeConnections::read(r, version, x2c)?;
            let len = c.child_offsets.len();
            let mut v = Vec::new();
            for (i, at) in c.child_offsets.iter().enumerate() {
                r.seek(*at as usize)?;
                let mut x = Mapping::new();
                put(
                    &mut x,
                    if i + 1 == len {
                        "Default Condition"
                    } else {
                        "Condition"
                    },
                    read_parameter(r, p, ParameterType::Int)?,
                );
                put(&mut x, "Node Index", r.read_u32()?);
                v.push(Value::Mapping(x));
            }
            if !v.is_empty() {
                put(&mut m, "Child Nodes", v);
            }
            merge(&mut m, c)?;
        }
        "BoolSelector" => {
            put(
                &mut m,
                "Parameter",
                read_parameter(r, p, ParameterType::Bool)?,
            );
            put(
                &mut m,
                "Unknown 1",
                read_parameter(r, p, ParameterType::Bool)?,
            );
            put(&mut m, "Unknown 2", r.read_u32()? != 0);
            let c = NodeConnections::read(r, version, x2c)?;
            let mut v = Vec::new();
            for (i, at) in c.child_offsets.iter().enumerate() {
                r.seek(*at as usize)?;
                let mut x = Mapping::new();
                put(
                    &mut x,
                    if i == 0 {
                        "Condition True"
                    } else {
                        "Condition False"
                    },
                    r.read_u32()?,
                );
                v.push(Value::Mapping(x));
            }
            if !v.is_empty() {
                put(&mut m, "Child Nodes", v);
            }
            merge(&mut m, c)?;
        }
        "PreviousTagSelector" => {
            put(&mut m, "Unknown", r.read_u32()?);
            let c = NodeConnections::read(r, version, x2c)?;
            let mut v = Vec::new();
            for at in &c.child_offsets {
                r.seek(*at as usize)?;
                let tag_offset = r.read_u32()?;
                let node_index = r.read_u32()?;
                let tags = if tag_offset != u32::MAX {
                    read_tags(r, p, tag_offset)?
                } else {
                    Vec::new()
                };
                let mut x = Mapping::new();
                put(&mut x, "Tags", tags);
                put(&mut x, "Node Index", node_index);
                v.push(Value::Mapping(x));
            }
            if !v.is_empty() {
                put(&mut m, "Child Nodes", v);
            }
            merge(&mut m, c)?;
        }
        "BonePositionSelector" => {
            put(
                &mut m,
                "Bone 1",
                read_parameter(r, p, ParameterType::String)?,
            );
            put(
                &mut m,
                "Bone 2",
                read_parameter(r, p, ParameterType::String)?,
            );
            put(&mut m, "Unknown 1", r.read_u32()?);
            put(&mut m, "Unknown 2", r.read_u32()?);
            put(
                &mut m,
                "Unknown 3",
                read_parameter(r, p, ParameterType::Bool)?,
            );
            let c = NodeConnections::read(r, version, x2c)?;
            let len = c.child_offsets.len();
            let mut v = Vec::new();
            for (i, at) in c.child_offsets.iter().enumerate() {
                r.seek(*at as usize)?;
                let mut x = Mapping::new();
                if i + 1 == len {
                    put(
                        &mut x,
                        "Default Condition",
                        read_parameter(r, p, ParameterType::String)?,
                    );
                    r.skip(8)?;
                } else {
                    put(
                        &mut x,
                        "Condition Min",
                        read_parameter(r, p, ParameterType::Float)?,
                    );
                    put(
                        &mut x,
                        "Condition Max",
                        read_parameter(r, p, ParameterType::Float)?,
                    );
                }
                put(&mut x, "Node Index", r.read_u32()?);
                v.push(Value::Mapping(x));
            }
            if !v.is_empty() {
                put(&mut m, "Child Nodes", v);
            }
            merge(&mut m, c)?;
        }
        "InitialFrame" => {
            put(&mut m, "Flag", r.read_u32()?);
            let tag_offset = r.read_u32()?;
            if tag_offset != 0 {
                put(&mut m, "Tags", read_tags(r, p, tag_offset)?);
            }
            if version == 0x417 {
                put(
                    &mut m,
                    "Unknown 1",
                    read_parameter(r, p, ParameterType::Bool)?,
                );
            }
            put(
                &mut m,
                "Bone 1",
                read_parameter(r, p, ParameterType::String)?,
            );
            put(
                &mut m,
                "Bone 2",
                read_parameter(r, p, ParameterType::String)?,
            );
            put(&mut m, "Unknown 2", r.read_u32()?);
            put(
                &mut m,
                "Unknown 3",
                read_parameter(r, p, ParameterType::Bool)?,
            );
            put(
                &mut m,
                "Unknown 4",
                read_parameter(r, p, ParameterType::Bool)?,
            );
            finish(&mut m, r, version, x2c)?;
        }
        "BoneBlender" => {
            let name = read_parameter(r, p, ParameterType::String)?;
            if let Value::String(ref value) = name {
                if let Some(group) = bone_groups.iter().find(|g| &g.name == value) {
                    put(&mut m, "Bone Group", group);
                }
            }
            put(&mut m, "Unknown 1", r.read_u32()?);
            put(
                &mut m,
                "Unknown 2",
                read_parameter(r, p, ParameterType::Float)?,
            );
            put(&mut m, "Unknown 3", r.read_u32()?);
            if version == 0x417 {
                put(&mut m, "Unknown 4", r.read_u32()?);
            }
            finish(&mut m, r, version, x2c)?;
        }
        "State" | "SubtractAnimation" | "Unknown7" => {
            finish(&mut m, r, version, x2c)?;
        }
        "Unknown2" | "Unknown4" => return Ok(None),
        _ => return Ok(None),
    }
    Ok(Some(Value::Mapping(m)))
}
fn finish(
    m: &mut Mapping,
    r: &mut BinaryReader<'_>,
    version: u32,
    x2c: &[X2cEntry],
) -> io::Result<()> {
    let c = NodeConnections::read(r, version, x2c)?;
    if !c.child_offsets.is_empty() {
        let mut children = Vec::new();
        for at in &c.child_offsets {
            r.seek(*at as usize)?;
            children.push(r.read_u32()?);
        }
        put(m, "Child Nodes", children);
    }
    merge(m, c)
}
fn merge(m: &mut Mapping, c: NodeConnections) -> io::Result<()> {
    if let Value::Mapping(extra) = serde_yaml::to_value(c).map_err(io::Error::other)? {
        for (k, v) in extra {
            m.insert(k, v);
        }
    }
    Ok(())
}
fn put<T: serde::Serialize>(m: &mut Mapping, k: &str, v: T) {
    m.insert(
        Value::String(k.into()),
        serde_yaml::to_value(v).unwrap_or(Value::Null),
    );
}
fn read_tags(
    r: &mut BinaryReader<'_>,
    p: &BinaryReader<'_>,
    offset: u32,
) -> io::Result<Vec<String>> {
    let ret = r.position();
    r.seek(offset as usize)?;
    let n = r.read_u32()?;
    let mut v = Vec::new();
    for _ in 0..n {
        v.push(p.read_c_string_at(r.read_u32()? as usize)?)
    }
    r.seek(ret)?;
    Ok(v)
}
fn selector_children(
    m: &mut Mapping,
    r: &mut BinaryReader<'_>,
    p: &BinaryReader<'_>,
    version: u32,
    x2c: &[X2cEntry],
    ty: ParameterType,
) -> io::Result<()> {
    let c = NodeConnections::read(r, version, x2c)?;
    let len = c.child_offsets.len();
    let mut v = Vec::new();
    for (i, at) in c.child_offsets.iter().enumerate() {
        r.seek(*at as usize)?;
        let mut x = Mapping::new();
        put(
            &mut x,
            if i + 1 == len {
                "Default Condition"
            } else {
                "Condition"
            },
            read_parameter(r, p, ty)?,
        );
        put(&mut x, "Node Index", r.read_u32()?);
        v.push(Value::Mapping(x));
    }
    if !v.is_empty() {
        put(m, "Child Nodes", v);
    }
    merge(m, c)
}
