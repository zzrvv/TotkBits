use base64::Engine;
use serde_json::{json, Map, Value};
use std::{collections::HashMap, fs, io, io::Cursor, path::Path};

use crate::parser::{
    binary::BinaryWriter,
    AOC::g1m::{G1mFile, ResolvedG1tTexture},
};

struct BinaryChunk {
    data: BinaryWriter,
    views: Vec<Value>,
}

impl BinaryChunk {
    fn new() -> Self {
        Self {
            data: BinaryWriter::new(),
            views: Vec::new(),
        }
    }

    fn push(&mut self, bytes: &[u8], target: Option<u32>) -> io::Result<usize> {
        self.data.align(4)?;
        let offset = self.data.position();
        self.data.write_bytes(bytes);
        let mut view = json!({ "buffer": 0, "byteOffset": offset, "byteLength": bytes.len() });
        if let Some(target) = target {
            view["target"] = target.into();
        }
        let index = self.views.len();
        self.views.push(view);
        Ok(index)
    }
}

fn floats(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    for value in values {
        writer.write_f32(value);
    }
    writer.into_inner()
}

fn u32s(values: impl IntoIterator<Item = u32>) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    for value in values {
        writer.write_u32(value);
    }
    writer.into_inner()
}

fn u16s(values: impl IntoIterator<Item = u16>) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    for value in values {
        writer.write_u16(value);
    }
    writer.into_inner()
}

fn normalize4(q: [f32; 4]) -> [f32; 4] {
    let length = q.iter().map(|value| value * value).sum::<f32>().sqrt();
    if length <= f32::EPSILON {
        [0.0, 0.0, 0.0, 1.0]
    } else {
        q.map(|value| value / length)
    }
}

fn compose_matrix(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> [f32; 16] {
    let [x, y, z, w] = normalize4(q);
    [
        (1.0 - 2.0 * (y * y + z * z)) * s[0],
        (2.0 * (x * y - z * w)) * s[1],
        (2.0 * (x * z + y * w)) * s[2],
        t[0],
        (2.0 * (x * y + z * w)) * s[0],
        (1.0 - 2.0 * (x * x + z * z)) * s[1],
        (2.0 * (y * z - x * w)) * s[2],
        t[1],
        (2.0 * (x * z - y * w)) * s[0],
        (2.0 * (y * z + x * w)) * s[1],
        (1.0 - 2.0 * (x * x + y * y)) * s[2],
        t[2],
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

fn multiply_matrix(a: [f32; 16], b: [f32; 16]) -> [f32; 16] {
    let mut result = [0.0; 16];
    for row in 0..4 {
        for column in 0..4 {
            for k in 0..4 {
                result[row * 4 + column] += a[row * 4 + k] * b[k * 4 + column];
            }
        }
    }
    result
}

fn inverse_affine_matrix(m: [f32; 16]) -> [f32; 16] {
    let determinant = m[0] * (m[5] * m[10] - m[6] * m[9]) - m[1] * (m[4] * m[10] - m[6] * m[8])
        + m[2] * (m[4] * m[9] - m[5] * m[8]);
    if determinant.abs() <= f32::EPSILON {
        return compose_matrix([0.0; 3], [0.0, 0.0, 0.0, 1.0], [1.0; 3]);
    }
    let inverse = [
        (m[5] * m[10] - m[6] * m[9]) / determinant,
        (m[2] * m[9] - m[1] * m[10]) / determinant,
        (m[1] * m[6] - m[2] * m[5]) / determinant,
        (m[6] * m[8] - m[4] * m[10]) / determinant,
        (m[0] * m[10] - m[2] * m[8]) / determinant,
        (m[2] * m[4] - m[0] * m[6]) / determinant,
        (m[4] * m[9] - m[5] * m[8]) / determinant,
        (m[1] * m[8] - m[0] * m[9]) / determinant,
        (m[0] * m[5] - m[1] * m[4]) / determinant,
    ];
    let t = [m[3], m[7], m[11]];
    [
        inverse[0],
        inverse[1],
        inverse[2],
        -(inverse[0] * t[0] + inverse[1] * t[1] + inverse[2] * t[2]),
        inverse[3],
        inverse[4],
        inverse[5],
        -(inverse[3] * t[0] + inverse[4] * t[1] + inverse[5] * t[2]),
        inverse[6],
        inverse[7],
        inverse[8],
        -(inverse[6] * t[0] + inverse[7] * t[1] + inverse[8] * t[2]),
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

fn transpose(m: [f32; 16]) -> [f32; 16] {
    std::array::from_fn(|index| m[(index % 4) * 4 + index / 4])
}

fn transform_point(m: [f32; 16], point: [f32; 3]) -> [f32; 3] {
    [
        m[0] * point[0] + m[1] * point[1] + m[2] * point[2] + m[3],
        m[4] * point[0] + m[5] * point[1] + m[6] * point[2] + m[7],
        m[8] * point[0] + m[9] * point[1] + m[10] * point[2] + m[11],
    ]
}

fn normalize3(value: [f32; 3]) -> [f32; 3] {
    let length = value
        .iter()
        .map(|component| component * component)
        .sum::<f32>()
        .sqrt();
    if length <= f32::EPSILON {
        value
    } else {
        value.map(|component| component / length)
    }
}

fn transform_normal(m: [f32; 16], normal: [f32; 3]) -> [f32; 3] {
    let inverse = inverse_affine_matrix(m);
    normalize3([
        inverse[0] * normal[0] + inverse[4] * normal[1] + inverse[8] * normal[2],
        inverse[1] * normal[0] + inverse[5] * normal[1] + inverse[9] * normal[2],
        inverse[2] * normal[0] + inverse[6] * normal[1] + inverse[10] * normal[2],
    ])
}

fn vertex_influences(
    indices: [u16; 4],
    weights: [f32; 4],
    fallback_bone: u16,
    bone_count: usize,
) -> ([u16; 4], [f32; 4]) {
    let mut combined = std::collections::BTreeMap::<u16, f32>::new();
    for (bone, weight) in indices.into_iter().zip(weights) {
        if (bone as usize) < bone_count && weight.is_finite() && weight > 0.0 {
            *combined.entry(bone).or_default() += weight;
        }
    }
    let mut combined: Vec<_> = combined.into_iter().collect();
    combined.sort_by(|left, right| right.1.total_cmp(&left.1));
    let total: f32 = combined.iter().take(4).map(|(_, weight)| weight).sum();
    let mut result_indices = [0; 4];
    let mut result_weights = [0.0; 4];
    if total > f32::EPSILON {
        for (influence, (bone, weight)) in combined.into_iter().take(4).enumerate() {
            result_indices[influence] = bone;
            result_weights[influence] = weight / total;
        }
    } else if bone_count > 0 {
        result_indices[0] = fallback_bone.min((bone_count - 1) as u16);
        result_weights[0] = 1.0;
    }
    (result_indices, result_weights)
}

fn convert_uv_for_glb(uv: [f32; 2]) -> [f32; 2] {
    // Keep U unchanged. glTF importers account for the embedded image's V
    // convention; pre-flipping V here cancels Blender's intended Mirror Y.
    uv
}

fn accessor(view: usize, component_type: u32, count: usize, kind: &str) -> Value {
    json!({ "bufferView": view, "componentType": component_type, "count": count, "type": kind })
}

fn has_fully_transparent_pixel(image: &image::DynamicImage) -> bool {
    image.to_rgba8().pixels().any(|pixel| pixel[3] == 0)
}

fn texture_png(texture: &ResolvedG1tTexture) -> Option<(Vec<u8>, bool)> {
    let data_url = (!texture.data_url.is_empty())
        .then_some(texture.data_url.as_str())
        .or_else(|| texture.data_urls.first().map(String::as_str))?;
    let (_, encoded) = data_url.strip_prefix("data:")?.split_once(',')?;
    let source = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let image = image::load_from_memory(&source).ok()?;
    let has_transparency = has_fully_transparent_pixel(&image);
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some((png.into_inner(), has_transparency))
}

pub fn export_g1m(
    models: &[(&G1mFile, &[ResolvedG1tTexture], String)],
    output: &Path,
) -> io::Result<()> {
    let mut binary = BinaryChunk::new();
    let mut accessors = Vec::new();
    let mut meshes = Vec::new();
    let mut nodes = Vec::new();
    let mut materials = Vec::new();
    let mut images = Vec::new();
    let mut textures = Vec::new();
    let mut skins = Vec::new();
    let mut texture_indices = HashMap::<String, usize>::new();
    let mut texture_has_transparency = HashMap::<String, bool>::new();

    for (_, resolved, prefix) in models {
        for texture in *resolved {
            let key = format!("{prefix}{}", texture.name).to_ascii_lowercase();
            if texture_indices.contains_key(&key) {
                continue;
            }
            let Some((bytes, has_transparency)) = texture_png(texture) else {
                continue;
            };
            let view = binary.push(&bytes, None)?;
            let index = images.len();
            images.push(json!({ "name": format!("{prefix}{}.png", texture.name), "bufferView": view, "mimeType": "image/png" }));
            textures.push(json!({ "source": index, "sampler": 0 }));
            texture_indices.insert(key.clone(), index);
            texture_has_transparency.insert(key, has_transparency);
            for alias in &texture.aliases {
                let alias = format!("{prefix}{alias}").to_ascii_lowercase();
                texture_indices.entry(alias.clone()).or_insert(index);
                texture_has_transparency
                    .entry(alias)
                    .or_insert(has_transparency);
            }
        }
    }

    let mut material_base = 0usize;
    for (model, _, prefix) in models {
        for material in &model.materials {
            let diffuse = material
                .texture_slots
                .iter()
                .find(|slot| slot.texture_type.eq_ignore_ascii_case("Diffuse"));
            let mut pbr = json!({ "metallicFactor": 0.0, "roughnessFactor": 1.0 });
            let diffuse_key =
                diffuse.map(|slot| format!("{prefix}{}", slot.name).to_ascii_lowercase());
            if let Some(texture) = diffuse.and_then(|slot| {
                texture_indices.get(&format!("{prefix}{}", slot.name).to_ascii_lowercase())
            }) {
                pbr["baseColorTexture"] = json!({
                    "index": texture,
                    "texCoord": diffuse.map_or(0, |slot| slot.uv_layer)
                });
            }
            let mut exported = json!({
                "name": format!("{prefix}{}", material.name),
                "pbrMetallicRoughness": pbr,
                "doubleSided": true
            });
            if diffuse_key
                .as_ref()
                .and_then(|key| texture_has_transparency.get(key))
                .copied()
                .unwrap_or(false)
            {
                exported["alphaMode"] = json!("MASK");
                exported["alphaCutoff"] = json!(0.5);
            }
            materials.push(exported);
        }

        let bone_node_base = nodes.len();
        let mut bone_worlds = Vec::with_capacity(model.render.bones.len());
        for (bone_index, bone) in model.render.bones.iter().enumerate() {
            let local = compose_matrix(bone.translation, bone.rotation, bone.scale);
            let world = if bone.parent_index >= 0 {
                bone_worlds
                    .get(bone.parent_index as usize)
                    .copied()
                    .map_or(local, |parent| multiply_matrix(parent, local))
            } else {
                local
            };
            bone_worlds.push(world);
            let children: Vec<_> = model
                .render
                .bones
                .iter()
                .enumerate()
                .filter_map(|(child, value)| {
                    (value.parent_index == bone_index as i16).then_some(bone_node_base + child)
                })
                .collect();
            let mut node = json!({
                "name": format!("{prefix}{}", bone.name),
                "translation": bone.translation,
                "rotation": normalize4(bone.rotation),
                "scale": bone.scale
            });
            if !children.is_empty() {
                node["children"] = json!(children);
            }
            nodes.push(node);
        }
        let skin_index = if model.render.bones.is_empty() {
            None
        } else {
            let matrices = bone_worlds
                .iter()
                .flat_map(|matrix| transpose(inverse_affine_matrix(*matrix)));
            let view = binary.push(&floats(matrices), None)?;
            let inverse_bind_accessor = accessors.len();
            accessors.push(accessor(view, 5126, bone_worlds.len(), "MAT4"));
            let joints: Vec<_> = (0..model.render.bones.len())
                .map(|index| bone_node_base + index)
                .collect();
            let index = skins.len();
            skins.push(json!({
                "name": format!("{prefix}Armature"),
                "joints": joints,
                "inverseBindMatrices": inverse_bind_accessor
            }));
            Some(index)
        };

        for mesh in &model.render.meshes {
            if mesh.positions.is_empty() || mesh.indices.is_empty() {
                continue;
            }
            let mut positions = mesh.positions.clone();
            let mut normals = mesh.normals.clone();
            if mesh.vertex_skin_count == 1 {
                for vertex in 0..positions.len() {
                    let bone = mesh
                        .bone_indices
                        .get(vertex)
                        .map(|indices| indices[0] as usize)
                        .unwrap_or(mesh.bone_index as usize);
                    if let Some(world) = bone_worlds.get(bone).copied() {
                        positions[vertex] = transform_point(world, positions[vertex]);
                        if let Some(normal) = normals.get_mut(vertex) {
                            *normal = transform_normal(world, *normal);
                        }
                    }
                }
            }
            let position_view =
                binary.push(&floats(positions.iter().flatten().copied()), Some(34962))?;
            let position_accessor = accessors.len();
            let mut position = accessor(position_view, 5126, positions.len(), "VEC3");
            let mut minimum = [f32::INFINITY; 3];
            let mut maximum = [f32::NEG_INFINITY; 3];
            for point in &positions {
                for axis in 0..3 {
                    minimum[axis] = minimum[axis].min(point[axis]);
                    maximum[axis] = maximum[axis].max(point[axis]);
                }
            }
            position["min"] = json!(minimum);
            position["max"] = json!(maximum);
            accessors.push(position);
            let mut attributes = Map::new();
            attributes.insert("POSITION".into(), position_accessor.into());

            if mesh.vertex_skin_count > 0 && skin_index.is_some() {
                let mut joints = Vec::with_capacity(mesh.positions.len() * 4);
                let mut weights = Vec::with_capacity(mesh.positions.len() * 4);
                for vertex in 0..mesh.positions.len() {
                    let indices = mesh.bone_indices.get(vertex).copied().unwrap_or([
                        mesh.bone_index,
                        0,
                        0,
                        0,
                    ]);
                    let values = if mesh.vertex_skin_count == 1 {
                        [1.0, 0.0, 0.0, 0.0]
                    } else {
                        mesh.bone_weights
                            .get(vertex)
                            .copied()
                            .unwrap_or([1.0, 0.0, 0.0, 0.0])
                    };
                    let (indices, values) = vertex_influences(
                        indices,
                        values,
                        mesh.bone_index,
                        model.render.bones.len(),
                    );
                    joints.extend(indices);
                    weights.extend(values);
                }
                let view = binary.push(&u16s(joints), Some(34962))?;
                let index = accessors.len();
                accessors.push(accessor(view, 5123, mesh.positions.len(), "VEC4"));
                attributes.insert("JOINTS_0".into(), index.into());
                let view = binary.push(&floats(weights), Some(34962))?;
                let index = accessors.len();
                accessors.push(accessor(view, 5126, mesh.positions.len(), "VEC4"));
                attributes.insert("WEIGHTS_0".into(), index.into());
            }

            if normals.len() == positions.len() {
                let view = binary.push(&floats(normals.iter().flatten().copied()), Some(34962))?;
                let index = accessors.len();
                accessors.push(accessor(view, 5126, normals.len(), "VEC3"));
                attributes.insert("NORMAL".into(), index.into());
            }
            let uv_maps = if mesh.uv_maps.is_empty() {
                std::slice::from_ref(&mesh.uv0)
            } else {
                &mesh.uv_maps
            };
            for (uv_index, uvs) in uv_maps.iter().enumerate() {
                if uvs.len() == positions.len() {
                    let view = binary.push(
                        &floats(uvs.iter().copied().map(convert_uv_for_glb).flatten()),
                        Some(34962),
                    )?;
                    let index = accessors.len();
                    accessors.push(accessor(view, 5126, uvs.len(), "VEC2"));
                    attributes.insert(format!("TEXCOORD_{uv_index}"), index.into());
                }
            }
            let index_view = binary.push(&u32s(mesh.indices.iter().copied()), Some(34963))?;
            let index_accessor = accessors.len();
            accessors.push(accessor(index_view, 5125, mesh.indices.len(), "SCALAR"));
            let mesh_index = meshes.len();
            meshes.push(json!({
                "name": format!("{prefix}{}", mesh.name),
                "primitives": [{
                    "attributes": Value::Object(attributes),
                    "indices": index_accessor,
                    "material": material_base + mesh.material_index as usize,
                    "mode": 4
                }]
            }));
            let mut node = json!({ "name": format!("{prefix}{}", mesh.name), "mesh": mesh_index });
            if mesh.vertex_skin_count > 0 {
                if let Some(skin) = skin_index {
                    node["skin"] = skin.into();
                }
            }
            nodes.push(node);
        }
        material_base += model.materials.len();
    }

    let child_nodes: std::collections::HashSet<usize> = nodes
        .iter()
        .filter_map(|node| node.get("children").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_u64)
        .map(|value| value as usize)
        .collect();
    let scene_nodes: Vec<_> = (0..nodes.len())
        .filter(|index| !child_nodes.contains(index))
        .collect();
    let mut document = json!({
        "asset": { "version": "2.0", "generator": "TotkBits" },
        "scene": 0,
        "scenes": [{ "name": "Scene", "nodes": scene_nodes }],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": binary.views,
        "buffers": [{ "byteLength": binary.data.position() }],
        "samplers": [{ "magFilter": 9729, "minFilter": 9987, "wrapS": 10497, "wrapT": 10497 }],
        "images": images,
        "textures": textures
    });
    if !skins.is_empty() {
        document["skins"] = json!(skins);
    }
    // Round-trip through gltf-json so malformed schema values fail before a file is written.
    let root: gltf_json::Root = serde_json::from_value(document.take())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut json_bytes = serde_json::to_vec(&root).map_err(io::Error::other)?;
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    binary.data.align(4)?;
    let binary_data = binary.data.into_inner();
    let total = 12 + 8 + json_bytes.len() + 8 + binary_data.len();
    let mut glb = BinaryWriter::new();
    glb.write_bytes(b"glTF");
    glb.write_u32(2);
    glb.write_u32(total as u32);
    glb.write_u32(json_bytes.len() as u32);
    glb.write_u32(0x4e4f534a);
    glb.write_bytes(&json_bytes);
    glb.write_u32(binary_data.len() as u32);
    glb.write_u32(0x004e4942);
    glb.write_bytes(&binary_data);
    fs::write(output, glb.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertex_groups_merge_and_normalize_weights() {
        let (joints, weights) = vertex_influences([7, 3, 7, 99], [0.25, 0.25, 0.5, 1.0], 2, 10);
        assert_eq!(joints, [7, 3, 0, 0]);
        assert!((weights[0] - 0.75).abs() < 1.0e-6);
        assert!((weights[1] - 0.25).abs() < 1.0e-6);
        assert_eq!(weights[2..], [0.0, 0.0]);

        let (joints, weights) = vertex_influences([99; 4], [f32::NAN, -1.0, 0.0, 1.0], 4, 10);
        assert_eq!(joints[0], 4);
        assert_eq!(weights, [1.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn uv_coordinates_match_blender_mirror_y() {
        assert_eq!(convert_uv_for_glb([0.25, 0.2]), [0.25, 0.2]);
        assert_eq!(convert_uv_for_glb([1.0, 1.0]), [1.0, 1.0]);
    }

    #[test]
    fn transparency_requires_a_fully_transparent_pixel() {
        let image = |alpha| {
            image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
                1,
                1,
                image::Rgba([255, 255, 255, alpha]),
            ))
        };
        assert!(!has_fully_transparent_pixel(&image(255)));
        assert!(!has_fully_transparent_pixel(&image(1)));
        assert!(!has_fully_transparent_pixel(&image(127)));
        assert!(has_fully_transparent_pixel(&image(0)));
    }

    #[test]
    fn exports_supplied_link_model_as_valid_glb() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tmp/g1m_importer/g1m_exporter/clean/22bd644e.g1m");
        if !source.is_file() {
            return;
        }
        let model = G1mFile::from_path(&source).unwrap();
        let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/g1m_glb_export_test.glb");
        export_g1m(&[(&model, &[], String::new())], &output).unwrap();
        let bytes = fs::read(&output).unwrap();
        assert_eq!(&bytes[..4], b"glTF");
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 2);
        assert_eq!(
            u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize,
            bytes.len()
        );
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let root = serde_json::from_slice::<gltf_json::Root>(&bytes[20..20 + json_len]).unwrap();
        if !model.render.bones.is_empty() {
            assert_eq!(root.skins.len(), 1);
            assert_eq!(root.skins[0].joints.len(), model.render.bones.len());
            assert!(root
                .meshes
                .iter()
                .flat_map(|mesh| &mesh.primitives)
                .any(|primitive| primitive.attributes.contains_key(
                    &gltf_json::validation::Checked::Valid(gltf_json::mesh::Semantic::Joints(0))
                )));
        }
        fs::remove_file(output).unwrap();
    }
}
