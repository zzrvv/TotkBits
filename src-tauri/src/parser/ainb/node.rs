use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AinbNode {
    pub node_type: String,
    #[serde(rename = "Node Index")]
    pub index: i16,
    #[serde(skip)]
    pub attachment_count: u16,
    #[serde(skip)]
    pub flags: u8,
    pub name: String,
    #[serde(skip)]
    pub name_hash: Option<u32>,
    #[serde(skip)]
    pub unknown: u32,
    #[serde(skip)]
    pub parameter_offset: u32,
    #[serde(skip)]
    pub expression_count: u16,
    #[serde(skip)]
    pub expression_io_memory_size: u16,
    #[serde(skip)]
    pub multi_parameter_count: u16,
    #[serde(skip)]
    pub base_attachment_index: u32,
    #[serde(skip)]
    pub base_query_index: u16,
    #[serde(skip)]
    pub query_count: u16,
    #[serde(skip)]
    pub state_info_offset: u32,
    pub guid: String,
    #[serde(rename = "Flags")]
    pub node_flags: Vec<String>,
    pub queries: Vec<u32>,
    pub attachments: serde_yaml::Value,
    pub properties: serde_yaml::Value,
    pub parameters: serde_yaml::Value,
    pub xlink_actions: serde_yaml::Value,
    pub plugs: serde_yaml::Value,
}

impl AinbNode {
    pub fn read(
        reader: &mut BinaryReader<'_>,
        version: u32,
        string_pool_offset: usize,
    ) -> io::Result<Self> {
        let node_type = node_type_name(reader.read_u16()?)?.to_string();
        let index = reader.read_i16()?;
        let attachment_count = reader.read_u16()?;
        let flags = reader.read_u8()?;
        reader.skip(1)?;
        let name_offset = reader.read_u32()? as usize;
        let name_hash = if version >= 0x407 {
            Some(reader.read_u32()?)
        } else {
            None
        };
        let unknown = reader.read_u32()?;
        let parameter_offset = reader.read_u32()?;
        let expression_count = reader.read_u16()?;
        let expression_io_memory_size = reader.read_u16()?;
        let multi_parameter_count = reader.read_u16()?;
        reader.skip(2)?;
        let base_attachment_index = reader.read_u32()?;
        let base_query_index = reader.read_u16()?;
        let query_count = reader.read_u16()?;
        let state_info_offset = reader.read_u32()?;
        let a = reader.read_u32()?;
        let b = reader.read_u16()?;
        let c = reader.read_u16()?;
        let d = reader.read_bytes(8)?;
        let guid = format!(
            "{a:08x}-{b:04x}-{c:04x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]
        );
        let mut node_flags = Vec::new();
        if flags & 1 != 0 {
            node_flags.push("Is Query".into());
        }
        if flags & 2 != 0 {
            node_flags.push("Is Module".into());
        }
        if flags & 4 != 0 {
            node_flags.push("Is Root Node".into());
        }
        if flags & 8 != 0 {
            node_flags.push("Use MultiParam Type 2".into());
        }
        Ok(Self {
            node_type,
            index,
            attachment_count,
            flags,
            name: reader.read_c_string_at(string_pool_offset + name_offset)?,
            name_hash,
            unknown,
            parameter_offset,
            expression_count,
            expression_io_memory_size,
            multi_parameter_count,
            base_attachment_index,
            base_query_index,
            query_count,
            state_info_offset,
            guid,
            node_flags,
            queries: Vec::new(),
            attachments: serde_yaml::Value::Sequence(Vec::new()),
            properties: serde_yaml::Value::Mapping(Default::default()),
            parameters: serde_yaml::Value::Mapping(Default::default()),
            xlink_actions: serde_yaml::Value::Sequence(Vec::new()),
            plugs: serde_yaml::Value::Mapping(Default::default()),
        })
    }
}

fn node_type_name(value: u16) -> io::Result<&'static str> {
    match value {
        0 => Ok("UserDefined"),
        1 => Ok("Element_S32Selector"),
        2 => Ok("Element_Sequential"),
        3 => Ok("Element_Simultaneous"),
        4 => Ok("Element_F32Selector"),
        5 => Ok("Element_StringSelector"),
        6 => Ok("Element_RandomSelector"),
        7 => Ok("Element_BoolSelector"),
        8 => Ok("Element_Fork"),
        9 => Ok("Element_Join"),
        10 => Ok("Element_Alert"),
        20 => Ok("Element_Expression"),
        100 => Ok("Element_ModuleIF_Input_S32"),
        101 => Ok("Element_ModuleIF_Input_F32"),
        102 => Ok("Element_ModuleIF_Input_Vec3f"),
        103 => Ok("Element_ModuleIF_Input_String"),
        104 => Ok("Element_ModuleIF_Input_Bool"),
        105 => Ok("Element_ModuleIF_Input_Ptr"),
        200 => Ok("Element_ModuleIF_Output_S32"),
        201 => Ok("Element_ModuleIF_Output_F32"),
        202 => Ok("Element_ModuleIF_Output_Vec3f"),
        203 => Ok("Element_ModuleIF_Output_String"),
        204 => Ok("Element_ModuleIF_Output_Bool"),
        205 => Ok("Element_ModuleIF_Output_Ptr"),
        300 => Ok("Element_ModuleIF_Child"),
        400 => Ok("Element_StateEnd"),
        500 => Ok("Element_SplitTiming"),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported AINB node type {value}"),
        )),
    }
}
