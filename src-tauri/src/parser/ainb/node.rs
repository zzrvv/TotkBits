use super::{
    model::{Action, Attachment},
    parameter::{ParamSet, ParamType, PropertySet},
    plug::{read_plug, Transition, PLUG_NAMES},
};
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::{collections::BTreeMap, io};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AinbNode {
    #[serde(rename = "Node Type")]
    pub node_type: String,
    #[serde(rename = "Node Index")]
    pub index: i16,
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "GUID")]
    pub guid: String,
    #[serde(rename = "Flags")]
    pub node_flags: Vec<String>,
    #[serde(rename = "Queries")]
    pub queries: Vec<u32>,
    #[serde(rename = "Attachments")]
    pub attachments: Vec<Attachment>,
    #[serde(rename = "Properties")]
    pub properties: PropertySet,
    #[serde(rename = "Parameters")]
    pub parameters: ParamSet,
    #[serde(rename = "XLink Actions")]
    pub xlink_actions: Vec<Action>,
    #[serde(rename = "Plugs")]
    pub plugs: BTreeMap<String, Vec<Value>>,

    #[serde(skip)]
    pub raw_flags: u8,
    #[serde(skip)]
    pub state_info_offset: u32,
    #[serde(skip)]
    pub raw_query_base: u16,
    #[serde(skip)]
    pub raw_multi_count: u16,
    #[serde(skip)]
    pub raw_parameter_offset: u32,
}

pub struct NodeTables<'a> {
    pub attachments: &'a [Attachment],
    pub attachment_indices: &'a [u32],
    pub properties: &'a PropertySet,
    pub parameters: &'a ParamSet,
    pub transitions: &'a [Transition],
    pub queries: &'a [u32],
    pub actions: &'a BTreeMap<i32, Vec<Action>>,
}

impl AinbNode {
    pub fn read(
        reader: &mut BinaryReader<'_>,
        version: u32,
        pool: usize,
        tables: &NodeTables<'_>,
    ) -> io::Result<Self> {
        let node_type = node_type_name(reader.read_u16()?)?.to_owned();
        let index = reader.read_i16()?;
        let attachment_count = reader.read_u16()? as usize;
        let raw_flags = reader.read_u8()?;
        reader.read_u8()?;
        let name = read_string(reader, pool)?;
        if version >= 0x407 {
            reader.read_u32()?;
        }
        reader.read_u32()?;
        let parameter_offset = reader.read_u32()? as usize;
        reader.read_u16()?;
        reader.read_u16()?;
        let raw_multi_count = reader.read_u16()?;
        reader.read_u16()?;
        let base_attachment_index = reader.read_u32()? as usize;
        let raw_query_base = reader.read_u16()?;
        let base_query_index = raw_query_base as usize;
        let query_count = reader.read_u16()? as usize;
        let state_info_offset = reader.read_u32()?;
        let guid = read_guid(reader)?;
        let return_position = reader.position();

        let attachment_indices = tables
            .attachment_indices
            .get(base_attachment_index..base_attachment_index + attachment_count)
            .ok_or_else(|| invalid("node attachment range exceeds table"))?;
        let attachments = attachment_indices
            .iter()
            .map(|index| {
                tables
                    .attachments
                    .get(*index as usize)
                    .cloned()
                    .ok_or_else(|| invalid("attachment index exceeds table"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let queries = tables
            .queries
            .get(base_query_index..base_query_index + query_count)
            .ok_or_else(|| invalid("node query range exceeds table"))?
            .to_vec();

        reader.seek(parameter_offset)?;
        let mut properties = PropertySet::new();
        for kind in ParamType::ALL {
            let base = reader.read_u32()? as usize;
            let count = reader.read_u32()? as usize;
            let values = tables
                .properties
                .get(kind.name())
                .map(Vec::as_slice)
                .unwrap_or(&[])
                .get(base..base + count)
                .ok_or_else(|| invalid("node property range exceeds table"))?
                .to_vec();
            if !values.is_empty() {
                properties.insert(kind.name().to_owned(), values);
            }
        }
        let mut parameters = ParamSet::default();
        for kind in ParamType::ALL {
            let input_base = reader.read_u32()? as usize;
            let input_count = reader.read_u32()? as usize;
            let output_base = reader.read_u32()? as usize;
            let output_count = reader.read_u32()? as usize;
            let inputs = tables
                .parameters
                .inputs
                .get(kind.name())
                .map(Vec::as_slice)
                .unwrap_or(&[])
                .get(input_base..input_base + input_count)
                .ok_or_else(|| invalid("node input range exceeds table"))?
                .to_vec();
            let outputs = tables
                .parameters
                .outputs
                .get(kind.name())
                .map(Vec::as_slice)
                .unwrap_or(&[])
                .get(output_base..output_base + output_count)
                .ok_or_else(|| invalid("node output range exceeds table"))?
                .to_vec();
            if !inputs.is_empty() {
                parameters.inputs.insert(kind.name().to_owned(), inputs);
            }
            if !outputs.is_empty() {
                parameters.outputs.insert(kind.name().to_owned(), outputs);
            }
        }
        let mut plug_info = [(0usize, 0usize); 10];
        for item in &mut plug_info {
            *item = (reader.read_u8()? as usize, reader.read_u8()? as usize);
        }
        let offset_base = reader.position();
        let mut plugs = BTreeMap::new();
        for plug_type in 0..10 {
            let (count, base) = plug_info[plug_type];
            if count == 0 {
                continue;
            }
            reader.seek(offset_base + base * 4)?;
            let offsets = (0..count)
                .map(|_| reader.read_u32().map(|value| value as usize))
                .collect::<io::Result<Vec<_>>>()?;
            let values = offsets
                .into_iter()
                .enumerate()
                .map(|(plug_index, offset)| {
                    reader.seek(offset)?;
                    read_plug(
                        reader,
                        pool,
                        version,
                        &node_type,
                        &name,
                        plug_type,
                        plug_index + 1 == count,
                        tables.transitions,
                    )
                })
                .collect::<io::Result<Vec<_>>>()?;
            plugs.insert(PLUG_NAMES[plug_type].to_owned(), values);
        }
        reader.seek(return_position)?;
        Ok(Self {
            node_type,
            index,
            name,
            guid,
            node_flags: flag_names(raw_flags),
            queries,
            attachments,
            properties,
            parameters,
            xlink_actions: tables
                .actions
                .get(&(index as i32))
                .cloned()
                .unwrap_or_default(),
            plugs,
            raw_flags,
            state_info_offset,
            raw_query_base,
            raw_multi_count,
            raw_parameter_offset: parameter_offset as u32,
        })
    }

    pub fn flags(&self) -> io::Result<u8> {
        let mut flags = self.raw_flags & !0x0f;
        for name in &self.node_flags {
            flags |= match name.as_str() {
                "Is Query" => 1,
                "Is Module" => 2,
                "Is Root Node" => 4,
                "Use MultiParam Type 2" => 8,
                other => return Err(invalid(format!("unknown node flag {other}"))),
            };
        }
        Ok(flags)
    }
}

fn flag_names(flags: u8) -> Vec<String> {
    [
        (1, "Is Query"),
        (2, "Is Module"),
        (4, "Is Root Node"),
        (8, "Use MultiParam Type 2"),
    ]
    .into_iter()
    .filter(|(mask, _)| flags & mask != 0)
    .map(|(_, name)| name.to_owned())
    .collect()
}

pub fn node_type_value(name: &str) -> io::Result<u16> {
    NODE_TYPES
        .iter()
        .find(|(_, candidate)| *candidate == name)
        .map(|(value, _)| *value)
        .ok_or_else(|| invalid(format!("unsupported AINB node type {name}")))
}

fn node_type_name(value: u16) -> io::Result<&'static str> {
    NODE_TYPES
        .iter()
        .find(|(candidate, _)| *candidate == value)
        .map(|(_, name)| *name)
        .ok_or_else(|| invalid(format!("unsupported AINB node type {value}")))
}

const NODE_TYPES: &[(u16, &str)] = &[
    (0, "UserDefined"),
    (1, "Element_S32Selector"),
    (2, "Element_Sequential"),
    (3, "Element_Simultaneous"),
    (4, "Element_F32Selector"),
    (5, "Element_StringSelector"),
    (6, "Element_RandomSelector"),
    (7, "Element_BoolSelector"),
    (8, "Element_Fork"),
    (9, "Element_Join"),
    (10, "Element_Alert"),
    (20, "Element_Expression"),
    (100, "Element_ModuleIF_Input_S32"),
    (101, "Element_ModuleIF_Input_F32"),
    (102, "Element_ModuleIF_Input_Vec3f"),
    (103, "Element_ModuleIF_Input_String"),
    (104, "Element_ModuleIF_Input_Bool"),
    (105, "Element_ModuleIF_Input_Ptr"),
    (200, "Element_ModuleIF_Output_S32"),
    (201, "Element_ModuleIF_Output_F32"),
    (202, "Element_ModuleIF_Output_Vec3f"),
    (203, "Element_ModuleIF_Output_String"),
    (204, "Element_ModuleIF_Output_Bool"),
    (205, "Element_ModuleIF_Output_Ptr"),
    (300, "Element_ModuleIF_Child"),
    (400, "Element_StateEnd"),
    (500, "Element_SplitTiming"),
];

fn read_guid(reader: &mut BinaryReader<'_>) -> io::Result<String> {
    let a = reader.read_u32()?;
    let b = reader.read_u16()?;
    let c = reader.read_u16()?;
    let d = reader.read_bytes(8)?;
    Ok(format!(
        "{a:08x}-{b:04x}-{c:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
    ))
}

fn read_string(reader: &mut BinaryReader<'_>, pool: usize) -> io::Result<String> {
    let offset = reader.read_u32()? as usize;
    reader.read_c_string_at(pool + offset)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
