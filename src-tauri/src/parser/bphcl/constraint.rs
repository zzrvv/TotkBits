use super::{BphclBuilder, BphclDocument, Item};
use crate::parser::physics_graph::PhysicsConstraintElement;
use std::io::{self, ErrorKind};

#[derive(Clone, Copy)]
enum PrimitiveKind {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    Real,
}

impl PrimitiveKind {
    fn size(self) -> u32 {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::Real => 4,
        }
    }
}

struct Member {
    offset: u32,
    kind: PrimitiveKind,
    particle: bool,
}

struct Layout {
    storage: Item,
    stride: u32,
    members: Vec<Member>,
}

pub(crate) fn read_elements(
    document: &BphclDocument,
    item_index: usize,
) -> io::Result<Vec<PhysicsConstraintElement>> {
    let layout = layout(document, item_index)?;
    let data = data(document)?;
    (0..layout.storage.count)
        .map(|index| {
            let base = checked_add(
                layout.storage.data_offset,
                index
                    .checked_mul(layout.stride)
                    .ok_or_else(|| invalid("constraint link offset overflow"))?,
            )?;
            let mut particles = Vec::new();
            let mut values = Vec::new();
            for member in &layout.members {
                let offset = checked_add(base, member.offset)?;
                if member.particle {
                    particles.push(read_integer(data, offset, member.kind)?);
                } else {
                    values.push(read_number(data, offset, member.kind)?);
                }
            }
            Ok(PhysicsConstraintElement { particles, values })
        })
        .collect()
}

pub(crate) fn write_elements(
    builder: &mut BphclBuilder<'_>,
    document: &BphclDocument,
    item_index: usize,
    elements: &[PhysicsConstraintElement],
) -> io::Result<()> {
    if elements.is_empty() {
        return Ok(());
    }
    let layout = layout(document, item_index)?;
    if elements.len() != layout.storage.count as usize {
        return Err(invalid(
            "HKCL constraint element count differs from template",
        ));
    }
    let particle_count = layout
        .members
        .iter()
        .filter(|member| member.particle)
        .count();
    let value_count = layout.members.len() - particle_count;
    for (index, element) in elements.iter().enumerate() {
        if element.particles.len() != particle_count || element.values.len() != value_count {
            return Err(invalid(
                "HKCL constraint member layout differs from BPHCL template",
            ));
        }
        let base = checked_add(
            layout.storage.data_offset,
            u32::try_from(index)
                .map_err(|_| invalid("constraint index exceeds u32"))?
                .checked_mul(layout.stride)
                .ok_or_else(|| invalid("constraint link offset overflow"))?,
        )?;
        let mut particle = 0;
        let mut value = 0;
        for member in &layout.members {
            let offset = checked_add(base, member.offset)?;
            if member.particle {
                write_integer(
                    &mut builder.data,
                    offset,
                    member.kind,
                    element.particles[particle],
                )?;
                particle += 1;
            } else {
                write_number(
                    &mut builder.data,
                    offset,
                    member.kind,
                    element.values[value],
                )?;
                value += 1;
            }
        }
    }
    Ok(())
}

fn layout(document: &BphclDocument, item_index: usize) -> io::Result<Layout> {
    let item = document
        .items
        .get(item_index)
        .ok_or_else(|| invalid("constraint ITEM is missing"))?;
    let class_name = document
        .type_table
        .type_name(item.type_index)
        .ok_or_else(|| invalid("constraint class name is missing"))?;
    let link_name = format!("{class_name}::Link");
    let link_type_index = (1..document.type_table.type_count() as u32)
        .find(|index| document.type_table.type_name(*index) == Some(link_name.as_str()))
        .ok_or_else(|| invalid("BPHCL constraint Link TYPE is missing"))?;
    let body = document
        .type_table
        .bodies
        .iter()
        .find(|body| body.type_index == link_type_index)
        .ok_or_else(|| invalid("BPHCL constraint Link body is missing"))?;
    let mut members = Vec::new();
    let mut end = 0;
    let mut alignment = 1;
    for member in &body.members {
        let name = document
            .type_table
            .field_strings
            .get(member.name_index as usize)
            .ok_or_else(|| invalid("BPHCL constraint member name is missing"))?;
        let kind = primitive_kind(&document.type_table, member.type_index)
            .ok_or_else(|| invalid("BPHCL constraint Link contains a non-primitive member"))?;
        end = end.max(
            member
                .offset
                .checked_add(kind.size())
                .ok_or_else(|| invalid("constraint Link size overflow"))?,
        );
        alignment = alignment.max(kind.size().min(4));
        members.push(Member {
            offset: member.offset,
            kind,
            particle: name.starts_with("particle"),
        });
    }
    if members.is_empty() {
        return Err(invalid("BPHCL constraint Link has no writable members"));
    }
    members.sort_by_key(|member| member.offset);
    let stride = body.size.unwrap_or_else(|| align(end, alignment));
    if stride < end {
        return Err(invalid("BPHCL constraint Link stride is too small"));
    }
    let storage_index = referenced(document, checked_add(item.data_offset, 40)?)?;
    let storage = document
        .items
        .get(storage_index)
        .cloned()
        .ok_or_else(|| invalid("BPHCL constraint Link array ITEM is missing"))?;
    Ok(Layout {
        storage,
        stride,
        members,
    })
}

fn primitive_kind(table: &super::TypeTable, index: u32) -> Option<PrimitiveKind> {
    match table.type_name(index)? {
        "hkUint8" | "hkBool" => Some(PrimitiveKind::U8),
        "hkInt8" => Some(PrimitiveKind::I8),
        "hkUint16" => Some(PrimitiveKind::U16),
        "hkInt16" => Some(PrimitiveKind::I16),
        "hkUint32" => Some(PrimitiveKind::U32),
        "hkInt32" => Some(PrimitiveKind::I32),
        "hkReal" => Some(PrimitiveKind::Real),
        _ => None,
    }
}

fn referenced(document: &BphclDocument, offset: u32) -> io::Result<usize> {
    if !document
        .patches
        .iter()
        .any(|patch| patch.offsets.contains(&offset))
    {
        return Err(invalid("BPHCL constraint array pointer is not patched"));
    }
    let value = read_u32(data(document)?, offset)?;
    let index = usize::try_from(value).map_err(|_| invalid("ITEM index exceeds usize"))?;
    if index >= document.items.len() {
        return Err(invalid("BPHCL constraint array ITEM index is invalid"));
    }
    Ok(index)
}

fn data(document: &BphclDocument) -> io::Result<&[u8]> {
    let section = document
        .tag
        .find("DATA")
        .ok_or_else(|| invalid("BPHCL has no DATA section"))?;
    Ok(&document.raw[section.payload_offset..section.payload_end()])
}

fn read_integer(data: &[u8], offset: u32, kind: PrimitiveKind) -> io::Result<u16> {
    let value = match kind {
        PrimitiveKind::U8 => read_bytes(data, offset, 1)?[0] as i64,
        PrimitiveKind::I8 => i8::from_le_bytes([read_bytes(data, offset, 1)?[0]]) as i64,
        PrimitiveKind::U16 => u16::from_le_bytes(read_array(data, offset)?) as i64,
        PrimitiveKind::I16 => i16::from_le_bytes(read_array(data, offset)?) as i64,
        PrimitiveKind::U32 => u32::from_le_bytes(read_array(data, offset)?) as i64,
        PrimitiveKind::I32 => i32::from_le_bytes(read_array(data, offset)?) as i64,
        PrimitiveKind::Real => {
            return Err(invalid(
                "particle index is stored as a floating-point value",
            ));
        }
    };
    u16::try_from(value).map_err(|_| invalid("constraint particle index exceeds u16"))
}

fn read_number(data: &[u8], offset: u32, kind: PrimitiveKind) -> io::Result<f32> {
    Ok(match kind {
        PrimitiveKind::U8 => read_bytes(data, offset, 1)?[0] as f32,
        PrimitiveKind::I8 => i8::from_le_bytes([read_bytes(data, offset, 1)?[0]]) as f32,
        PrimitiveKind::U16 => u16::from_le_bytes(read_array(data, offset)?) as f32,
        PrimitiveKind::I16 => i16::from_le_bytes(read_array(data, offset)?) as f32,
        PrimitiveKind::U32 => u32::from_le_bytes(read_array(data, offset)?) as f32,
        PrimitiveKind::I32 => i32::from_le_bytes(read_array(data, offset)?) as f32,
        PrimitiveKind::Real => f32::from_le_bytes(read_array(data, offset)?),
    })
}

fn write_integer(data: &mut [u8], offset: u32, kind: PrimitiveKind, value: u16) -> io::Result<()> {
    match kind {
        PrimitiveKind::U8 => write_bytes(
            data,
            offset,
            &[u8::try_from(value).map_err(|_| invalid("particle index exceeds u8"))?],
        ),
        PrimitiveKind::I8 => write_bytes(
            data,
            offset,
            &i8::try_from(value)
                .map_err(|_| invalid("particle index exceeds i8"))?
                .to_le_bytes(),
        ),
        PrimitiveKind::U16 => write_bytes(data, offset, &value.to_le_bytes()),
        PrimitiveKind::I16 => write_bytes(
            data,
            offset,
            &i16::try_from(value)
                .map_err(|_| invalid("particle index exceeds i16"))?
                .to_le_bytes(),
        ),
        PrimitiveKind::U32 => write_bytes(data, offset, &u32::from(value).to_le_bytes()),
        PrimitiveKind::I32 => write_bytes(data, offset, &i32::from(value).to_le_bytes()),
        PrimitiveKind::Real => Err(invalid(
            "particle index cannot be written to a floating-point member",
        )),
    }
}

fn write_number(data: &mut [u8], offset: u32, kind: PrimitiveKind, value: f32) -> io::Result<()> {
    if !value.is_finite() {
        return Err(invalid("constraint value is not finite"));
    }
    match kind {
        PrimitiveKind::Real => write_bytes(data, offset, &value.to_le_bytes()),
        PrimitiveKind::U8 => write_bytes(data, offset, &(value as u8).to_le_bytes()),
        PrimitiveKind::I8 => write_bytes(data, offset, &(value as i8).to_le_bytes()),
        PrimitiveKind::U16 => write_bytes(data, offset, &(value as u16).to_le_bytes()),
        PrimitiveKind::I16 => write_bytes(data, offset, &(value as i16).to_le_bytes()),
        PrimitiveKind::U32 => write_bytes(data, offset, &(value as u32).to_le_bytes()),
        PrimitiveKind::I32 => write_bytes(data, offset, &(value as i32).to_le_bytes()),
    }
}

fn read_u32(data: &[u8], offset: u32) -> io::Result<u32> {
    Ok(u32::from_le_bytes(read_array(data, offset)?))
}

fn read_array<const N: usize>(data: &[u8], offset: u32) -> io::Result<[u8; N]> {
    let mut bytes = [0; N];
    bytes.copy_from_slice(read_bytes(data, offset, N)?);
    Ok(bytes)
}

fn read_bytes(data: &[u8], offset: u32, size: usize) -> io::Result<&[u8]> {
    let offset = offset as usize;
    data.get(offset..offset + size)
        .ok_or_else(|| invalid("constraint read exceeds DATA"))
}

fn write_bytes(data: &mut [u8], offset: u32, bytes: &[u8]) -> io::Result<()> {
    let offset = offset as usize;
    data.get_mut(offset..offset + bytes.len())
        .ok_or_else(|| invalid("constraint write exceeds DATA"))?
        .copy_from_slice(bytes);
    Ok(())
}

fn checked_add(value: u32, addition: u32) -> io::Result<u32> {
    value
        .checked_add(addition)
        .ok_or_else(|| invalid("DATA offset overflow"))
}

fn align(value: u32, alignment: u32) -> u32 {
    (value + alignment - 1) / alignment * alignment
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message)
}
