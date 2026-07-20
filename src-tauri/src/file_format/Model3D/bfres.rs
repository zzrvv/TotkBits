//! Read-only parser for Nintendo BFRES resource containers.
//!
//! BFRES stores its object graph as relocated pointers.  This module parses the
//! container header and inventories the typed resource sections without tying
//! the result to TotkBits' document/YAML representation.

use serde::Serialize;
use std::{fmt, fs, path::Path};

const SECTION_SIGNATURES: &[&[u8; 4]] = &[
    b"FMDL", b"FSKL", b"FVTX", b"FSHP", b"FMAT", b"FSKA", b"FSHU", b"FSHA", b"FSCN", b"FTXP",
    b"FVIS", b"FMAA", b"FREL",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Endian {
    Little,
    Big,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BfresHeader {
    pub version: [u8; 4],
    pub endian: Endian,
    pub alignment_exponent: u8,
    pub target_address_size: u8,
    pub name_offset: u32,
    pub flags: u16,
    pub block_offset: u16,
    pub relocation_table_offset: u32,
    /// Size used by BFRES itself. Files are commonly padded beyond this value.
    pub file_size: u32,
    pub string_pool_size: u32,
    pub string_pool_offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BfresSection {
    pub signature: [u8; 4],
    pub offset: u64,
    pub name: Option<String>,
}

impl BfresSection {
    pub fn signature_str(&self) -> &str {
        std::str::from_utf8(&self.signature).unwrap_or("????")
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BfresFile {
    pub header: BfresHeader,
    pub name: Option<String>,
    pub sections: Vec<BfresSection>,
    pub materials: Vec<super::bfmat::BfresMaterial>,
    pub render: BfresRenderGraph,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct BfresRenderGraph {
    pub bones: Vec<BfresBone>,
    pub matrix_to_bone: Vec<u16>,
    pub meshes: Vec<BfresMesh>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BfresBone {
    pub name: String,
    pub parent_index: i16,
    pub smooth_matrix_index: i16,
    pub rigid_matrix_index: i16,
    pub scale: [f32; 3],
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BfresMesh {
    pub name: String,
    pub material_index: u16,
    pub bone_index: u16,
    pub vertex_skin_count: u8,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uv0: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub bone_indices: Vec<[u16; 4]>,
    pub bone_weights: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    pub skin_bones: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BfresError {
    pub offset: usize,
    pub message: String,
}

impl BfresError {
    fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }
}

impl fmt::Display for BfresError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BFRES error at 0x{:X}: {}", self.offset, self.message)
    }
}

impl std::error::Error for BfresError {}

impl BfresFile {
    pub fn open(
        path: &std::path::Path,
    ) -> Option<(
        crate::file_format::BinTextFile::OpenedFile<'static>,
        crate::Open_and_Save::SendData,
    )> {
        if !path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("bfres"))
        {
            return None;
        }
        let file = Self::from_path(path).ok()?;
        let mut opened = crate::file_format::BinTextFile::OpenedFile::default();
        opened.file_type = crate::Zstd::TotkFileType::Bfres;
        opened.path = crate::Settings::Pathlib::new(path);
        opened.bfres = Some(file);

        let mut data = crate::Open_and_Save::SendData::default();
        data.path = crate::Settings::Pathlib::new(path);
        data.file_label = format!("{} [BFRES]", data.path.name);
        data.file_metadata = "[BFRES] [3D]".into();
        data.status_text = format!("Opened BFRES {}", path.display());
        data.tab = "3D".into();
        data.read_only = true;
        Some((opened, data))
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, BfresError> {
        let data = fs::read(path.as_ref())
            .map_err(|error| BfresError::new(0, format!("failed to read file: {error}")))?;
        Self::from_bytes(&data)
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, BfresError> {
        if data.len() < 0x30 {
            return Err(BfresError::new(0, "header is truncated"));
        }
        if &data[..4] != b"FRES" {
            return Err(BfresError::new(0, "invalid FRES signature"));
        }

        let endian = match &data[0x0C..0x0E] {
            [0xFF, 0xFE] => Endian::Little,
            [0xFE, 0xFF] => Endian::Big,
            _ => return Err(BfresError::new(0x0C, "invalid byte-order mark")),
        };
        let read_u16 = |offset| u16_at(data, offset, endian);
        let read_u32 = |offset| u32_at(data, offset, endian);

        let header = BfresHeader {
            version: data[8..12].try_into().unwrap(),
            endian,
            alignment_exponent: data[0x0E],
            target_address_size: data[0x0F],
            name_offset: read_u32(0x10)?,
            flags: read_u16(0x14)?,
            block_offset: read_u16(0x16)?,
            relocation_table_offset: read_u32(0x18)?,
            file_size: read_u32(0x1C)?,
            string_pool_size: read_u32(0x20)?,
            string_pool_offset: read_u32(0x24)?,
        };

        if header.file_size as usize > data.len() {
            return Err(BfresError::new(0x1C, "declared file size exceeds input"));
        }
        if header.target_address_size != 0
            && header.target_address_size != 4
            && header.target_address_size != 8
        {
            return Err(BfresError::new(0x0F, "unsupported target address size"));
        }

        let name = read_string(data, header.name_offset as u64);
        let mut sections = Vec::new();
        for offset in (0..data.len().saturating_sub(3)).filter(|offset| offset % 4 == 0) {
            let signature: [u8; 4] = data[offset..offset + 4].try_into().unwrap();
            if !SECTION_SIGNATURES
                .iter()
                .any(|candidate| **candidate == signature)
            {
                continue;
            }
            // All resource sections begin with their signature, four reserved
            // bytes, then an absolute pointer to their ResString name. FREL is
            // the sole unnamed container-level section.
            let section_name = if &signature == b"FREL" {
                None
            } else if header.target_address_size != 4 {
                u64_at(data, offset + 8, endian)
                    .ok()
                    .and_then(|pointer| read_string(data, pointer))
            } else {
                u32_at(data, offset + 8, endian)
                    .ok()
                    .and_then(|pointer| read_string(data, pointer as u64))
            };
            sections.push(BfresSection {
                signature,
                offset: offset as u64,
                name: section_name,
            });
        }

        if sections.is_empty() {
            return Err(BfresError::new(0, "no BFRES resource sections found"));
        }
        let materials = super::bfmat::parse_materials(data, &sections, endian, header.version[2]);
        let render = parse_render_graph(data, endian, header.file_size as usize, &sections)?;
        Ok(Self {
            header,
            name,
            sections,
            materials,
            render,
        })
    }

    pub fn sections_with_signature<'a>(
        &'a self,
        signature: &'a [u8; 4],
    ) -> impl Iterator<Item = &'a BfresSection> + 'a {
        self.sections
            .iter()
            .filter(move |section| &section.signature == signature)
    }
}

#[derive(Clone)]
struct VertexStream {
    vertex_count: usize,
    attributes: Vec<VertexAttribute>,
    buffers: Vec<(usize, Vec<u8>)>,
}

#[derive(Clone)]
struct VertexAttribute {
    name: String,
    format: u16,
    offset: usize,
    buffer_index: usize,
}

fn parse_render_graph(
    data: &[u8],
    endian: Endian,
    logical_file_size: usize,
    sections: &[BfresSection],
) -> Result<BfresRenderGraph, BfresError> {
    if endian != Endian::Little {
        return Ok(BfresRenderGraph::default());
    }
    let buffer_info = u64_at(data, 0xB0, endian)? as usize;
    let external_flags = byte_at(data, 0xEE).unwrap_or(0);
    let buffer_base = if buffer_info != 0 && buffer_info + 16 <= data.len() {
        u64_at(data, buffer_info + 8, endian)? as usize
    } else if external_flags & 1 != 0 {
        logical_file_size + 288
    } else {
        return Ok(BfresRenderGraph::default());
    };
    let mut streams = std::collections::HashMap::new();
    for section in sections
        .iter()
        .filter(|section| &section.signature == b"FVTX")
    {
        let offset = section.offset as usize;
        if let Ok(stream) = parse_vertex_stream(data, offset, buffer_base, endian) {
            streams.insert(offset, stream);
        }
    }
    let (bones, matrix_to_bone) = sections
        .iter()
        .find(|section| &section.signature == b"FSKL")
        .map(|section| super::bfskl::parse_skeleton(data, section.offset as usize, endian))
        .transpose()?
        .unwrap_or_default();
    let mut meshes = Vec::new();
    for section in sections
        .iter()
        .filter(|section| &section.signature == b"FSHP")
    {
        let mut parsed = parse_shape(
            data,
            section,
            buffer_base,
            endian,
            &streams,
            &matrix_to_bone,
        )?;
        meshes.append(&mut parsed);
    }
    let mut names = std::collections::HashMap::<String, usize>::new();
    for mesh in &mut meshes {
        let count = names.entry(mesh.name.clone()).or_default();
        if *count > 0 {
            mesh.name = format!("{} ({})", mesh.name, *count + 1);
        }
        *count += 1;
    }
    Ok(BfresRenderGraph {
        bones,
        matrix_to_bone,
        meshes,
    })
}

fn parse_vertex_stream(
    data: &[u8],
    offset: usize,
    buffer_base: usize,
    endian: Endian,
) -> Result<VertexStream, BfresError> {
    let attr_offset = u64_at(data, offset + 8, endian)? as usize;
    let sizes_offset = u64_at(data, offset + 48, endian)? as usize;
    let strides_offset = u64_at(data, offset + 56, endian)? as usize;
    let relative_buffer = u32_at(data, offset + 72, endian)? as usize;
    let attr_count = byte_at(data, offset + 76)? as usize;
    let buffer_count = byte_at(data, offset + 77)? as usize;
    let vertex_count = u32_at(data, offset + 80, endian)? as usize;
    let alignment = u16_at(data, offset + 86, endian)? as usize;
    let mut attributes = Vec::with_capacity(attr_count);
    for index in 0..attr_count {
        let entry = attr_offset + index * 16;
        attributes.push(VertexAttribute {
            name: read_string(data, u64_at(data, entry, endian)?)
                .unwrap_or_else(|| format!("attribute_{index}")),
            format: u16::from_be_bytes(
                data.get(entry + 8..entry + 10)
                    .ok_or_else(|| BfresError::new(entry, "truncated vertex attribute"))?
                    .try_into()
                    .unwrap(),
            ),
            offset: u16_at(data, entry + 12, endian)? as usize,
            buffer_index: byte_at(data, entry + 14)? as usize,
        });
    }
    let mut buffers = Vec::with_capacity(buffer_count);
    let mut cursor = buffer_base
        .checked_add(relative_buffer)
        .ok_or_else(|| BfresError::new(offset, "vertex buffer offset overflow"))?;
    for index in 0..buffer_count {
        cursor = align_up(cursor, alignment.max(1));
        let size = u32_at(data, sizes_offset + index * 16, endian)? as usize;
        let stride = u32_at(data, strides_offset + index * 16, endian)? as usize;
        let bytes = data
            .get(cursor..cursor + size)
            .ok_or_else(|| BfresError::new(cursor, "vertex buffer exceeds file"))?
            .to_vec();
        buffers.push((stride, bytes));
        cursor += size;
    }
    Ok(VertexStream {
        vertex_count,
        attributes,
        buffers,
    })
}

fn parse_shape(
    data: &[u8],
    section: &BfresSection,
    buffer_base: usize,
    endian: Endian,
    streams: &std::collections::HashMap<usize, VertexStream>,
    _matrix_to_bone: &[u16],
) -> Result<Vec<BfresMesh>, BfresError> {
    let offset = section.offset as usize;
    let vertex_offset = u64_at(data, offset + 16, endian)? as usize;
    let mesh_offset = u64_at(data, offset + 24, endian)? as usize;
    let skin_offset = u64_at(data, offset + 32, endian)? as usize;
    let material_index = u16_at(data, offset + 82, endian)?;
    let bone_index = u16_at(data, offset + 84, endian)?;
    let skin_count = u16_at(data, offset + 88, endian)? as usize;
    let vertex_skin_count = byte_at(data, offset + 90)?;
    let mesh_count = byte_at(data, offset + 91)? as usize;
    let stream = streams
        .get(&vertex_offset)
        .ok_or_else(|| BfresError::new(offset + 16, "shape references unknown FVTX"))?;
    let skin_bones = if skin_count == 0 {
        Vec::new()
    } else {
        (0..skin_count)
            .map(|i| u16_at(data, skin_offset + i * 2, endian))
            .collect::<Result<Vec<_>, _>>()?
    };
    let positions: Vec<[f32; 3]> = decode_attribute(stream, data, "_p")?
        .into_iter()
        .map(|v| [v[0], v[1], v[2]])
        .collect();
    let normals: Vec<[f32; 3]> = decode_attribute(stream, data, "_n")
        .unwrap_or_default()
        .into_iter()
        .map(|v| [v[0], v[1], v[2]])
        .collect();
    let uv0: Vec<[f32; 2]> = decode_attribute(stream, data, "_u0")
        .unwrap_or_default()
        .into_iter()
        .map(|v| [v[0], v[1]])
        .collect();
    let colors = decode_attribute(stream, data, "_c0").unwrap_or_default();

    // BFRES vertex bone indices are shape-local indices into FSHP::SkinBoneIndices.
    // They are NOT indices into FSKL::MatrixToBone. Mapping through matrix_to_bone
    // here assigns vertices to unrelated bones and produces the classic exploded
    // mesh seen when the model is skinned.
    let raw_indices = decode_attribute(stream, data, "_i").unwrap_or_default();
    let mut bone_indices: Vec<[u16; 4]> = raw_indices
        .into_iter()
        .map(|v| {
            let map = |value: f32| -> u16 {
                if !value.is_finite() || value < 0.0 {
                    return bone_index;
                }
                let local_index = value.round() as usize;
                skin_bones.get(local_index).copied().unwrap_or(bone_index)
            };
            [map(v[0]), map(v[1]), map(v[2]), map(v[3])]
        })
        .collect();

    let mut bone_weights = decode_attribute(stream, data, "_w").unwrap_or_default();

    // Rigid shapes have no per-vertex skin attributes. Bind every vertex to the
    // shape's rigid bone instead of leaving empty/zero skinning data.
    if vertex_skin_count == 0 || skin_bones.is_empty() {
        bone_indices = vec![[bone_index, bone_index, bone_index, bone_index]; stream.vertex_count];
        bone_weights = vec![[1.0, 0.0, 0.0, 0.0]; stream.vertex_count];
    } else {
        // Missing weights are valid for one-bone skinning in some files.
        if bone_weights.len() != stream.vertex_count {
            bone_weights = vec![[1.0, 0.0, 0.0, 0.0]; stream.vertex_count];
        }

        // Normalise and sanitise weights. Small quantisation errors are common,
        // but NaN/negative/zero-sum weights must never reach the renderer.
        for weights in &mut bone_weights {
            for weight in weights.iter_mut() {
                if !weight.is_finite() || *weight < 0.0 {
                    *weight = 0.0;
                }
            }
            let sum: f32 = weights.iter().sum();
            if sum > f32::EPSILON {
                for weight in weights.iter_mut() {
                    *weight /= sum;
                }
            } else {
                *weights = [1.0, 0.0, 0.0, 0.0];
            }
        }
    }
    let mut result = Vec::new();
    // Mesh entries are ordered from highest to lowest detail. Rendering only
    // the first avoids drawing every LOD on top of the same shape.
    for mesh_index in 0..mesh_count.min(1) {
        let entry = mesh_offset + mesh_index * 56;
        let size_offset = u64_at(data, entry + 24, endian)? as usize;
        let face_offset = u32_at(data, entry + 32, endian)? as usize;
        let primitive = u32_at(data, entry + 36, endian)?;
        let format = u32_at(data, entry + 40, endian)?;
        let index_count = u32_at(data, entry + 44, endian)? as usize;
        // FSHP Mesh indices are relative to this base vertex. Omitting it makes
        // otherwise valid local indices reference vertices from another region
        // of the shared FVTX stream, producing long triangles/spikes.
        let first_vertex = u32_at(data, entry + 48, endian)?;
        let byte_size = u32_at(data, size_offset, endian)? as usize;
        let raw = data
            .get(buffer_base + face_offset..buffer_base + face_offset + byte_size)
            .ok_or_else(|| {
                BfresError::new(buffer_base + face_offset, "index buffer exceeds file")
            })?;
        let indices = decode_indices(raw, format, index_count, endian, primitive)?
            .chunks_exact(3)
            .filter_map(|triangle| {
                let triangle = [
                    triangle[0].checked_add(first_vertex)?,
                    triangle[1].checked_add(first_vertex)?,
                    triangle[2].checked_add(first_vertex)?,
                ];
                triangle
                    .iter()
                    .all(|index| *index < stream.vertex_count as u32)
                    .then_some(triangle)
            })
            .flatten()
            .collect();
        result.push(BfresMesh {
            name: if mesh_count > 1 {
                format!(
                    "{} #{mesh_index}",
                    section.name.as_deref().unwrap_or("Shape")
                )
            } else {
                section
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("Shape_{mesh_index}"))
            },
            material_index,
            bone_index,
            vertex_skin_count,
            positions: positions.clone(),
            normals: normals.clone(),
            uv0: uv0.clone(),
            colors: colors.clone(),
            bone_indices: bone_indices.clone(),
            bone_weights: bone_weights.clone(),
            indices,
            skin_bones: skin_bones.clone(),
        });
    }
    Ok(result)
}

fn decode_attribute(
    stream: &VertexStream,
    _data: &[u8],
    prefix: &str,
) -> Result<Vec<[f32; 4]>, BfresError> {
    let attribute = stream
        .attributes
        .iter()
        .find(|attribute| attribute.name == prefix || attribute.name.starts_with(prefix))
        .ok_or_else(|| BfresError::new(0, format!("missing {prefix} vertex attribute")))?;
    let (stride, bytes) = stream
        .buffers
        .get(attribute.buffer_index)
        .ok_or_else(|| BfresError::new(0, "vertex attribute buffer index is invalid"))?;
    (0..stream.vertex_count)
        .map(|index| {
            decode_vertex_value(bytes, index * *stride + attribute.offset, attribute.format)
        })
        .collect()
}

fn decode_vertex_value(data: &[u8], offset: usize, format: u16) -> Result<[f32; 4], BfresError> {
    let b = |i| byte_at(data, offset + i);
    let u = |i| u16_at(data, offset + i, Endian::Little);
    let s = |i| i16_at(data, offset + i, Endian::Little);
    let f = |i| f32_at(data, offset + i, Endian::Little);
    Ok(match format {
        0x020E => {
            let packed = u32_at(data, offset, Endian::Little)?;
            let signed = |shift: u32| (((packed >> shift) & 0x3ff) as i32) << 22 >> 22;
            [
                signed(0) as f32 / 511.0,
                signed(10) as f32 / 511.0,
                signed(20) as f32 / 511.0,
                ((packed >> 30) & 3) as f32,
            ]
        }
        0x0518 => [f(0)?, f(4)?, f(8)?, 1.0],
        0x0519 => [f(0)?, f(4)?, f(8)?, f(12)?],
        0x0517 => [f(0)?, f(4)?, 0.0, 1.0],
        0x0516 => [f(0)?, 0.0, 0.0, 1.0],
        0x0512 => [half(u(0)?), half(u(2)?), 0.0, 1.0],
        0x0515 => [half(u(0)?), half(u(2)?), half(u(4)?), half(u(6)?)],
        0x010B => [
            b(0)? as f32 / 255.0,
            b(1)? as f32 / 255.0,
            b(2)? as f32 / 255.0,
            b(3)? as f32 / 255.0,
        ],
        0x020B => [
            b(0)? as i8 as f32 / 127.0,
            b(1)? as i8 as f32 / 127.0,
            b(2)? as i8 as f32 / 127.0,
            b(3)? as i8 as f32 / 127.0,
        ],
        0x030B => [b(0)? as f32, b(1)? as f32, b(2)? as f32, b(3)? as f32],
        0x0115 => [
            u(0)? as f32 / 65535.0,
            u(2)? as f32 / 65535.0,
            u(4)? as f32 / 65535.0,
            u(6)? as f32 / 65535.0,
        ],
        0x0215 => [
            s(0)? as f32 / 32767.0,
            s(2)? as f32 / 32767.0,
            s(4)? as f32 / 32767.0,
            s(6)? as f32 / 32767.0,
        ],
        0x0315 => [u(0)? as f32, u(2)? as f32, u(4)? as f32, u(6)? as f32],
        0x0109 => [b(0)? as f32 / 255.0, b(1)? as f32 / 255.0, 0.0, 1.0],
        0x0309 => [b(0)? as f32, b(1)? as f32, 0.0, 1.0],
        0x0112 => [u(0)? as f32 / 65535.0, u(2)? as f32 / 65535.0, 0.0, 1.0],
        0x0212 => [s(0)? as f32 / 32767.0, s(2)? as f32 / 32767.0, 0.0, 1.0],
        _ => {
            return Err(BfresError::new(
                offset,
                format!("unsupported vertex format 0x{format:04X}"),
            ))
        }
    })
}

fn decode_indices(
    raw: &[u8],
    format: u32,
    count: usize,
    endian: Endian,
    primitive: u32,
) -> Result<Vec<u32>, BfresError> {
    let source = match format {
        0 => (0..count)
            .map(|i| byte_at(raw, i).map(u32::from))
            .collect::<Result<Vec<_>, _>>()?,
        1 => (0..count)
            .map(|i| u16_at(raw, i * 2, endian).map(u32::from))
            .collect::<Result<Vec<_>, _>>()?,
        2 => (0..count)
            .map(|i| u32_at(raw, i * 4, endian))
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(BfresError::new(0, "unsupported index format")),
    };
    if primitive == 3 {
        return Ok(source);
    }
    if primitive == 4 {
        let mut triangles = Vec::new();
        let restart = match format {
            0 => u8::MAX as u32,
            1 => u16::MAX as u32,
            _ => u32::MAX,
        };
        let mut strip = Vec::new();
        for index in source {
            if index == restart {
                strip.clear();
                continue;
            }
            strip.push(index);
            if strip.len() >= 3 {
                let i = strip.len() - 1;
                let (a, b) = if i % 2 == 0 {
                    (strip[i - 2], strip[i - 1])
                } else {
                    (strip[i - 1], strip[i - 2])
                };
                let c = strip[i];
                if a != b && b != c && a != c {
                    triangles.extend([a, b, c]);
                }
            }
        }
        return Ok(triangles);
    }
    Err(BfresError::new(
        0,
        format!("unsupported primitive type {primitive}"),
    ))
}

fn align_up(value: usize, alignment: usize) -> usize {
    if alignment <= 1 {
        value
    } else {
        (value + alignment - 1) / alignment * alignment
    }
}

fn half(value: u16) -> f32 {
    let sign = ((value >> 15) & 1) as u32;
    let exponent = ((value >> 10) & 0x1f) as u32;
    let fraction = (value & 0x3ff) as u32;
    let bits = if exponent == 0 {
        if fraction == 0 {
            sign << 31
        } else {
            let shift = fraction.leading_zeros() - 21;
            (sign << 31) | ((127 - 15 - shift) << 23) | ((fraction << (shift + 1) & 0x3ff) << 13)
        }
    } else if exponent == 31 {
        (sign << 31) | 0x7f800000 | (fraction << 13)
    } else {
        (sign << 31) | ((exponent + 112) << 23) | (fraction << 13)
    };
    f32::from_bits(bits)
}

fn byte_at(data: &[u8], offset: usize) -> Result<u8, BfresError> {
    data.get(offset)
        .copied()
        .ok_or_else(|| BfresError::new(offset, "truncated byte"))
}
pub(super) fn i16_at(data: &[u8], offset: usize, endian: Endian) -> Result<i16, BfresError> {
    Ok(u16_at(data, offset, endian)? as i16)
}
pub(super) fn f32_at(data: &[u8], offset: usize, endian: Endian) -> Result<f32, BfresError> {
    Ok(f32::from_bits(u32_at(data, offset, endian)?))
}

pub(super) fn u16_at(data: &[u8], offset: usize, endian: Endian) -> Result<u16, BfresError> {
    let bytes: [u8; 2] = data
        .get(offset..offset + 2)
        .ok_or_else(|| BfresError::new(offset, "truncated u16"))?
        .try_into()
        .unwrap();
    Ok(match endian {
        Endian::Little => u16::from_le_bytes(bytes),
        Endian::Big => u16::from_be_bytes(bytes),
    })
}

fn u32_at(data: &[u8], offset: usize, endian: Endian) -> Result<u32, BfresError> {
    let bytes: [u8; 4] = data
        .get(offset..offset + 4)
        .ok_or_else(|| BfresError::new(offset, "truncated u32"))?
        .try_into()
        .unwrap();
    Ok(match endian {
        Endian::Little => u32::from_le_bytes(bytes),
        Endian::Big => u32::from_be_bytes(bytes),
    })
}

pub(super) fn u64_at(data: &[u8], offset: usize, endian: Endian) -> Result<u64, BfresError> {
    let bytes: [u8; 8] = data
        .get(offset..offset + 8)
        .ok_or_else(|| BfresError::new(offset, "truncated u64"))?
        .try_into()
        .unwrap();
    Ok(match endian {
        Endian::Little => u64::from_le_bytes(bytes),
        Endian::Big => u64::from_be_bytes(bytes),
    })
}

pub(super) fn read_string(data: &[u8], offset: u64) -> Option<String> {
    let offset = usize::try_from(offset).ok()?;
    if offset == 0 || offset >= data.len() {
        return None;
    }
    if let Some(length_bytes) = data.get(offset..offset + 2) {
        let length = u16::from_le_bytes(length_bytes.try_into().ok()?) as usize;
        if length > 0 && length <= 0x1000 {
            let bytes = data.get(offset + 2..offset + 2 + length)?;
            if !bytes.iter().any(|byte| *byte < 0x20 && *byte != b'\t') {
                if let Ok(value) = std::str::from_utf8(bytes) {
                    return Some(value.to_owned());
                }
            }
        }
    }
    let tail = &data[offset..];
    let end = tail.iter().position(|byte| *byte == 0)?;
    let bytes = &tail[..end];
    if bytes.is_empty()
        || bytes.len() > 0x1000
        || bytes.iter().any(|byte| *byte < 0x20 && *byte != b'\t')
    {
        return None;
    }
    std::str::from_utf8(bytes).ok().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_bfres_data() {
        assert!(BfresFile::from_bytes(b"not a bfres file").is_err());
    }

    #[test]
    fn parses_bfres_corpus() {
        let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/bfres");
        if !corpus.is_dir() {
            return;
        }
        let mut parsed = 0;
        for entry in fs::read_dir(corpus).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|value| value.to_str()) != Some("bfres") {
                continue;
            }
            let bfres = BfresFile::from_path(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(bfres.header.endian, Endian::Little);
            assert!(bfres.sections_with_signature(b"FMDL").next().is_some());
            assert!(bfres.sections_with_signature(b"FSKL").next().is_some());
            assert!(bfres.sections_with_signature(b"FVTX").next().is_some());
            assert!(bfres.sections_with_signature(b"FSHP").next().is_some());
            assert!(bfres.sections_with_signature(b"FMAT").next().is_some());
            assert!(
                !bfres.materials.is_empty(),
                "{} has no decoded materials",
                path.display()
            );
            assert!(
                bfres
                    .materials
                    .iter()
                    .any(|material| !material.texture_slots.is_empty()),
                "{} has no decoded material texture slots",
                path.display()
            );
            assert!(
                !bfres.render.bones.is_empty(),
                "{} has no decoded bones",
                path.display()
            );
            assert!(
                !bfres.render.meshes.is_empty(),
                "{} has no decoded meshes",
                path.display()
            );
            assert!(
                bfres
                    .render
                    .meshes
                    .iter()
                    .all(|mesh| !mesh.positions.is_empty() && !mesh.indices.is_empty()),
                "{} has incomplete geometry",
                path.display()
            );
            assert!(
                bfres.render.meshes.iter().all(|mesh| mesh
                    .indices
                    .iter()
                    .all(|index| *index < mesh.positions.len() as u32)),
                "{} has out-of-range mesh indices",
                path.display()
            );
            assert!(
                bfres
                    .render
                    .meshes
                    .iter()
                    .all(|mesh| mesh.normals.len() == mesh.positions.len()),
                "{} has incomplete vertex normals",
                path.display()
            );
            parsed += 1;
        }
        assert!(parsed > 0, "BFRES corpus is empty");
    }
}
