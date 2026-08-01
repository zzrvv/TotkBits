use serde::Serialize;
use std::{
    collections::{BTreeSet, HashMap},
    io,
    path::{Path, PathBuf},
    sync::LazyLock,
};

use crate::{
    file_format::Model3D::bfres::{BfresBone, BfresMesh, BfresRenderGraph},
    parser::{
        binary::{BinaryReader, Endian},
        AOC::g1t::G1tFile,
    },
};

fn read_support_file(paths: &[&str], fallback: &str) -> String {
    for relative in paths {
        let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
        if let Ok(value) = std::fs::read_to_string(&manifest_path) {
            return value;
        }
        if let Ok(executable) = std::env::current_exe() {
            if let Some(parent) = executable.parent() {
                if let Ok(value) = std::fs::read_to_string(parent.join(relative)) {
                    return value;
                }
            }
        }
    }
    fallback.to_owned()
}

static PAIRS: LazyLock<HashMap<String, serde_json::Value>> = LazyLock::new(|| {
    serde_json::from_str(&read_support_file(&["bin/G1M_to_G1T_pairs.json"], "{}"))
        .unwrap_or_default()
});

static BOTW_BONE_NAMES: LazyLock<HashMap<usize, String>> = LazyLock::new(|| {
    serde_json::from_str::<HashMap<String, String>>(&read_support_file(
        &["bin/bones_botw.json"],
        "{}",
    ))
    .unwrap_or_default()
    .into_iter()
    .filter_map(|(bone, name)| {
        bone.strip_prefix("bone_")
            .and_then(|index| index.parse().ok())
            .map(|index| (index, name))
    })
    .collect()
});

static AOC_NAMES: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    serde_json::from_str(&read_support_file(
        &["bin/AOC_names.json", "misc/AOC_names.json"],
        "{}",
    ))
    .unwrap_or_default()
});

#[derive(Debug, Clone, Serialize)]
pub struct G1mHeader {
    pub version: [u8; 4],
    pub endian: String,
    pub target_address_size: u8,
    pub alignment_exponent: u8,
    pub file_size: u64,
    pub string_pool_offset: u64,
    pub string_pool_size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct G1mSection {
    pub signature: [u8; 4],
    pub offset: u64,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct G1mTextureSlot {
    pub index: usize,
    pub name: String,
    pub uv_layer: u16,
    pub sampler: String,
    pub texture_type: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct G1mMaterial {
    pub name: String,
    pub offset: u64,
    pub texture_slots: Vec<G1mTextureSlot>,
}

#[derive(Debug, Clone, Serialize)]
pub struct G1mFile {
    pub header: G1mHeader,
    pub name: Option<String>,
    pub sections: Vec<G1mSection>,
    pub materials: Vec<G1mMaterial>,
    pub render: BfresRenderGraph,
    pub format: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedG1tTexture {
    pub name: String,
    pub path: String,
    pub source: String,
    pub data_url: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone)]
struct VertexBuffer {
    offset: usize,
    stride: usize,
    count: usize,
}
#[derive(Clone)]
struct Attribute {
    buffer: usize,
    offset: usize,
    data_type: u8,
    semantic: u8,
    layer: u8,
}
#[derive(Clone, Default)]
struct AttributeSet {
    buffers: Vec<usize>,
    attributes: Vec<Attribute>,
}
#[derive(Clone)]
struct IndexBuffer {
    offset: usize,
    count: usize,
    stride: usize,
}
#[derive(Clone, Default)]
struct Submesh {
    vertex_buffer: usize,
    bone_palette: usize,
    bone_index: u32,
    material: u32,
    index_buffer: usize,
    primitive: u32,
    index_offset: usize,
    index_count: usize,
}
#[derive(Default)]
struct Geometry {
    materials: Vec<G1mMaterial>,
    vertex_buffers: Vec<VertexBuffer>,
    attributes: Vec<AttributeSet>,
    palettes: Vec<Vec<u32>>,
    index_buffers: Vec<IndexBuffer>,
    submeshes: Vec<Submesh>,
    visible_submeshes: Option<BTreeSet<usize>>,
}

impl G1mFile {
    pub fn from_path(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        Self::parse(
            &std::fs::read(path)?,
            path.file_stem().and_then(|v| v.to_str()).unwrap_or("G1M"),
        )
    }

    pub fn parse(data: &[u8], name: &str) -> io::Result<Self> {
        let endian = match data.get(..4) {
            Some(b"_M1G") => Endian::Little,
            Some(b"G1M_") => Endian::Big,
            _ => return Err(invalid("not a G1M model")),
        };
        let mut reader = BinaryReader::with_endian(data, endian);
        reader.skip(4)?;
        let version = reader.read_array_at::<4>(4)?;
        reader.skip(4)?;
        let file_size = reader.read_u32()? as usize;
        let chunk_offset = reader.read_u32()? as usize;
        reader.skip(4)?;
        let chunk_count = reader.read_u32()? as usize;
        if file_size > data.len() || chunk_count > 4096 {
            return Err(invalid("invalid G1M header"));
        }
        reader.seek(chunk_offset)?;
        let mut sections = Vec::new();
        let mut bones = Vec::new();
        let mut geometry = None;
        for _ in 0..chunk_count {
            let start = reader.position();
            let signature: [u8; 4] = reader.read_bytes(4)?.try_into().unwrap();
            reader.skip(4)?;
            let size = reader.read_u32()? as usize;
            if size < 12 || start.checked_add(size).is_none_or(|end| end > data.len()) {
                return Err(invalid("invalid G1M chunk size"));
            }
            sections.push(G1mSection {
                signature,
                offset: start as u64,
                name: None,
            });
            if matches!(&signature, b"G1MS" | b"SM1G") && bones.is_empty() {
                bones = parse_skeleton(&data[start..start + size], endian)?;
            } else if matches!(&signature, b"G1MG" | b"GM1G") {
                geometry = Some(parse_geometry(&data[start..start + size], endian, start)?);
            }
            reader.seek(start + size)?;
        }
        let geometry = geometry.ok_or_else(|| invalid("G1M has no G1MG geometry chunk"))?;
        let meshes = build_meshes(data, &geometry, endian)?;
        let display_name = AOC_NAMES
            .get(&name.to_ascii_lowercase())
            .filter(|value| !value.is_empty())
            .map(|value| value.replace(' ', "_"))
            .unwrap_or_else(|| name.to_owned());
        Ok(Self {
            header: G1mHeader {
                version,
                endian: format!("{endian:?}"),
                target_address_size: 4,
                alignment_exponent: 0,
                file_size: file_size as u64,
                string_pool_offset: 0,
                string_pool_size: 0,
            },
            name: Some(display_name),
            sections,
            materials: geometry.materials,
            render: BfresRenderGraph {
                bones,
                matrix_to_bone: Vec::new(),
                meshes,
            },
            format: "G1M".into(),
        })
    }

    pub fn open(
        path: &Path,
    ) -> Option<(
        crate::file_format::BinTextFile::OpenedFile<'static>,
        crate::Open_and_Save::SendData,
    )> {
        let magic = std::fs::read(path).ok()?;
        if !matches!(magic.get(..4), Some(b"_M1G") | Some(b"G1M_")) {
            return None;
        }
        let mut opened = crate::file_format::BinTextFile::OpenedFile::default();
        opened.path = crate::Settings::Pathlib::new(path);
        opened.file_type = crate::Zstd::TotkFileType::Other;
        let mut send = crate::Open_and_Save::SendData::default();
        send.path = crate::Settings::Pathlib::new(path);
        send.file_label = format!("{} [G1M]", send.path.name);
        send.file_metadata = "[G1M] [3D MODEL] [READ ONLY]".into();
        send.status_text = format!("Opened G1M {}", path.display());
        send.tab = "3D".into();
        send.read_only = true;
        Some((opened, send))
    }

    pub fn resolve_textures(&self, source: &Path, aoc_path: &Path) -> Vec<ResolvedG1tTexture> {
        let Some(pair) = paired_texture_info(source) else {
            return Vec::new();
        };
        let resolved = resolve_kidsobj_textures(self, source, aoc_path, &pair);
        if !resolved.is_empty() {
            return resolved;
        }
        // Standalone dumps sometimes place the paired value directly beside
        // the model as a G1T without KTID/KidsObj metadata.
        resolve_direct_g1t(source, aoc_path, &pair.ktid_hash)
    }
}

fn parse_skeleton(data: &[u8], endian: Endian) -> io::Result<Vec<BfresBone>> {
    let mut reader = BinaryReader::with_endian(data, endian);
    reader.skip(12)?;
    let joint_offset = reader.read_u32()? as usize;
    reader.skip(4)?;
    let joint_count = reader.read_u16()? as usize;
    let index_count = reader.read_u16()? as usize;
    reader.skip(4 + index_count * 2)?;
    reader.seek(joint_offset)?;
    let mut bones = Vec::with_capacity(joint_count);
    for index in 0..joint_count {
        let scale = [reader.read_f32()?, reader.read_f32()?, reader.read_f32()?];
        let parent = reader.read_i32()?;
        let rotation = [
            reader.read_f32()?,
            reader.read_f32()?,
            reader.read_f32()?,
            reader.read_f32()?,
        ];
        let translation = [reader.read_f32()?, reader.read_f32()?, reader.read_f32()?];
        reader.skip(4)?;
        bones.push(BfresBone {
            name: BOTW_BONE_NAMES
                .get(&index)
                .cloned()
                .unwrap_or_else(|| format!("bone_{index}")),
            parent_index: i16::try_from(parent).unwrap_or(-1),
            smooth_matrix_index: -1,
            rigid_matrix_index: -1,
            rotation_mode: "quaternion".into(),
            scale,
            rotation,
            translation,
        });
    }
    Ok(bones)
}

fn parse_geometry(data: &[u8], endian: Endian, file_offset: usize) -> io::Result<Geometry> {
    let mut reader = BinaryReader::with_endian(data, endian);
    reader.skip(12 + 4 + 4 + 24)?;
    let section_count = reader.read_u32()? as usize;
    let version = BinaryReader::with_endian(data, endian).read_u32_at(4)?;
    let mut geometry = Geometry::default();
    for _ in 0..section_count {
        let section_start = reader.position();
        let kind = reader.read_u32()?;
        let size = reader.read_u32()? as usize;
        let count = reader.read_u32()? as usize;
        let section_end = section_start
            .checked_add(size)
            .ok_or_else(|| invalid("section overflow"))?;
        match kind {
            0x0001_0002 => {
                for material_index in 0..count {
                    reader.skip(4)?;
                    let texture_count = reader.read_u32()? as usize;
                    reader.skip(8)?;
                    let mut slots = Vec::new();
                    for slot_index in 0..texture_count {
                        let id = reader.read_u16()?;
                        let layer = reader.read_u16()?;
                        let texture_type = reader.read_u16()?;
                        let subtype = reader.read_u16()?;
                        reader.skip(4)?;
                        let (sampler, classification) = classify_texture(texture_type, subtype);
                        slots.push(G1mTextureSlot {
                            index: slot_index,
                            name: id.to_string(),
                            uv_layer: layer,
                            sampler,
                            texture_type: classification,
                        });
                    }
                    geometry.materials.push(G1mMaterial {
                        name: format!("Material {material_index}"),
                        offset: section_start as u64,
                        texture_slots: slots,
                    });
                }
            }
            0x0001_0004 => {
                for _ in 0..count {
                    reader.skip(4)?;
                    let stride = reader.read_u32()? as usize;
                    let vertex_count = reader.read_u32()? as usize;
                    if version > 0x3030_3430 {
                        reader.skip(4)?;
                    }
                    let offset = file_offset + reader.position();
                    geometry.vertex_buffers.push(VertexBuffer {
                        offset,
                        stride,
                        count: vertex_count,
                    });
                    reader.skip(
                        stride
                            .checked_mul(vertex_count)
                            .ok_or_else(|| invalid("vertex buffer overflow"))?,
                    )?;
                }
            }
            0x0001_0005 => {
                for _ in 0..count {
                    let list_count = reader.read_u32()? as usize;
                    let mut buffers = Vec::with_capacity(list_count);
                    for _ in 0..list_count {
                        buffers.push(reader.read_u32()? as usize);
                    }
                    let attr_count = reader.read_u32()? as usize;
                    let mut attributes = Vec::with_capacity(attr_count);
                    for _ in 0..attr_count {
                        let buffer = reader.read_u16()? as usize;
                        let offset = reader.read_u16()? as usize;
                        let data_type = reader.read_u8()?;
                        reader.skip(1)?;
                        let semantic = reader.read_u8()?;
                        let layer = reader.read_u8()?;
                        attributes.push(Attribute {
                            buffer,
                            offset,
                            data_type,
                            semantic,
                            layer,
                        });
                    }
                    geometry.attributes.push(AttributeSet {
                        buffers,
                        attributes,
                    });
                }
            }
            0x0001_0006 => {
                for _ in 0..count {
                    let joint_count = reader.read_u32()? as usize;
                    let mut palette = Vec::with_capacity(joint_count);
                    for _ in 0..joint_count {
                        reader.skip(8)?;
                        palette.push(reader.read_u32()? & 0x7fff_ffff);
                    }
                    geometry.palettes.push(palette);
                }
            }
            0x0001_0007 => {
                for _ in 0..count {
                    let index_count = reader.read_u32()? as usize;
                    let bits = reader.read_u32()? as usize;
                    if version > 0x3030_3430 {
                        reader.skip(4)?;
                    }
                    let stride = bits / 8;
                    let offset = file_offset + reader.position();
                    geometry.index_buffers.push(IndexBuffer {
                        offset,
                        count: index_count,
                        stride,
                    });
                    reader.skip(
                        stride
                            .checked_mul(index_count)
                            .ok_or_else(|| invalid("index buffer overflow"))?,
                    )?;
                    reader.align(4)?;
                }
            }
            0x0001_0008 => {
                for _ in 0..count {
                    let values: Vec<u32> = (0..14)
                        .map(|_| reader.read_u32())
                        .collect::<io::Result<_>>()?;
                    geometry.submeshes.push(Submesh {
                        vertex_buffer: values[1] as usize,
                        bone_palette: values[2] as usize,
                        bone_index: values[3],
                        material: values[6],
                        index_buffer: values[7] as usize,
                        primitive: values[9],
                        index_offset: values[12] as usize,
                        index_count: values[13] as usize,
                    });
                }
            }
            0x0001_0009 => {
                for group_index in 0..count {
                    let (submesh_count_1, submesh_count_2) = if version > 0x3030_3330 {
                        reader.skip(12)?;
                        let first = reader.read_u32()? as usize;
                        let second = reader.read_u32()? as usize;
                        if version > 0x3030_3430 {
                            reader.skip(16)?;
                        }
                        (first, second)
                    } else {
                        reader.skip(4)?;
                        let first = reader.read_u32()? as usize;
                        let second = reader.read_u32()? as usize;
                        (first, second)
                    };
                    for _ in 0..submesh_count_1.saturating_add(submesh_count_2) {
                        reader.skip(16 + 8)?;
                        let index_count = reader.read_u32()? as usize;
                        if group_index == 0 {
                            let visible = geometry.visible_submeshes.get_or_insert_default();
                            for _ in 0..index_count {
                                visible.insert(reader.read_u32()? as usize);
                            }
                        } else if index_count == 0 {
                            reader.skip(4)?;
                        } else {
                            reader.skip(index_count.saturating_mul(4))?;
                        }
                        if group_index == 0 && index_count == 0 {
                            reader.skip(4)?;
                        }
                    }
                }
            }
            _ => {}
        }
        if section_end > data.len() {
            return Err(invalid("G1MG section exceeds chunk"));
        }
        reader.seek(section_end)?;
    }
    Ok(geometry)
}

fn build_meshes(data: &[u8], geometry: &Geometry, endian: Endian) -> io::Result<Vec<BfresMesh>> {
    let source = BinaryReader::with_endian(data, endian);
    let mut meshes = Vec::new();
    for (mesh_index, submesh) in geometry.submeshes.iter().enumerate() {
        if geometry
            .visible_submeshes
            .as_ref()
            .is_some_and(|visible| !visible.contains(&mesh_index))
        {
            continue;
        }
        let Some(attrs) = geometry.attributes.get(submesh.vertex_buffer) else {
            continue;
        };
        let Some(index_buffer) = geometry.index_buffers.get(submesh.index_buffer) else {
            continue;
        };
        let mut raw_indices = Vec::with_capacity(submesh.index_count);
        for index in submesh.index_offset
            ..submesh
                .index_offset
                .saturating_add(submesh.index_count)
                .min(index_buffer.count)
        {
            let offset = index_buffer.offset + index * index_buffer.stride;
            raw_indices.push(match index_buffer.stride {
                2 => source.read_u16_at(offset)? as u32,
                4 => source.read_u32_at(offset)?,
                _ => return Err(invalid("unsupported G1M index width")),
            });
        }
        let indices = if submesh.primitive == 4 {
            strip_to_triangles(&raw_indices)
        } else {
            raw_indices
        };
        if indices.is_empty() {
            continue;
        }
        let vertex_count = attrs
            .buffers
            .iter()
            .filter_map(|index| geometry.vertex_buffers.get(*index).map(|v| v.count))
            .min()
            .unwrap_or(0);
        let mut positions = vec![[0.0; 3]; vertex_count];
        let mut normals = vec![[0.0; 3]; vertex_count];
        let mut uv_maps: Vec<Vec<[f32; 2]>> = Vec::new();
        let mut colors = vec![[1.0; 4]; vertex_count];
        let mut bone_indices = vec![[0; 4]; vertex_count];
        let mut bone_weights = vec![[0.0; 4]; vertex_count];
        for attribute in &attrs.attributes {
            let Some(buffer_index) = attrs.buffers.get(attribute.buffer).copied() else {
                continue;
            };
            let Some(buffer) = geometry.vertex_buffers.get(buffer_index) else {
                continue;
            };
            let is_uv =
                attribute.semantic == 5 && matches!(attribute.data_type, 0x01 | 0x03 | 0x0a | 0x0b);
            if is_uv {
                let highest_layer = if matches!(attribute.data_type, 0x03 | 0x0b) {
                    1
                } else {
                    attribute.layer as usize
                };
                while uv_maps.len() <= highest_layer {
                    uv_maps.push(vec![[0.0; 2]; vertex_count]);
                }
            }
            for vertex in 0..vertex_count.min(buffer.count) {
                let values = read_attribute(
                    &source,
                    buffer.offset + vertex * buffer.stride + attribute.offset,
                    attribute.data_type,
                )?;
                match attribute.semantic {
                    0 => positions[vertex] = [values[0], values[1], values[2]],
                    1 => bone_weights[vertex] = values,
                    2 => {
                        let palette = geometry.palettes.get(submesh.bone_palette);
                        for component in 0..4 {
                            let local = values[component] as usize / 3;
                            bone_indices[vertex][component] = palette
                                .and_then(|p| p.get(local))
                                .copied()
                                .unwrap_or(local as u32)
                                as u16;
                        }
                    }
                    3 => normals[vertex] = [values[0], values[1], values[2]],
                    5 if is_uv => {
                        if matches!(attribute.data_type, 0x03 | 0x0b) {
                            uv_maps[0][vertex] = [values[0], values[1]];
                            uv_maps[1][vertex] = [values[2], values[3]];
                        } else {
                            uv_maps[attribute.layer as usize][vertex] = [values[0], values[1]];
                        }
                    }
                    10 => colors[vertex] = values,
                    _ => {}
                }
            }
        }
        let skin_bones = bone_indices
            .iter()
            .flatten()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        // G1M files commonly carry placeholder TEXCOORD layers. A layer whose
        // vertices all collapse onto one coordinate cannot map a 2D texture;
        // discard it so the first exposed layer is the first useful UV set.
        uv_maps.retain(|uv_map| useful_uv_map(uv_map));
        let uv0 = uv_maps.first().cloned().unwrap_or_default();
        meshes.push(BfresMesh {
            name: format!("Mesh {mesh_index}"),
            material_index: submesh.material as u16,
            bone_index: submesh.bone_index as u16,
            vertex_skin_count: if bone_weights.iter().any(|w| w[1] > 0.0) {
                4
            } else {
                1
            },
            positions,
            normals,
            uv0,
            uv_maps,
            colors,
            bone_indices,
            bone_weights,
            indices,
            skin_bones,
        });
    }
    Ok(meshes)
}

fn read_attribute(reader: &BinaryReader<'_>, offset: usize, kind: u8) -> io::Result<[f32; 4]> {
    let mut values = [0.0; 4];
    match kind {
        0x00 => values[0] = f32::from_bits(reader.read_u32_at(offset)?),
        0x01..=0x03 => {
            let count = usize::from(kind) + 1;
            for (i, value) in values.iter_mut().enumerate().take(count) {
                *value = f32::from_bits(reader.read_u32_at(offset + i * 4)?);
            }
        }
        0x05 => {
            for (i, value) in values.iter_mut().enumerate() {
                *value = reader.read_u8_at(offset + i)? as f32;
            }
        }
        0x07 => {
            for (i, value) in values.iter_mut().enumerate() {
                *value = reader.read_u16_at(offset + i * 2)? as f32;
            }
        }
        0x0a => {
            for (i, value) in values.iter_mut().enumerate().take(2) {
                *value = half(reader.read_u16_at(offset + i * 2)?);
            }
        }
        0x0b => {
            for (i, value) in values.iter_mut().enumerate() {
                *value = half(reader.read_u16_at(offset + i * 2)?);
            }
        }
        0x0d => {
            for (i, value) in values.iter_mut().enumerate() {
                *value = reader.read_u8_at(offset + i)? as f32 / 255.0;
            }
        }
        _ => {}
    }
    Ok(values)
}

fn useful_uv_map(uv_map: &[[f32; 2]]) -> bool {
    let Some(first) = uv_map.first() else {
        return false;
    };
    first.iter().all(|value| value.is_finite())
        && uv_map.iter().skip(1).any(|uv| {
            uv.iter().all(|value| value.is_finite())
                && ((uv[0] - first[0]).abs() > 1.0e-6 || (uv[1] - first[1]).abs() > 1.0e-6)
        })
}

fn half(value: u16) -> f32 {
    let sign = ((value & 0x8000) as u32) << 16;
    let exponent = (value >> 10) & 0x1f;
    let fraction = (value & 0x03ff) as u32;
    let bits = match exponent {
        0 if fraction == 0 => sign,
        0 => {
            let shift = fraction.leading_zeros() - 21;
            sign | ((127 - 15 - shift) << 23) | ((fraction << (shift + 1)) & 0x7f_ffff)
        }
        31 => sign | 0x7f80_0000 | (fraction << 13),
        _ => sign | (((exponent as u32) + 112) << 23) | (fraction << 13),
    };
    f32::from_bits(bits)
}

fn strip_to_triangles(strip: &[u32]) -> Vec<u32> {
    let mut output = Vec::new();
    for index in 0..strip.len().saturating_sub(2) {
        let triangle = if index % 2 == 0 {
            [strip[index], strip[index + 1], strip[index + 2]]
        } else {
            [strip[index], strip[index + 2], strip[index + 1]]
        };
        if triangle[0] != triangle[1] && triangle[1] != triangle[2] && triangle[0] != triangle[2] {
            output.extend(triangle);
        }
    }
    output
}

fn classify_texture(kind: u16, subtype: u16) -> (String, String) {
    match (kind, subtype) {
        (1, _) => ("_a0".into(), "Diffuse".into()),
        (3, _) => ("_n0".into(), "Normal".into()),
        (19, 0) | (2, _) => ("_e0".into(), "Emission".into()),
        (5, 5) => ("_ao0".into(), "AmbientOcclusion".into()),
        (37, 37) | (66, 66) => ("_s0".into(), "Specular".into()),
        _ => (String::new(), "Unknown".into()),
    }
}

struct PairedTextureInfo {
    ktid_hash: String,
    kidsobjdb: Option<String>,
}

fn paired_texture_info(source: &Path) -> Option<PairedTextureInfo> {
    let stem = source
        .file_stem()?
        .to_str()?
        .split('.')
        .next()?
        .to_ascii_lowercase();
    let pair = PAIRS.get(&stem)?;
    Some(PairedTextureInfo {
        ktid_hash: pair.get("g1t")?.as_str()?.to_owned(),
        kidsobjdb: pair
            .get("kidsobjdb")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    })
}

fn resolve_kidsobj_textures(
    model: &G1mFile,
    source: &Path,
    aoc: &Path,
    pair: &PairedTextureInfo,
) -> Vec<ResolvedG1tTexture> {
    let Some(ktid_path) = find_asset(source, aoc, "ktid", &pair.ktid_hash, "ktid") else {
        return Vec::new();
    };
    let Some(kids_name) = pair.kidsobjdb.as_deref() else {
        return Vec::new();
    };
    let (kids_stem, kids_ext) = kids_name
        .rsplit_once('.')
        .unwrap_or((kids_name, "kidsobjdb"));
    let Some(kids_path) = find_asset(source, aoc, "kidsobjdb", kids_stem, kids_ext) else {
        return Vec::new();
    };
    let Ok(ktid) = parse_ktid(&std::fs::read(ktid_path).unwrap_or_default()) else {
        return Vec::new();
    };
    let Ok(kids) = parse_kidsobj(&std::fs::read(kids_path).unwrap_or_default()) else {
        return Vec::new();
    };
    let slot_ids: BTreeSet<_> = model
        .materials
        .iter()
        .flat_map(|material| material.texture_slots.iter().map(|slot| slot.name.clone()))
        .collect();
    let mut textures = Vec::new();
    for slot_id in slot_ids {
        let Ok(index) = slot_id.parse::<u32>() else {
            continue;
        };
        let Some(ktid_hash) = ktid.get(&index) else {
            continue;
        };
        let Some(g1t_hash) = kids.get(ktid_hash) else {
            continue;
        };
        let Some(path) = find_g1t(source, aoc, g1t_hash) else {
            continue;
        };
        let Ok(g1t) = G1tFile::parse(&std::fs::read(&path).unwrap_or_default()) else {
            continue;
        };
        let Some(texture) = g1t.textures.into_iter().next() else {
            continue;
        };
        textures.push(resolved_texture(slot_id, path, texture, source));
    }
    textures
}

fn resolve_direct_g1t(source: &Path, aoc: &Path, hash: &str) -> Vec<ResolvedG1tTexture> {
    let Some(path) = find_g1t(source, aoc, hash) else {
        return Vec::new();
    };
    let Ok(g1t) = G1tFile::parse(&std::fs::read(&path).unwrap_or_default()) else {
        return Vec::new();
    };
    g1t.textures
        .into_iter()
        .map(|texture| resolved_texture(texture.index.to_string(), path.clone(), texture, source))
        .collect()
}

fn resolved_texture(
    name: String,
    path: PathBuf,
    texture: crate::parser::AOC::g1t::G1tTexture,
    source: &Path,
) -> ResolvedG1tTexture {
    ResolvedG1tTexture {
        name,
        path: format!("{}#{}", path.display(), texture.index),
        source: if path.parent() == source.parent() {
            "adjacent-g1t"
        } else {
            "aoc-path"
        }
        .into(),
        data_url: texture.data_url,
        width: texture.width,
        height: texture.height,
    }
}

fn parse_ktid(data: &[u8]) -> io::Result<HashMap<u32, String>> {
    if data.len() % 8 != 0 {
        return Err(invalid("KTID size is not a multiple of 8"));
    }
    let reader = BinaryReader::with_endian(data, Endian::Little);
    let mut result = HashMap::new();
    for offset in (0..data.len()).step_by(8) {
        result.insert(
            reader.read_u32_at(offset)?,
            format!("{:08x}", reader.read_u32_at(offset + 4)?),
        );
    }
    Ok(result)
}

fn parse_kidsobj(data: &[u8]) -> io::Result<HashMap<String, String>> {
    let mut reader = BinaryReader::with_endian(data, Endian::Little);
    if reader.read_u32()? != 0x4b4f_445f {
        return Err(invalid("invalid KidsObj signature"));
    }
    reader.skip(12)?;
    let entry_count = reader.read_u32()? as usize;
    reader.skip(8)?;
    let mut result = HashMap::new();
    for _ in 0..entry_count {
        let signature = reader.read_u32()?;
        reader.skip(8)?;
        let object_hash = format!("{:08x}", reader.read_u32()?);
        reader.skip(4)?;
        let column_count = reader.read_u32()? as usize;
        let mut rows = Vec::with_capacity(column_count);
        for _ in 0..column_count {
            reader.skip(4)?;
            let row_count = reader.read_u32()? as usize;
            reader.skip(4)?;
            rows.push(row_count);
        }
        let mut first_texture = None;
        for row_count in rows {
            for _ in 0..row_count {
                let value = reader.read_u32()?;
                if value != 0 && first_texture.is_none() {
                    first_texture = Some(format!("{value:08x}"));
                }
            }
        }
        if signature == 0x4b4f_4449 {
            if let Some(texture) = first_texture {
                result.insert(object_hash, texture);
            }
        }
    }
    Ok(result)
}

fn find_asset(
    source: &Path,
    aoc: &Path,
    folder: &str,
    stem: &str,
    extension: &str,
) -> Option<PathBuf> {
    let filename = format!("{stem}.{extension}");
    let adjacent = source.parent()?.join(&filename);
    if adjacent.is_file() {
        return Some(adjacent);
    }
    if aoc.as_os_str().is_empty() {
        return None;
    }
    for candidate in [aoc.join(&filename), aoc.join(folder).join(&filename)] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    std::fs::read_dir(aoc)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join(folder).join(&filename))
        .find(|path| path.is_file())
}

fn find_g1t(source: &Path, aoc: &Path, hash: &str) -> Option<PathBuf> {
    let name = format!("{hash}.g1t");
    let adjacent = source.parent()?.join(&name);
    if adjacent.is_file() {
        return Some(adjacent);
    }
    if aoc.as_os_str().is_empty() {
        return None;
    }
    for candidate in [
        aoc.join(&name),
        aoc.join("MaterialEditor/g1t").join(&name),
        aoc.join("g1t").join(&name),
    ] {
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    std::fs::read_dir(aoc)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("g1t").join(&name))
        .find(|path| path.is_file())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_g1m_corpus() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/g1m");
        let mut parsed = 0;
        let mut renderable = 0;
        for path in std::fs::read_dir(root)
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("g1m"))
            })
        {
            let model = G1mFile::from_path(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert!(model
                .render
                .meshes
                .iter()
                .flat_map(|mesh| &mesh.uv_maps)
                .all(|uv_map| useful_uv_map(uv_map)));
            parsed += 1;
            renderable += usize::from(!model.render.meshes.is_empty());
        }
        assert!(parsed >= 28);
        assert!(renderable >= 24);
    }

    #[test]
    fn pairing_table_contains_sample_model() {
        assert_eq!(
            paired_texture_info(Path::new("00320929.g1m"))
                .map(|pair| pair.ktid_hash)
                .as_deref(),
            Some("5399aa72")
        );
    }

    #[test]
    fn botw_bone_names_are_applied_by_joint_index() {
        assert_eq!(BOTW_BONE_NAMES.get(&0).map(String::as_str), Some("Root"));
        assert_eq!(BOTW_BONE_NAMES.get(&22).map(String::as_str), Some("Head"));
    }

    #[test]
    fn aoc_model_names_are_resolved_from_hashes() {
        assert_eq!(
            AOC_NAMES.get("038bb045").map(|name| name.replace(' ', "_")),
            Some("Amber_Earrings".to_owned())
        );
        assert!(!AOC_NAMES.contains_key("ffffffff"));
    }

    #[test]
    fn degenerate_uv_maps_are_rejected() {
        assert!(!useful_uv_map(&[]));
        assert!(!useful_uv_map(&[[0.0, 0.0], [0.0, 0.0]]));
        assert!(useful_uv_map(&[[0.0, 0.0], [1.0, 0.0]]));
    }

    #[test]
    fn cloth_control_indices_are_not_exposed_as_uv_maps() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/g1m/0cb432cf.g1m");
        let model = G1mFile::from_path(path).unwrap();
        assert!(model
            .render
            .meshes
            .iter()
            .all(|mesh| mesh.uv_maps.len() <= 2));
    }
}
