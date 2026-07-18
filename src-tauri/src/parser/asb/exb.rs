use super::{
    command_info::CommandInfo, data_type::DataType, instruction::Instruction, opcode::Opcode,
    source::Source, value::ExbValue,
};
use crate::parser::binary::{BinaryReader, BinaryWriter};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExbInfo {
    #[serde(rename = "Magic")]
    pub magic: String,
    #[serde(rename = "Version")]
    pub version: u32,
    #[serde(rename = "Static Memory Size")]
    pub static_memory_size: u32,
    #[serde(rename = "Instance Count")]
    pub instance_count: u32,
    #[serde(rename = "Scratch32 Size")]
    pub scratch32_size: u32,
    #[serde(rename = "Scratch64 Size")]
    pub scratch64_size: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Exb {
    #[serde(rename = "Info")]
    pub info: ExbInfo,
    #[serde(rename = "Commands")]
    pub commands: Vec<CommandInfo>,
}

impl Exb {
    pub fn from_bytes(data: &[u8]) -> io::Result<Self> {
        let mut r = BinaryReader::new(data);
        if r.read_bytes(4)? != b"EXB " {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid EXB magic",
            ));
        }
        let version = r.read_u32()?;
        if !matches!(version, 1 | 2) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported EXB version {version}"),
            ));
        }
        let static_memory_size = r.read_u32()?;
        let instance_count = r.read_u32()?;
        let scratch32_size = r.read_u32()?;
        let scratch64_size = r.read_u32()?;
        let info_offset = r.read_u32()? as usize;
        let table_offset = r.read_u32()? as usize;
        let signature_offset = r.read_u32()? as usize;
        let parameter_offset = r.read_u32()? as usize;
        let string_offset = r.read_u32()? as usize;
        r.seek(signature_offset)?;
        let signature_count = r.read_u32()?;
        let mut signatures = Vec::new();
        for _ in 0..signature_count {
            signatures.push(r.read_u32()? as usize);
        }
        r.seek(info_offset)?;
        let command_count = r.read_u32()?;
        let mut metadata = Vec::new();
        for _ in 0..command_count {
            let base = r.read_i32()?;
            let pre = if version == 2 {
                Some(r.read_u32()?)
            } else {
                None
            };
            let base_instruction = r.read_u32()? as usize;
            let count = if version == 2 {
                Some(r.read_u32()? as usize)
            } else {
                None
            };
            r.read_u32()?;
            r.read_u16()?;
            r.read_u16()?;
            let output = DataType::from_u16(r.read_u16()?)?;
            let input = DataType::from_u16(r.read_u16()?)?;
            metadata.push((base, pre, base_instruction, count, output, input));
        }
        r.seek(table_offset)?;
        let instruction_count = r.read_u32()?;
        let mut instructions = Vec::new();
        for _ in 0..instruction_count {
            instructions.push(read_instruction(
                &mut r,
                data,
                parameter_offset,
                string_offset,
                &signatures,
            )?);
        }
        let mut sequential = 0;
        let mut commands = Vec::new();
        for (base, pre, start, count, output, input) in metadata {
            let list = if let Some(count) = count {
                instructions
                    .get(start..start + count)
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "EXB instruction range exceeds table",
                        )
                    })?
                    .to_vec()
            } else {
                let begin = sequential;
                while sequential < instructions.len() {
                    let end = instructions[sequential].opcode == Opcode::Terminator;
                    sequential += 1;
                    if end {
                        break;
                    }
                }
                instructions[begin..sequential].to_vec()
            };
            commands.push(CommandInfo {
                base_index_pre_command_entry: base,
                pre_entry_static_memory_usage: pre,
                output_data_type: output,
                input_data_type: input,
                instructions: list,
            });
        }
        Ok(Self {
            info: ExbInfo {
                magic: "EXB ".into(),
                version,
                static_memory_size,
                instance_count,
                scratch32_size,
                scratch64_size,
            },
            commands,
        })
    }

    pub fn to_bytes(&self, instance_count: u32) -> io::Result<Vec<u8>> {
        let mut w = BinaryWriter::new();
        w.write_bytes(b"EXB ");
        w.write_u32(2);
        for _ in 0..9 {
            w.write_u32(0);
        }
        let info_offset = w.position() as u32;
        w.write_u32(self.commands.len() as u32);
        let mut flat = Vec::new();
        let mut max_static = 0;
        let mut max32 = 0;
        let mut max64 = 0;
        for command in &self.commands {
            let start = flat.len() as u32;
            flat.extend(command.instructions.clone());
            let (static_size, s32, s64) = memory_sizes(&command.instructions);
            max_static = max_static.max(static_size);
            max32 = max32.max(s32);
            max64 = max64.max(s64);
            w.write_i32(command.base_index_pre_command_entry);
            w.write_u32(command.pre_entry_static_memory_usage.unwrap_or(0));
            w.write_u32(start);
            w.write_u32(command.instructions.len() as u32);
            w.write_u32(static_size);
            w.write_u16(s32 as u16);
            w.write_u16(s64 as u16);
            w.write_u16(command.output_data_type.as_u16());
            w.write_u16(command.input_data_type.as_u16());
        }
        let table_offset = w.position() as u32;
        w.write_u32(flat.len() as u32);
        let mut signatures = Vec::<String>::new();
        let mut strings = BTreeMap::<String, u32>::new();
        let mut pending_strings = Vec::<String>::new();
        for instruction in &flat {
            write_instruction(&mut w, instruction, &mut signatures, &mut pending_strings)?;
        }
        let signature_offset = w.position() as u32;
        w.write_u32(signatures.len() as u32);
        let signature_slots = w.position();
        for _ in &signatures {
            w.write_u32(0);
        }
        let parameter_offset = w.position() as u32;
        let mut parameter_end = parameter_offset as usize;
        for instruction in &flat {
            for (source, index, value) in [
                (
                    instruction.lhs_source,
                    instruction.lhs_index,
                    instruction.lhs_value.as_ref(),
                ),
                (
                    instruction.rhs_source,
                    instruction.rhs_index,
                    instruction.rhs_value.as_ref(),
                ),
            ] {
                if let (Some(source), Some(index), Some(value)) = (source, index, value) {
                    if matches!(source, Source::ParamTbl | Source::ParamTblStr) {
                        w.seek(parameter_offset as usize + index as usize);
                        write_value(&mut w, value, source, &mut pending_strings)?;
                        parameter_end = parameter_end.max(w.position());
                    }
                }
            }
        }
        w.seek(parameter_end);
        let string_offset = w.position() as u32;
        for value in pending_strings
            .into_iter()
            .chain(signatures.iter().cloned())
        {
            if !strings.contains_key(&value) {
                let offset = (w.position() as u32) - string_offset;
                strings.insert(value.clone(), offset);
                w.write_c_string(&value);
            }
        }
        for (i, signature) in signatures.iter().enumerate() {
            w.seek(signature_slots + i * 4);
            w.write_u32(*strings.get(signature).unwrap());
        }
        w.seek(8);
        w.write_u32(max_static);
        w.write_u32(instance_count);
        w.write_u32(max32);
        w.write_u32(max64);
        w.write_u32(info_offset);
        w.write_u32(table_offset);
        w.write_u32(signature_offset);
        w.write_u32(parameter_offset);
        w.write_u32(string_offset);
        Ok(w.into_inner())
    }
}

fn read_instruction(
    r: &mut BinaryReader<'_>,
    data: &[u8],
    parameter: usize,
    strings: usize,
    signatures: &[usize],
) -> io::Result<Instruction> {
    let opcode = Opcode::from_u8(r.read_u8()?)?;
    if opcode == Opcode::Terminator {
        r.skip(7)?;
        return Ok(Instruction::terminator());
    }
    let data_type = DataType::from_u16(r.read_u8()? as u16)?;
    if opcode == Opcode::UserFunction {
        let index = r.read_u16()?;
        let signature_index = r.read_u32()? as usize;
        let signature_offset = *signatures.get(signature_index).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "EXB signature index exceeds table",
            )
        })?;
        return Ok(Instruction {
            opcode,
            data_type: Some(data_type),
            lhs_source: None,
            rhs_source: None,
            lhs_index: None,
            rhs_index: None,
            lhs_value: None,
            rhs_value: None,
            static_memory_index: Some(index),
            signature: Some(read_string(data, strings + signature_offset)?),
        });
    }
    let lhs_source = Source::from_u8(r.read_u8()?)?;
    let rhs_source = Source::from_u8(r.read_u8()?)?;
    let lhs_index = r.read_u16()?;
    let rhs_index = r.read_u16()?;
    let lhs_value = read_operand(data, parameter, strings, data_type, lhs_source, lhs_index)?;
    let rhs_value = read_operand(data, parameter, strings, data_type, rhs_source, rhs_index)?;
    Ok(Instruction {
        opcode,
        data_type: Some(data_type),
        lhs_source: Some(lhs_source),
        rhs_source: Some(rhs_source),
        lhs_index: Some(lhs_index),
        rhs_index: Some(rhs_index),
        lhs_value,
        rhs_value,
        static_memory_index: None,
        signature: None,
    })
}
fn read_operand(
    data: &[u8],
    parameter: usize,
    strings: usize,
    ty: DataType,
    source: Source,
    index: u16,
) -> io::Result<Option<ExbValue>> {
    let mut r = BinaryReader::new(data);
    Ok(match source {
        Source::Imm => Some(ExbValue::Integer(index as u32)),
        Source::ImmStr => Some(ExbValue::String(read_string(
            data,
            strings + index as usize,
        )?)),
        Source::ParamTbl => {
            r.seek(parameter + index as usize)?;
            Some(match ty {
                DataType::Bool => ExbValue::Bool(r.read_u32()? != 0),
                DataType::S32 => ExbValue::Integer(r.read_u32()?),
                DataType::F32 => ExbValue::Float(r.read_f32()?),
                DataType::Vec3f => ExbValue::Vec3f([r.read_f32()?, r.read_f32()?, r.read_f32()?]),
                _ => return Ok(None),
            })
        }
        Source::ParamTblStr => {
            r.seek(parameter + index as usize)?;
            Some(ExbValue::String(read_string(
                data,
                strings + r.read_u32()? as usize,
            )?))
        }
        _ => None,
    })
}
fn read_string(data: &[u8], offset: usize) -> io::Result<String> {
    BinaryReader::new(data).read_c_string_at(offset)
}
fn write_instruction(
    w: &mut BinaryWriter,
    i: &Instruction,
    signatures: &mut Vec<String>,
    strings: &mut Vec<String>,
) -> io::Result<()> {
    w.write_u8(i.opcode.as_u8());
    if i.opcode == Opcode::Terminator {
        w.write_bytes(&[0; 7]);
        return Ok(());
    }
    w.write_u8(i.data_type.unwrap_or(DataType::None).as_u16() as u8);
    if i.opcode == Opcode::UserFunction {
        w.write_u16(i.static_memory_index.unwrap_or(0));
        let signature = i.signature.clone().unwrap_or_default();
        let index = if let Some(index) = signatures.iter().position(|v| v == &signature) {
            index
        } else {
            signatures.push(signature.clone());
            signatures.len() - 1
        };
        strings.push(signature);
        w.write_u32(index as u32);
    } else {
        w.write_u8(i.lhs_source.unwrap_or(Source::Imm).as_u8());
        w.write_u8(i.rhs_source.unwrap_or(Source::Imm).as_u8());
        w.write_u16(i.lhs_index.unwrap_or(0));
        w.write_u16(i.rhs_index.unwrap_or(0));
        if let Some(ExbValue::String(v)) = &i.lhs_value {
            strings.push(v.clone())
        }
        if let Some(ExbValue::String(v)) = &i.rhs_value {
            strings.push(v.clone())
        }
    }
    Ok(())
}
fn write_value(
    w: &mut BinaryWriter,
    value: &ExbValue,
    source: Source,
    strings: &mut Vec<String>,
) -> io::Result<()> {
    match value {
        ExbValue::Bool(v) => w.write_u32(u32::from(*v)),
        ExbValue::Integer(v) => w.write_u32(*v),
        ExbValue::Float(v) => w.write_f32(*v),
        ExbValue::Vec3f(v) => {
            for x in v {
                w.write_f32(*x)
            }
        }
        ExbValue::String(v) => {
            strings.push(v.clone());
            if source == Source::ParamTblStr {
                w.write_u32(0)
            }
        }
    }
    Ok(())
}
fn memory_sizes(instructions: &[Instruction]) -> (u32, u32, u32) {
    let mut a = 0;
    let mut b = 0;
    let mut c = 0;
    for i in instructions {
        let size = i.data_type.unwrap_or(DataType::None).byte_size();
        for (source, index) in [(i.lhs_source, i.lhs_index), (i.rhs_source, i.rhs_index)] {
            if let (Some(source), Some(index)) = (source, index) {
                let end = index as u32 + size;
                match source {
                    Source::StaticMem => a = a.max(end),
                    Source::Scratch32 => b = b.max(end),
                    Source::Scratch64 => c = c.max(end),
                    _ => {}
                }
            }
        }
        if let Some(index) = i.static_memory_index {
            a = a.max(index as u32 + size)
        }
    }
    (a, b, c)
}
