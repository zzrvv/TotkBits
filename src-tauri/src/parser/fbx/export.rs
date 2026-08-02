use crate::file_format::Model3D::bfres::{BfresBone, BfresMesh};
use crate::parser::AOC::g1m::{G1mFile, G1mMaterial, ResolvedG1tTexture};
use base64::Engine;
use image_dds::{ImageFormat, Mipmaps, Quality};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, BufReader, Cursor};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TextureExportFormat {
    None,
    Png,
    Dds,
}

impl TextureExportFormat {
    pub fn parse(value: &str) -> io::Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "none" => Ok(Self::None),
            "png" => Ok(Self::Png),
            "dds" => Ok(Self::Dds),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "texture format must be none, png, or dds",
            )),
        }
    }
}

struct ModelInput<'a> {
    model: &'a G1mFile,
    textures: &'a [ResolvedG1tTexture],
    prefix: String,
}

struct Ids {
    next: i64,
}

impl Ids {
    fn new() -> Self {
        Self { next: 1_000_000 }
    }

    fn take(&mut self) -> i64 {
        let value = self.next;
        self.next += 1;
        value
    }
}

struct TextureLink {
    id: i64,
    video_id: i64,
    material_id: i64,
    property: &'static str,
    name: String,
    relative_path: String,
    uv_set: String,
}

struct MeshLink {
    geometry_id: i64,
    model_id: i64,
    material_id: i64,
    skin_id: Option<i64>,
    clusters: Vec<(i64, usize)>,
}

pub fn export_g1m(
    models: &[(&G1mFile, &[ResolvedG1tTexture], String)],
    output: &Path,
    texture_format: TextureExportFormat,
    armature_name: &str,
) -> io::Result<()> {
    if models.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "no G1M models to export",
        ));
    }
    let inputs: Vec<_> = models
        .iter()
        .map(|(model, textures, prefix)| ModelInput {
            model,
            textures,
            prefix: prefix.clone(),
        })
        .collect();
    let texture_paths = export_textures(&inputs, output, texture_format)?;
    let ascii = build_ascii(&inputs, &texture_paths, armature_name);
    fs::write(output, ascii_to_binary(&ascii)?)
}

fn ascii_to_binary(ascii: &str) -> io::Result<Vec<u8>> {
    use fbxcel::{
        low::FbxVersion,
        writer::v7400::binary::{FbxFooter, Writer},
    };

    let tokenizer = fbxscii::Tokenizer::new(BufReader::new(ascii.as_bytes()));
    let arena = fbxscii::Parser::new(tokenizer).load().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid generated FBX: {error:?}"),
        )
    })?;
    let mut writer =
        Writer::new(Cursor::new(Vec::new()), FbxVersion::V7_4).map_err(fbx_writer_error)?;
    for (index, element) in arena.as_slice().iter().enumerate() {
        if element.parent_index.is_none() {
            write_binary_element(&mut writer, &arena, index)?;
        }
    }
    let output = writer
        .finalize(&FbxFooter::default())
        .map_err(fbx_writer_error)?;
    Ok(output.into_inner())
}

fn write_binary_element(
    writer: &mut fbxcel::writer::v7400::binary::Writer<Cursor<Vec<u8>>>,
    arena: &fbxscii::ElementAmphitheatre,
    index: usize,
) -> io::Result<()> {
    let element = arena
        .get(index)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid FBX element index"))?;
    let array_child = element
        .children
        .iter()
        .filter_map(|index| arena.get(*index))
        .find(|child| child.key == "a");
    {
        let mut attributes = writer.new_node(&element.key).map_err(fbx_writer_error)?;
        if let Some(array) = array_child {
            if matches!(
                element.key.as_str(),
                "PolygonVertexIndex" | "Indexes" | "Materials"
            ) {
                let values = array
                    .tokens
                    .iter()
                    .map(|value| {
                        value
                            .parse::<i32>()
                            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
                    })
                    .collect::<io::Result<Vec<_>>>()?;
                attributes
                    .append_arr_i32_from_iter(None, values)
                    .map_err(fbx_writer_error)?;
            } else {
                let values = array
                    .tokens
                    .iter()
                    .map(|value| {
                        value
                            .parse::<f64>()
                            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
                    })
                    .collect::<io::Result<Vec<_>>>()?;
                attributes
                    .append_arr_f64_from_iter(None, values)
                    .map_err(fbx_writer_error)?;
            }
        } else {
            for (token_index, token) in element.tokens.iter().enumerate() {
                append_binary_attribute(
                    &mut attributes,
                    &element.key,
                    token_index,
                    token,
                    &element.tokens,
                )?;
            }
        }
    }
    if array_child.is_none() {
        for &child in &element.children {
            write_binary_element(writer, arena, child)?;
        }
    }
    writer.close_node().map_err(fbx_writer_error)
}

fn append_binary_attribute(
    attributes: &mut fbxcel::writer::v7400::binary::AttributesWriter<'_, Cursor<Vec<u8>>>,
    node: &str,
    index: usize,
    value: &str,
    node_tokens: &[String],
) -> io::Result<()> {
    let is_object = matches!(
        node,
        "Geometry" | "Model" | "Material" | "Texture" | "Video" | "Deformer" | "NodeAttribute"
    );
    if is_object && index == 1 {
        let binary_name = value
            .split_once("::")
            .map(|(class, name)| format!("{name}\0\u{1}{class}"))
            .unwrap_or_else(|| value.to_owned());
        return attributes
            .append_string_direct(&binary_name)
            .map_err(fbx_writer_error);
    }
    if value == "T" || value == "F" {
        return attributes
            .append_bool(value == "T")
            .map_err(fbx_writer_error);
    }
    let is_object_id = index == 0
        && matches!(
            node,
            "Geometry"
                | "Model"
                | "Material"
                | "Texture"
                | "Video"
                | "Deformer"
                | "NodeAttribute"
                | "Document"
        );
    let is_connection_id = node == "C" && matches!(index, 1 | 2);
    let is_root_node_id = node == "RootNode" && index == 0;
    let is_pose_node_id = node == "Node" && index == 0;
    if is_object_id || is_connection_id || is_root_node_id || is_pose_node_id {
        let value = value
            .parse::<i64>()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        return attributes.append_i64(value).map_err(fbx_writer_error);
    }
    // Properties70 values are strongly typed. In particular, Blender rejects
    // integral-looking literals (such as UnitScaleFactor = 1) when the FBX
    // property declaration says that the value is a double.
    if node == "P" && index >= 4 {
        let property_type = node_tokens.get(1).map(String::as_str).unwrap_or("");
        if !matches!(
            property_type,
            "int" | "Integer" | "bool" | "Bool" | "enum" | "Enum"
        ) {
            let value = value
                .parse::<f64>()
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            return attributes.append_f64(value).map_err(fbx_writer_error);
        }
    }
    if let Ok(value) = value.parse::<i32>() {
        return attributes.append_i32(value).map_err(fbx_writer_error);
    }
    if let Ok(value) = value.parse::<i64>() {
        return attributes.append_i64(value).map_err(fbx_writer_error);
    }
    if let Ok(value) = value.parse::<f64>() {
        return attributes.append_f64(value).map_err(fbx_writer_error);
    }
    attributes
        .append_string_direct(value)
        .map_err(fbx_writer_error)
}

fn fbx_writer_error(error: fbxcel::writer::v7400::binary::Error) -> io::Error {
    io::Error::other(error.to_string())
}

fn export_textures(
    models: &[ModelInput<'_>],
    output: &Path,
    format: TextureExportFormat,
) -> io::Result<BTreeMap<String, String>> {
    let mut paths = BTreeMap::new();
    if format == TextureExportFormat::None {
        return Ok(paths);
    }
    let stem = output
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("model");
    let folder_name = format!("{stem}_textures");
    let folder = output.parent().unwrap_or_else(|| Path::new("."));
    // .join(&folder_name);
    fs::create_dir_all(&folder)?;
    let extension = if format == TextureExportFormat::Png {
        "png"
    } else if format == TextureExportFormat::Dds {
        "dds"
    } else {
        "png"
    };
    let mut used = BTreeSet::new();
    for input in models {
        for texture in input.textures {
            let key = texture_key(&input.prefix, &texture.name);
            if paths.contains_key(&key) {
                continue;
            }
            let base = safe_name(&format!("{}{}", input.prefix, texture.name));
            let mut filename = format!("{base}.{extension}");
            let mut suffix = 2;
            while !used.insert(filename.to_ascii_lowercase()) {
                filename = format!("{base}_{suffix}.{extension}");
                suffix += 1;
            }
            let png = decode_data_url(&texture.data_url)?;
            let bytes = if format == TextureExportFormat::Png {
                png
            } else {
                png_to_dds(&png)?
            };
            fs::write(folder.join(&filename), bytes)?;
            paths.insert(key, format!("{filename}"));
        }
    }
    Ok(paths)
}

fn decode_data_url(value: &str) -> io::Result<Vec<u8>> {
    let encoded = value
        .split_once(',')
        .map(|(_, encoded)| encoded)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid texture data URL"))?;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
}

fn png_to_dds(png: &[u8]) -> io::Result<Vec<u8>> {
    let image = image::load_from_memory(png)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?
        .to_rgba8();
    let dds = image_dds::dds_from_image(
        &image,
        ImageFormat::Rgba8Unorm,
        Quality::Normal,
        Mipmaps::Disabled,
    )
    .map_err(|error| io::Error::other(error.to_string()))?;
    let mut bytes = Vec::new();
    dds.write(&mut bytes)
        .map_err(|error| io::Error::other(error.to_string()))?;
    Ok(bytes)
}

fn build_ascii(
    models: &[ModelInput<'_>],
    texture_paths: &BTreeMap<String, String>,
    armature_name: &str,
) -> String {
    let mut ids = Ids::new();
    let rotation_root_id = ids.take();
    let root_id = ids.take();
    let document_id = ids.take();
    let mut bone_ids = Vec::with_capacity(models.len());
    let mut bone_attribute_ids = Vec::with_capacity(models.len());
    let mut material_ids = Vec::with_capacity(models.len());
    let mut mesh_links = Vec::new();
    let mut texture_links = Vec::new();
    for input in models {
        bone_ids.push(
            (0..input.model.render.bones.len())
                .map(|_| ids.take())
                .collect::<Vec<_>>(),
        );
        bone_attribute_ids.push(
            (0..input.model.render.bones.len())
                .map(|_| ids.take())
                .collect::<Vec<_>>(),
        );
        material_ids.push(
            (0..input.model.materials.len())
                .map(|_| ids.take())
                .collect::<Vec<_>>(),
        );
    }

    let mut objects = String::new();
    write_model_object(
        &mut objects,
        rotation_root_id,
        "G1M_Orientation",
        "Null",
        [0.0; 3],
        [90.0, 0.0, 0.0],
        [1.0; 3],
    );
    write_model_object(
        &mut objects,
        root_id,
        armature_name,
        "Null",
        [0.0; 3],
        [0.0; 3],
        [1.0; 3],
    );
    for (model_index, input) in models.iter().enumerate() {
        for (index, bone) in input.model.render.bones.iter().enumerate() {
            let rotation = quaternion_euler_degrees(bone.rotation);
            write_model_object(
                &mut objects,
                bone_ids[model_index][index],
                &format!("{}{}", input.prefix, bone.name),
                "LimbNode",
                bone.translation,
                rotation,
                bone.scale,
            );
            write_bone_attribute(
                &mut objects,
                bone_attribute_ids[model_index][index],
                &format!("{}{}", input.prefix, bone.name),
            );
        }
        for (index, material) in input.model.materials.iter().enumerate() {
            write_material(
                &mut objects,
                material_ids[model_index][index],
                &format!("{}{}", input.prefix, material.name),
            );
            for (property, slot) in material_texture_slots(material) {
                let Some(relative_path) =
                    texture_paths.get(&texture_key(&input.prefix, &slot.name))
                else {
                    continue;
                };
                texture_links.push(TextureLink {
                    id: ids.take(),
                    video_id: ids.take(),
                    material_id: material_ids[model_index][index],
                    property,
                    name: format!("{}{}", input.prefix, slot.name),
                    relative_path: relative_path.clone(),
                    uv_set: format!("UVChannel_{}", slot.uv_layer as usize + 1),
                });
            }
        }
        for mesh in &input.model.render.meshes {
            let geometry_id = ids.take();
            let model_id = ids.take();
            let used_bones = mesh_bones(mesh, input.model.render.bones.len());
            let skin_id = (!used_bones.is_empty()).then(|| ids.take());
            let clusters: Vec<(i64, usize)> = used_bones
                .into_iter()
                .map(|bone| (ids.take(), bone))
                .collect();
            write_geometry(&mut objects, geometry_id, mesh, &input.model.render.bones);
            write_model_object(
                &mut objects,
                model_id,
                &format!("{}{}", input.prefix, mesh.name),
                "Mesh",
                [0.0; 3],
                [0.0; 3],
                [1.0; 3],
            );
            if let Some(skin) = skin_id {
                writeln!(
                    objects,
                    "    Deformer: {skin}, \"Deformer::Skin\", \"Skin\" {{"
                )
                .unwrap();
                writeln!(objects, "        Version: 101").unwrap();
                writeln!(objects, "        Link_DeformAcuracy: 50").unwrap();
                writeln!(objects, "    }}").unwrap();
                for &(cluster, bone) in &clusters {
                    write_cluster(&mut objects, cluster, mesh, bone, &input.model.render.bones);
                }
            }
            let material_id = material_ids[model_index]
                .get(mesh.material_index as usize)
                .copied()
                .unwrap_or(root_id);
            mesh_links.push(MeshLink {
                geometry_id,
                model_id,
                material_id,
                skin_id,
                clusters,
            });
        }
    }
    for texture in &texture_links {
        write_texture(&mut objects, texture);
    }

    let mut connections = String::new();
    connection(&mut connections, rotation_root_id, 0);
    connection(&mut connections, root_id, rotation_root_id);
    for (model_index, input) in models.iter().enumerate() {
        for (index, bone) in input.model.render.bones.iter().enumerate() {
            connection(
                &mut connections,
                bone_attribute_ids[model_index][index],
                bone_ids[model_index][index],
            );
            let parent = if bone.parent_index >= 0 {
                bone_ids[model_index]
                    .get(bone.parent_index as usize)
                    .copied()
                    .unwrap_or(root_id)
            } else {
                root_id
            };
            connection(&mut connections, bone_ids[model_index][index], parent);
        }
    }
    let mut mesh_cursor = 0;
    for (model_index, input) in models.iter().enumerate() {
        for _mesh in &input.model.render.meshes {
            let link = &mesh_links[mesh_cursor];
            connection(&mut connections, link.geometry_id, link.model_id);
            connection(&mut connections, link.model_id, root_id);
            connection(&mut connections, link.material_id, link.model_id);
            if let Some(skin) = link.skin_id {
                connection(&mut connections, skin, link.geometry_id);
                for &(cluster, bone) in &link.clusters {
                    connection(&mut connections, cluster, skin);
                    connection(&mut connections, bone_ids[model_index][bone], cluster);
                }
            }
            mesh_cursor += 1;
        }
    }
    for texture in &texture_links {
        connection(&mut connections, texture.video_id, texture.id);
        property_connection(
            &mut connections,
            texture.id,
            texture.material_id,
            texture.property,
        );
        if texture.property == "DiffuseColor" {
            property_connection(
                &mut connections,
                texture.id,
                texture.material_id,
                "TransparentColor",
            );
        }
    }

    let bone_count = models
        .iter()
        .map(|input| input.model.render.bones.len())
        .sum::<usize>();
    let mesh_count = mesh_links.len();
    let material_count = models
        .iter()
        .map(|input| input.model.materials.len())
        .sum::<usize>();
    let deformer_count = mesh_links
        .iter()
        .map(|link| usize::from(link.skin_id.is_some()) + link.clusters.len())
        .sum::<usize>();
    let definition_count = 2
        + bone_count * 2
        + mesh_count * 2
        + material_count
        + texture_links.len() * 2
        + deformer_count;

    let ascii = format!(
        "; FBX 7.4.0 project file\nFBXHeaderExtension:  {{\n    FBXHeaderVersion: 1003\n    FBXVersion: 7400\n    Creator: \"TotkBits\"\n}}\nGlobalSettings:  {{\n    Version: 1000\n    Properties70:  {{\n        P: \"UpAxis\", \"int\", \"Integer\", \"\",1\n        P: \"UpAxisSign\", \"int\", \"Integer\", \"\",1\n        P: \"FrontAxis\", \"int\", \"Integer\", \"\",2\n        P: \"FrontAxisSign\", \"int\", \"Integer\", \"\",-1\n        P: \"CoordAxis\", \"int\", \"Integer\", \"\",0\n        P: \"CoordAxisSign\", \"int\", \"Integer\", \"\",1\n        P: \"UnitScaleFactor\", \"double\", \"Number\", \"\",1\n    }}\n}}\nDocuments:  {{\n    Count: 1\n    Document: {document_id}, \"Scene\", \"Scene\" {{\n        Properties70:  {{}}\n        RootNode: {rotation_root_id}\n    }}\n}}\nReferences:  {{}}\nDefinitions:  {{\n    Version: 100\n    Count: 7\n    ObjectType: \"Model\" {{ Count: {} }}\n    ObjectType: \"NodeAttribute\" {{ Count: {bone_count} }}\n    ObjectType: \"Geometry\" {{ Count: {mesh_count} }}\n    ObjectType: \"Material\" {{ Count: {material_count} }}\n    ObjectType: \"Texture\" {{ Count: {} }}\n    ObjectType: \"Video\" {{ Count: {} }}\n    ObjectType: \"Deformer\" {{ Count: {deformer_count} }}\n}}\nObjects:  {{\n{objects}}}\nConnections:  {{\n{connections}}}\n",
        2 + bone_count + mesh_count,
        texture_links.len(),
        texture_links.len(),
    );
    ascii.replacen(
        "    Count: 7\n    ObjectType:",
        &format!("    Count: {definition_count}\n    ObjectType:"),
        1,
    )
}

fn write_bone_attribute(out: &mut String, id: i64, name: &str) {
    writeln!(
        out,
        "    NodeAttribute: {id}, \"NodeAttribute::{}\", \"LimbNode\" {{",
        escaped(name)
    )
    .unwrap();
    writeln!(out, "        TypeFlags: \"Skeleton\"").unwrap();
    writeln!(out, "    }}").unwrap();
}

fn write_geometry(out: &mut String, id: i64, mesh: &BfresMesh, bones: &[BfresBone]) {
    let (positions, normals) = model_space_geometry(mesh, bones);
    writeln!(
        out,
        "    Geometry: {id}, \"Geometry::{}\", \"Mesh\" {{",
        escaped(&mesh.name)
    )
    .unwrap();
    write_vec3_array(out, "Vertices", &positions);
    let polygons: Vec<i64> = mesh
        .indices
        .chunks_exact(3)
        .flat_map(|triangle| {
            [
                triangle[0] as i64,
                triangle[1] as i64,
                -(triangle[2] as i64) - 1,
            ]
        })
        .collect();
    write_i64_array(out, "PolygonVertexIndex", &polygons);
    if normals.len() == positions.len() {
        writeln!(out, "        LayerElementNormal: 0 {{").unwrap();
        writeln!(out, "            Version: 101").unwrap();
        writeln!(out, "            Name: \"Normals\"").unwrap();
        writeln!(out, "            MappingInformationType: \"ByVertice\"").unwrap();
        writeln!(out, "            ReferenceInformationType: \"Direct\"").unwrap();
        write_vec3_array_indented(out, "Normals", &normals, 12);
        writeln!(out, "        }}").unwrap();
    }
    let uv_maps = if mesh.uv_maps.is_empty() {
        std::slice::from_ref(&mesh.uv0)
    } else {
        &mesh.uv_maps
    };
    let valid_uvs: Vec<_> = uv_maps
        .iter()
        .filter(|uv| uv.len() == positions.len())
        .collect();
    for (index, uv) in valid_uvs.iter().enumerate() {
        writeln!(out, "        LayerElementUV: {index} {{").unwrap();
        writeln!(out, "            Version: 101").unwrap();
        writeln!(out, "            Name: \"UVChannel_{}\"", index + 1).unwrap();
        writeln!(out, "            MappingInformationType: \"ByVertice\"").unwrap();
        writeln!(out, "            ReferenceInformationType: \"Direct\"").unwrap();
        write_uv_array(out, "UV", uv);
        writeln!(out, "        }}").unwrap();
    }
    writeln!(out, "        LayerElementMaterial: 0 {{").unwrap();
    writeln!(out, "            Version: 101").unwrap();
    writeln!(out, "            Name: \"\"").unwrap();
    writeln!(out, "            MappingInformationType: \"AllSame\"").unwrap();
    writeln!(
        out,
        "            ReferenceInformationType: \"IndexToDirect\""
    )
    .unwrap();
    writeln!(out, "            Materials: *1 {{ a: 0 }}").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        Layer: 0 {{").unwrap();
    if normals.len() == positions.len() {
        layer_element(out, "LayerElementNormal", 0);
    }
    if !valid_uvs.is_empty() {
        layer_element(out, "LayerElementUV", 0);
    }
    layer_element(out, "LayerElementMaterial", 0);
    writeln!(out, "        }}").unwrap();
    for index in 1..valid_uvs.len() {
        writeln!(out, "        Layer: {index} {{").unwrap();
        layer_element(out, "LayerElementUV", index);
        writeln!(out, "        }}").unwrap();
    }
    writeln!(out, "    }}").unwrap();
}

fn write_model_object(
    out: &mut String,
    id: i64,
    name: &str,
    kind: &str,
    translation: [f32; 3],
    rotation: [f32; 3],
    scale: [f32; 3],
) {
    writeln!(
        out,
        "    Model: {id}, \"Model::{}\", \"{kind}\" {{",
        escaped(name)
    )
    .unwrap();
    writeln!(out, "        Version: 232").unwrap();
    writeln!(out, "        Properties70:  {{").unwrap();
    property_vec3(out, "Lcl Translation", "Lcl Translation", translation);
    property_vec3(out, "Lcl Rotation", "Lcl Rotation", rotation);
    property_vec3(out, "Lcl Scaling", "Lcl Scaling", scale);
    if kind == "Mesh" {
        property_vec3(out, "GeometricRotation", "Vector3D", [-90.0, 0.0, 0.0]);
    }
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        Shading: T").unwrap();
    writeln!(out, "        Culling: \"CullingOff\"").unwrap();
    writeln!(out, "    }}").unwrap();
}

fn write_material(out: &mut String, id: i64, name: &str) {
    writeln!(
        out,
        "    Material: {id}, \"Material::{}\", \"\" {{",
        escaped(name)
    )
    .unwrap();
    writeln!(out, "        Version: 102").unwrap();
    writeln!(out, "        ShadingModel: \"phong\"").unwrap();
    writeln!(out, "        MultiLayer: 0").unwrap();
    writeln!(out, "        Properties70:  {{").unwrap();
    writeln!(
        out,
        "            P: \"DiffuseColor\", \"Color\", \"\", \"A\",0.8,0.8,0.8"
    )
    .unwrap();
    writeln!(
        out,
        "            P: \"TransparentColor\", \"Color\", \"\", \"A\",1,1,1"
    )
    .unwrap();
    writeln!(
        out,
        "            P: \"TransparencyFactor\", \"Number\", \"\", \"A\",0"
    )
    .unwrap();
    writeln!(out, "            P: \"Opacity\", \"Number\", \"\", \"A\",1").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
}

fn write_texture(out: &mut String, texture: &TextureLink) {
    let path = texture.relative_path.replace('/', "\\");
    writeln!(
        out,
        "    Video: {}, \"Video::{}\", \"Clip\" {{",
        texture.video_id,
        escaped(&texture.name)
    )
    .unwrap();
    writeln!(out, "        Type: \"Clip\"").unwrap();
    writeln!(out, "        FileName: \"{}\"", escaped(&path)).unwrap();
    writeln!(out, "        RelativeFilename: \"{}\"", escaped(&path)).unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(
        out,
        "    Texture: {}, \"Texture::{}\", \"\" {{",
        texture.id,
        escaped(&texture.name)
    )
    .unwrap();
    writeln!(out, "        Type: \"TextureVideoClip\"").unwrap();
    writeln!(out, "        Version: 202").unwrap();
    writeln!(
        out,
        "        TextureName: \"Texture::{}\"",
        escaped(&texture.name)
    )
    .unwrap();
    writeln!(out, "        Media: \"Video::{}\"", escaped(&texture.name)).unwrap();
    writeln!(out, "        FileName: \"{}\"", escaped(&path)).unwrap();
    writeln!(out, "        RelativeFilename: \"{}\"", escaped(&path)).unwrap();
    writeln!(out, "        UVSet: \"{}\"", texture.uv_set).unwrap();
    writeln!(out, "        AlphaSource: \"Black\"").unwrap();
    writeln!(out, "    }}").unwrap();
}

fn write_cluster(out: &mut String, id: i64, mesh: &BfresMesh, bone: usize, bones: &[BfresBone]) {
    let mut indices = Vec::new();
    let mut weights = Vec::new();
    for vertex in 0..mesh.positions.len() {
        let mut weight = 0.0f32;
        if mesh.vertex_skin_count == 1 {
            let index = mesh
                .bone_indices
                .get(vertex)
                .map(|v| v[0] as usize)
                .unwrap_or(mesh.bone_index as usize);
            if index == bone {
                weight = 1.0;
            }
        } else {
            for influence in 0..4 {
                if mesh
                    .bone_indices
                    .get(vertex)
                    .is_some_and(|v| v[influence] as usize == bone)
                {
                    weight += mesh
                        .bone_weights
                        .get(vertex)
                        .map_or(if influence == 0 { 1.0 } else { 0.0 }, |v| v[influence]);
                }
            }
        }
        if weight > 0.0 {
            indices.push(vertex as i64);
            weights.push(weight as f64);
        }
    }
    writeln!(
        out,
        "    Deformer: {id}, \"SubDeformer::Cluster_{bone}\", \"Cluster\" {{"
    )
    .unwrap();
    writeln!(out, "        Version: 100").unwrap();
    write_i64_array(out, "Indexes", &indices);
    write_f64_array(out, "Weights", &weights, 8);
    let bone_world = bone_world_matrix(bones, bone);
    write_matrix(out, "Transform", inverse_affine_matrix(bone_world));
    write_matrix(out, "TransformLink", bone_world);
    write_matrix(out, "TransformAssociateModel", identity_matrix());
    writeln!(out, "    }}").unwrap();
}

fn mesh_bones(mesh: &BfresMesh, bone_count: usize) -> Vec<usize> {
    if mesh.vertex_skin_count > 0 {
        // A cluster also carries a bone's bind matrix. Blender's FBX exporter
        // deliberately emits zero-weight clusters for every armature bone so
        // importers can reconstruct the complete rest hierarchy instead of
        // guessing transforms for unweighted ancestors and siblings.
        return (0..bone_count).collect();
    }
    Vec::new()
}

fn material_texture_slots(
    material: &G1mMaterial,
) -> Vec<(&'static str, &crate::parser::AOC::g1m::G1mTextureSlot)> {
    let mut result = Vec::with_capacity(2);
    if let Some(slot) = material
        .texture_slots
        .iter()
        .find(|slot| slot.texture_type == "Diffuse")
    {
        result.push(("DiffuseColor", slot));
    }
    if let Some(slot) = material
        .texture_slots
        .iter()
        .find(|slot| slot.texture_type == "Normal")
    {
        result.push(("NormalMap", slot));
    }
    result
}

fn model_space_geometry(mesh: &BfresMesh, bones: &[BfresBone]) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    if mesh.vertex_skin_count != 1 || bones.is_empty() {
        return (mesh.positions.clone(), mesh.normals.clone());
    }
    let worlds: Vec<_> = (0..bones.len())
        .map(|index| bone_world_matrix(bones, index))
        .collect();
    let mut positions = mesh.positions.clone();
    let mut normals = mesh.normals.clone();
    for vertex in 0..positions.len() {
        let bone = mesh
            .bone_indices
            .get(vertex)
            .map(|value| value[0] as usize)
            .unwrap_or(mesh.bone_index as usize);
        let Some(matrix) = worlds.get(bone) else {
            continue;
        };
        positions[vertex] = transform_point(*matrix, positions[vertex]);
        if let Some(normal) = normals.get_mut(vertex) {
            *normal = transform_normal(*matrix, *normal);
        }
    }
    (positions, normals)
}

fn bone_world_matrix(bones: &[BfresBone], index: usize) -> [f64; 16] {
    let Some(bone) = bones.get(index) else {
        return identity_matrix();
    };
    let local = compose_matrix(bone.translation, bone.rotation, bone.scale);
    if bone.parent_index >= 0 {
        multiply_matrix(bone_world_matrix(bones, bone.parent_index as usize), local)
    } else {
        local
    }
}

fn compose_matrix(t: [f32; 3], q: [f32; 4], s: [f32; 3]) -> [f64; 16] {
    let [x, y, z, w] = normalize4(q);
    let (x, y, z, w) = (x as f64, y as f64, z as f64, w as f64);
    let (sx, sy, sz) = (s[0] as f64, s[1] as f64, s[2] as f64);
    [
        (1.0 - 2.0 * (y * y + z * z)) * sx,
        (2.0 * (x * y - z * w)) * sy,
        (2.0 * (x * z + y * w)) * sz,
        t[0] as f64,
        (2.0 * (x * y + z * w)) * sx,
        (1.0 - 2.0 * (x * x + z * z)) * sy,
        (2.0 * (y * z - x * w)) * sz,
        t[1] as f64,
        (2.0 * (x * z - y * w)) * sx,
        (2.0 * (y * z + x * w)) * sy,
        (1.0 - 2.0 * (x * x + y * y)) * sz,
        t[2] as f64,
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

fn multiply_matrix(a: [f64; 16], b: [f64; 16]) -> [f64; 16] {
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

fn inverse_affine_matrix(m: [f64; 16]) -> [f64; 16] {
    let determinant = m[0] * (m[5] * m[10] - m[6] * m[9]) - m[1] * (m[4] * m[10] - m[6] * m[8])
        + m[2] * (m[4] * m[9] - m[5] * m[8]);
    if determinant.abs() <= f64::EPSILON {
        return identity_matrix();
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
    let translation = [m[3], m[7], m[11]];
    [
        inverse[0],
        inverse[1],
        inverse[2],
        -(inverse[0] * translation[0] + inverse[1] * translation[1] + inverse[2] * translation[2]),
        inverse[3],
        inverse[4],
        inverse[5],
        -(inverse[3] * translation[0] + inverse[4] * translation[1] + inverse[5] * translation[2]),
        inverse[6],
        inverse[7],
        inverse[8],
        -(inverse[6] * translation[0] + inverse[7] * translation[1] + inverse[8] * translation[2]),
        0.0,
        0.0,
        0.0,
        1.0,
    ]
}

fn transform_point(m: [f64; 16], p: [f32; 3]) -> [f32; 3] {
    [
        (m[0] * p[0] as f64 + m[1] * p[1] as f64 + m[2] * p[2] as f64 + m[3]) as f32,
        (m[4] * p[0] as f64 + m[5] * p[1] as f64 + m[6] * p[2] as f64 + m[7]) as f32,
        (m[8] * p[0] as f64 + m[9] * p[1] as f64 + m[10] * p[2] as f64 + m[11]) as f32,
    ]
}

fn transform_normal(m: [f64; 16], normal: [f32; 3]) -> [f32; 3] {
    let determinant = m[0] * (m[5] * m[10] - m[6] * m[9]) - m[1] * (m[4] * m[10] - m[6] * m[8])
        + m[2] * (m[4] * m[9] - m[5] * m[8]);
    if determinant.abs() <= f64::EPSILON {
        return normalize3(normal);
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
    normalize3([
        (inverse[0] * normal[0] as f64
            + inverse[3] * normal[1] as f64
            + inverse[6] * normal[2] as f64) as f32,
        (inverse[1] * normal[0] as f64
            + inverse[4] * normal[1] as f64
            + inverse[7] * normal[2] as f64) as f32,
        (inverse[2] * normal[0] as f64
            + inverse[5] * normal[1] as f64
            + inverse[8] * normal[2] as f64) as f32,
    ])
}

fn quaternion_euler_degrees(q: [f32; 4]) -> [f32; 3] {
    let [x, y, z, w] = normalize4(q);
    let r00 = 1.0 - 2.0 * (y * y + z * z);
    let r10 = 2.0 * (x * y + z * w);
    let r11 = 1.0 - 2.0 * (x * x + z * z);
    let r12 = 2.0 * (y * z - x * w);
    let r20 = 2.0 * (x * z - y * w);
    let r21 = 2.0 * (y * z + x * w);
    let r22 = 1.0 - 2.0 * (x * x + y * y);
    let pitch = (-r20).clamp(-1.0, 1.0).asin();
    let (roll, yaw) = if pitch.cos().abs() > 1.0e-5 {
        (r21.atan2(r22), r10.atan2(r00))
    } else {
        // At +/-90 degrees, roll and yaw describe the same degree of
        // freedom. Pin yaw to zero instead of evaluating atan2(0, 0), which
        // otherwise introduces a spurious 180-degree pose rotation.
        ((-r12).atan2(r11), 0.0)
    };
    [roll.to_degrees(), pitch.to_degrees(), yaw.to_degrees()]
}

fn normalize4(q: [f32; 4]) -> [f32; 4] {
    let length = q.iter().map(|value| value * value).sum::<f32>().sqrt();
    if length <= f32::EPSILON {
        [0.0, 0.0, 0.0, 1.0]
    } else {
        q.map(|value| value / length)
    }
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let length = v.iter().map(|value| value * value).sum::<f32>().sqrt();
    if length <= f32::EPSILON {
        v
    } else {
        v.map(|value| value / length)
    }
}

fn identity_matrix() -> [f64; 16] {
    [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
}

fn write_vec3_array(out: &mut String, name: &str, values: &[[f32; 3]]) {
    write_vec3_array_indented(out, name, values, 8);
}
fn write_vec3_array_indented(out: &mut String, name: &str, values: &[[f32; 3]], indent: usize) {
    let flat: Vec<f64> = values
        .iter()
        .flat_map(|value| value.map(f64::from))
        .collect();
    write_f64_array(out, name, &flat, indent);
}
fn write_uv_array(out: &mut String, name: &str, values: &[[f32; 2]]) {
    let flat: Vec<f64> = values
        .iter()
        .flat_map(|value| [value[0] as f64, 1.0 - value[1] as f64])
        .collect();
    write_f64_array(out, name, &flat, 12);
}
fn write_i64_array(out: &mut String, name: &str, values: &[i64]) {
    writeln!(out, "        {name}: *{} {{", values.len()).unwrap();
    write!(out, "            a: ").unwrap();
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write!(out, "{value}").unwrap();
    }
    writeln!(out, "\n        }}").unwrap();
}
fn write_f64_array(out: &mut String, name: &str, values: &[f64], indent: usize) {
    let spaces = " ".repeat(indent);
    writeln!(out, "{spaces}{name}: *{} {{", values.len()).unwrap();
    write!(out, "{spaces}    a: ").unwrap();
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write!(out, "{value:.9}").unwrap();
    }
    writeln!(out, "\n{spaces}}}").unwrap();
}
fn write_matrix(out: &mut String, name: &str, values: [f64; 16]) {
    // FBX stores transform matrices transposed relative to the column-vector
    // matrices used by the viewer and the geometry conversion helpers.
    let mut fbx = [0.0; 16];
    for row in 0..4 {
        for column in 0..4 {
            fbx[row * 4 + column] = values[column * 4 + row];
        }
    }
    write_f64_array(out, name, &fbx, 8);
}
fn property_vec3(out: &mut String, name: &str, kind: &str, value: [f32; 3]) {
    writeln!(
        out,
        "            P: \"{name}\", \"{kind}\", \"\", \"A\",{},{},{}",
        value[0], value[1], value[2]
    )
    .unwrap();
}
fn layer_element(out: &mut String, kind: &str, index: usize) {
    writeln!(out, "            LayerElement:  {{").unwrap();
    writeln!(out, "                Type: \"{kind}\"").unwrap();
    writeln!(out, "                TypedIndex: {index}").unwrap();
    writeln!(out, "            }}").unwrap();
}
fn connection(out: &mut String, child: i64, parent: i64) {
    writeln!(out, "    C: \"OO\",{child},{parent}").unwrap();
}
fn property_connection(out: &mut String, child: i64, parent: i64, property: &str) {
    writeln!(out, "    C: \"OP\",{child},{parent},\"{property}\"").unwrap();
}
fn texture_key(prefix: &str, name: &str) -> String {
    format!("{prefix}\0{name}")
}
fn safe_name(value: &str) -> String {
    let result: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if result.is_empty() {
        "texture".into()
    } else {
        result
    }
}
fn escaped(value: &str) -> String {
    value
        .replace('\\', "_")
        .replace('"', "'")
        .replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_g1m_geometry_skeleton_and_skinning() {
        let source = std::env::var_os("TOTKBITS_TEST_G1M")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/g1m/038bb045.g1m")
            });
        if !source.is_file() {
            return;
        }
        let data = fs::read(&source).unwrap();
        let model = G1mFile::parse_for_export(&data, "038bb045").unwrap();
        let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/g1m_fbx_export_test.fbx");
        export_g1m(
            &[(&model, &[], String::new())],
            &output,
            TextureExportFormat::None,
            source.file_stem().and_then(|stem| stem.to_str()).unwrap(),
        )
        .unwrap();
        let exported = fs::read(&output).unwrap();
        assert!(exported.starts_with(b"Kaydara FBX Binary"));
        let parsed = crate::parser::fbx::FbxFile::parse(&exported, "roundtrip").unwrap();
        assert_eq!(parsed.render.meshes.len(), model.render.meshes.len());
        if std::env::var_os("TOTKBITS_KEEP_TEST_FBX").is_none() {
            fs::remove_file(output).unwrap();
        }
    }
}
