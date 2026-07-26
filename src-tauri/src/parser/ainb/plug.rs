use super::common::AinbWriter;
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::io;

pub const PLUG_NAMES: [&str; 10] = [
    "Generic",
    "_01",
    "Child",
    "Transition",
    "String",
    "Int",
    "_06",
    "_07",
    "_08",
    "_09",
];

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    pub transition_type: u32,
    pub update_post_calc: bool,
    pub command_name: String,
}

pub fn read_plug(
    reader: &mut BinaryReader<'_>,
    pool: usize,
    version: u32,
    node_type: &str,
    node_name: &str,
    plug_type: usize,
    is_last: bool,
    transitions: &[Transition],
) -> io::Result<Value> {
    let node_index = reader.read_i32()?;
    let mut output = mapping([("Node Index", Value::from(node_index))]);
    match plug_type {
        0 => {
            insert(&mut output, "Name", read_string(reader, pool)?);
            if node_type == "Element_BoolSelector" {
                insert(&mut output, "Unknown 1", reader.read_u32()?);
                insert(&mut output, "Unknown 2", reader.read_u32()?);
            } else if node_type == "Element_F32Selector" {
                insert(&mut output, "Unknown 1", reader.read_u32()?);
                insert(&mut output, "Unknown 2", reader.read_f32()? as f64);
            } else if node_type == "Element_Expression" {
                read_selector_input(reader, pool, version, &mut output, false)?;
            }
        }
        2 => {
            insert(&mut output, "Name", read_string(reader, pool)?);
            match node_type {
                "Element_S32Selector" => {
                    let index = reader.read_i16()?;
                    let flag = reader.read_u16()?;
                    let condition = reader.read_i32()?;
                    if is_last {
                        insert(&mut output, "Is Default", true);
                    } else if flag >> 15 != 0 {
                        insert(&mut output, "Blackboard Index", index);
                        insert(&mut output, "Default Condition", condition);
                    } else {
                        insert(&mut output, "Condition", condition);
                    }
                }
                "Element_F32Selector" => {
                    if is_last {
                        insert(&mut output, "Is Default", true);
                        reader.read_u32()?;
                        let default_offset = reader.read_u32()?;
                        if default_offset != 0 {
                            insert(
                                &mut output,
                                "Default Value",
                                reader.read_c_string_at(pool + default_offset as usize)?,
                            );
                        }
                        reader.skip(0x18)?;
                    } else {
                        read_f32_condition(reader, &mut output, true)?;
                        read_f32_condition(reader, &mut output, false)?;
                        reader.skip(0x10)?;
                    }
                }
                "Element_StringSelector" => {
                    let index = reader.read_i16()?;
                    let flag = reader.read_u16()?;
                    let condition = read_string(reader, pool)?;
                    if is_last {
                        insert(&mut output, "Is Default", true);
                    } else if flag >> 15 != 0 {
                        insert(&mut output, "Blackboard Index", index);
                        insert(&mut output, "Default Condition", condition);
                    } else {
                        insert(&mut output, "Condition", condition);
                    }
                }
                "Element_RandomSelector" => {
                    let index = reader.read_i16()?;
                    let flag = reader.read_u16()?;
                    let weight = reader.read_f32()? as f64;
                    if flag >> 15 != 0 {
                        insert(&mut output, "Blackboard Index", index);
                        insert(&mut output, "Default Weight", weight);
                    } else {
                        insert(&mut output, "Weight", weight);
                    }
                }
                _ if matches!(
                    node_name,
                    "SelectorBSABrainVerbUpdater" | "SelectorBSAFormChangeUpdater"
                ) =>
                {
                    let flag = reader.read_u32()?;
                    let value = reader.read_u32()?;
                    if flag >> 31 != 0 {
                        insert(&mut output, "Child Enum BB Index", flag & 0xffff);
                    } else {
                        insert(&mut output, "Child Enum Value", value);
                    }
                }
                _ => {}
            }
        }
        3 => {
            let index = reader.read_u32()? as usize;
            let transition = transitions
                .get(index)
                .ok_or_else(|| invalid("transition index exceeds table"))?;
            insert(&mut output, "Transition Type", transition.transition_type);
            insert(&mut output, "Update Post Calc", transition.update_post_calc);
            if transition.transition_type == 0 {
                insert(
                    &mut output,
                    "Transition Name",
                    transition.command_name.clone(),
                );
            }
        }
        4 => {
            insert(&mut output, "Name", read_string(reader, pool)?);
            if matches!(node_type, "Element_StringSelector" | "Element_Expression") {
                read_selector_input(reader, pool, version, &mut output, true)?;
            }
        }
        5 => {
            insert(&mut output, "Name", read_string(reader, pool)?);
            if matches!(node_type, "Element_S32Selector" | "Element_Expression") {
                read_selector_input(reader, pool, version, &mut output, false)?;
            }
        }
        _ => return Err(invalid(format!("unsupported plug type {plug_type}"))),
    }
    Ok(Value::Mapping(output))
}

pub fn plug_size(
    value: &Value,
    node_type: &str,
    node_name: &str,
    plug_type: usize,
) -> io::Result<usize> {
    let map = as_mapping(value)?;
    Ok(match plug_type {
        0 if matches!(node_type, "Element_BoolSelector" | "Element_F32Selector") => 0x10,
        0 if node_type == "Element_Expression" && map.contains_key(&key("Unknown")) => 0x10,
        0 => 8,
        2 if node_type == "Element_F32Selector" => 0x28,
        2 if matches!(
            node_type,
            "Element_S32Selector" | "Element_StringSelector" | "Element_RandomSelector"
        ) || matches!(
            node_name,
            "SelectorBSABrainVerbUpdater" | "SelectorBSAFormChangeUpdater"
        ) =>
        {
            0x10
        }
        2 => 8,
        3 => 8,
        4 | 5 if map.contains_key(&key("Unknown")) => 0x10,
        4 | 5 => 8,
        _ => return Err(invalid(format!("unsupported plug type {plug_type}"))),
    })
}

pub fn transition_from_plug(value: &Value) -> io::Result<Transition> {
    let map = as_mapping(value)?;
    Ok(Transition {
        transition_type: get_u32(map, "Transition Type")?,
        update_post_calc: get_bool(map, "Update Post Calc")?,
        command_name: get_optional_string(map, "Transition Name").unwrap_or_default(),
    })
}

pub fn write_plug(
    writer: &mut AinbWriter,
    value: &Value,
    node_type: &str,
    node_name: &str,
    plug_type: usize,
    transitions: &[Transition],
) -> io::Result<()> {
    let map = as_mapping(value)?;
    writer.write_i32(get_i32(map, "Node Index")?);
    match plug_type {
        0 => {
            writer.write_string_offset(get_string(map, "Name")?);
            if matches!(node_type, "Element_BoolSelector" | "Element_F32Selector") {
                writer.write_u32(get_u32(map, "Unknown 1")?);
                if node_type == "Element_F32Selector" {
                    writer.write_f32(get_f32(map, "Unknown 2")?);
                } else {
                    writer.write_u32(get_u32(map, "Unknown 2")?);
                }
            } else if node_type == "Element_Expression" {
                write_selector_input(writer, map, false)?;
            }
        }
        2 => {
            writer.write_string_offset(get_string(map, "Name")?);
            match node_type {
                "Element_S32Selector" => {
                    write_index_or_zero(writer, map, "Blackboard Index")?;
                    writer.write_i32(if get_optional_bool(map, "Is Default").unwrap_or(false) {
                        0
                    } else {
                        get_optional_i32(map, "Condition")
                            .or_else(|| get_optional_i32(map, "Default Condition"))
                            .ok_or_else(|| invalid("missing S32 selector condition"))?
                    });
                }
                "Element_F32Selector" => {
                    if get_optional_bool(map, "Is Default").unwrap_or(false) {
                        writer.write_u32(0);
                        if let Some(value) = get_optional_string(map, "Default Value") {
                            writer.write_string_offset(&value);
                        } else {
                            writer.write_u32(0);
                        }
                        writer.write_bytes(&[0; 0x18]);
                    } else {
                        write_f32_condition(writer, map, true)?;
                        write_f32_condition(writer, map, false)?;
                        writer.write_bytes(&[0; 0x10]);
                    }
                }
                "Element_StringSelector" => {
                    write_index_or_zero(writer, map, "Blackboard Index")?;
                    if get_optional_bool(map, "Is Default").unwrap_or(false) {
                        writer.write_string_offset("その他");
                    } else {
                        writer.write_string_offset(
                            get_optional_string(map, "Condition")
                                .or_else(|| get_optional_string(map, "Default Condition"))
                                .as_deref()
                                .ok_or_else(|| invalid("missing string selector condition"))?,
                        );
                    }
                }
                "Element_RandomSelector" => {
                    write_index_or_zero(writer, map, "Blackboard Index")?;
                    writer.write_f32(
                        get_optional_f32(map, "Weight")
                            .or_else(|| get_optional_f32(map, "Default Weight"))
                            .ok_or_else(|| invalid("missing random selector weight"))?,
                    );
                }
                _ if matches!(
                    node_name,
                    "SelectorBSABrainVerbUpdater" | "SelectorBSAFormChangeUpdater"
                ) =>
                {
                    if let Some(index) = get_optional_u32(map, "Child Enum BB Index") {
                        writer.write_u32(index | 0x0800_0000);
                        writer.write_u32(0);
                    } else {
                        writer.write_u32(0);
                        writer.write_u32(get_u32(map, "Child Enum Value")?);
                    }
                }
                _ => {}
            }
        }
        3 => {
            let transition = transition_from_plug(value)?;
            let index = transitions
                .iter()
                .position(|candidate| candidate == &transition)
                .ok_or_else(|| invalid("transition plug missing from context"))?;
            writer.write_u32(index as u32);
        }
        4 => {
            writer.write_string_offset(get_string(map, "Name")?);
            if matches!(node_type, "Element_StringSelector" | "Element_Expression") {
                write_selector_input(writer, map, true)?;
            }
        }
        5 => {
            writer.write_string_offset(get_string(map, "Name")?);
            if matches!(node_type, "Element_S32Selector" | "Element_Expression") {
                write_selector_input(writer, map, false)?;
            }
        }
        _ => return Err(invalid(format!("unsupported plug type {plug_type}"))),
    }
    Ok(())
}

fn read_selector_input(
    reader: &mut BinaryReader<'_>,
    pool: usize,
    version: u32,
    map: &mut Mapping,
    string_value: bool,
) -> io::Result<()> {
    if version < 0x407 {
        return Ok(());
    }
    insert(map, "Unknown", reader.read_u32()?);
    if string_value {
        insert(map, "Default Value", read_string(reader, pool)?);
    } else {
        insert(map, "Default Value", reader.read_i32()?);
    }
    Ok(())
}

fn write_selector_input(
    writer: &mut AinbWriter,
    map: &Mapping,
    string_value: bool,
) -> io::Result<()> {
    if !map.contains_key(&key("Unknown")) {
        return Ok(());
    }
    writer.write_u32(get_u32(map, "Unknown")?);
    if string_value {
        writer.write_string_offset(get_string(map, "Default Value")?);
    } else {
        writer.write_i32(get_i32(map, "Default Value")?);
    }
    Ok(())
}

fn read_f32_condition(
    reader: &mut BinaryReader<'_>,
    map: &mut Mapping,
    minimum: bool,
) -> io::Result<()> {
    let index = reader.read_i16()?;
    let flag = reader.read_u16()?;
    let value = reader.read_f32()? as f64;
    if flag >> 15 != 0 {
        insert(
            map,
            if minimum {
                "Condition Min Blackboard Index"
            } else {
                "Condition Max Blackboard Index"
            },
            index,
        );
    } else {
        insert(
            map,
            if minimum {
                "Condition Min"
            } else {
                "Condition Max"
            },
            value,
        );
    }
    Ok(())
}

fn write_f32_condition(writer: &mut AinbWriter, map: &Mapping, minimum: bool) -> io::Result<()> {
    let bb_key = if minimum {
        "Condition Min Blackboard Index"
    } else {
        "Condition Max Blackboard Index"
    };
    let value_key = if minimum {
        "Condition Min"
    } else {
        "Condition Max"
    };
    if let Some(index) = get_optional_i32(map, bb_key) {
        writer.write_i16(index as i16);
        writer.write_u16(0x8000);
        writer.write_u32(0);
    } else {
        writer.write_u32(0);
        writer.write_f32(get_f32(map, value_key)?);
    }
    Ok(())
}

fn write_index_or_zero(writer: &mut AinbWriter, map: &Mapping, name: &str) -> io::Result<()> {
    if let Some(index) = get_optional_i32(map, name) {
        writer.write_i16(index as i16);
        writer.write_u16(0x8000);
    } else {
        writer.write_u32(0);
    }
    Ok(())
}

fn mapping<const N: usize>(values: [(&str, Value); N]) -> Mapping {
    values
        .into_iter()
        .map(|(name, value)| (key(name), value))
        .collect()
}

fn insert(map: &mut Mapping, name: &str, value: impl Into<Value>) {
    map.insert(key(name), value.into());
}

fn key(name: &str) -> Value {
    Value::from(name)
}

fn as_mapping(value: &Value) -> io::Result<&Mapping> {
    value
        .as_mapping()
        .ok_or_else(|| invalid("plug must be a mapping"))
}

fn get_string<'a>(map: &'a Mapping, name: &str) -> io::Result<&'a str> {
    map.get(Value::from(name))
        .and_then(Value::as_str)
        .ok_or_else(|| invalid(format!("missing string field {name}")))
}

fn get_optional_string(map: &Mapping, name: &str) -> Option<String> {
    map.get(Value::from(name))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn get_i32(map: &Mapping, name: &str) -> io::Result<i32> {
    get_optional_i32(map, name).ok_or_else(|| invalid(format!("missing i32 field {name}")))
}

fn get_optional_i32(map: &Mapping, name: &str) -> Option<i32> {
    map.get(Value::from(name))
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn get_u32(map: &Mapping, name: &str) -> io::Result<u32> {
    get_optional_u32(map, name).ok_or_else(|| invalid(format!("missing u32 field {name}")))
}

fn get_optional_u32(map: &Mapping, name: &str) -> Option<u32> {
    map.get(Value::from(name))
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
}

fn get_f32(map: &Mapping, name: &str) -> io::Result<f32> {
    get_optional_f32(map, name).ok_or_else(|| invalid(format!("missing f32 field {name}")))
}

fn get_optional_f32(map: &Mapping, name: &str) -> Option<f32> {
    map.get(Value::from(name))
        .and_then(Value::as_f64)
        .map(|value| value as f32)
}

fn get_bool(map: &Mapping, name: &str) -> io::Result<bool> {
    get_optional_bool(map, name).ok_or_else(|| invalid(format!("missing bool field {name}")))
}

fn get_optional_bool(map: &Mapping, name: &str) -> Option<bool> {
    map.get(Value::from(name)).and_then(Value::as_bool)
}

fn read_string(reader: &mut BinaryReader<'_>, pool: usize) -> io::Result<String> {
    let offset = reader.read_u32()? as usize;
    reader.read_c_string_at(pool + offset)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
