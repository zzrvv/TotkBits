use crate::file_format::Model3D::bfres::{BfresMesh, BfresRenderGraph};
use crate::parser::binary::BinaryReader;
use fbxcel_dom::{
    any::AnyDocument,
    v7400::object::{geometry::TypedGeometryHandle, TypedObjectHandle},
};
use serde::Serialize;
use std::{io, path::Path};

#[derive(Debug, Serialize)]
pub struct FbxHeader {
    pub version: [u8; 4],
    pub endian: String,
    pub target_address_size: u8,
    pub alignment_exponent: u8,
    pub file_size: u64,
    pub string_pool_offset: u64,
    pub string_pool_size: u64,
}
#[derive(Debug, Serialize)]
pub struct FbxMaterial {
    pub name: String,
    pub offset: u64,
    pub texture_slots: Vec<FbxTextureSlot>,
}
#[derive(Debug, Serialize)]
pub struct FbxTextureSlot {
    pub index: usize,
    pub name: String,
    pub texture_type: String,
}
#[derive(Debug, Serialize)]
pub struct FbxFile {
    pub header: FbxHeader,
    pub name: Option<String>,
    pub sections: Vec<FbxSection>,
    pub materials: Vec<FbxMaterial>,
    pub render: BfresRenderGraph,
    pub format: String,
}
#[derive(Debug, Serialize)]
pub struct FbxSection {
    pub signature: [u8; 4],
    pub offset: u64,
    pub name: Option<String>,
}

impl FbxFile {
    pub fn from_path(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        let data = std::fs::read(path)?;
        Self::parse(
            &data,
            path.file_stem().and_then(|v| v.to_str()).unwrap_or("FBX"),
        )
    }
    pub fn parse(data: &[u8], name: &str) -> io::Result<Self> {
        let version = BinaryReader::new(data)
            .read_u32_at(23)
            .map_err(|_| invalid("truncated FBX header"))?;
        let document = AnyDocument::from_seekable_reader(std::io::Cursor::new(data))
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        let AnyDocument::V7400(_, document) = document else {
            return Err(invalid("unsupported FBX document version"));
        };
        let mut meshes = Vec::new();
        for object in document.objects() {
            if let TypedObjectHandle::Geometry(TypedGeometryHandle::Mesh(handle)) =
                object.get_typed()
            {
                let mesh = mesh_dom(handle)?;
                if !mesh.indices.is_empty() {
                    meshes.push(mesh)
                }
            }
        }
        if meshes.is_empty() {
            return Err(invalid("FBX contains no renderable mesh geometry"));
        }
        Ok(Self {
            header: FbxHeader {
                version: version.to_le_bytes(),
                endian: "Little".into(),
                target_address_size: if version >= 7500 { 8 } else { 4 },
                alignment_exponent: 0,
                file_size: data.len() as u64,
                string_pool_offset: 0,
                string_pool_size: 0,
            },
            name: Some(name.into()),
            sections: vec![FbxSection {
                signature: *b"FBX ",
                offset: 0,
                name: Some(name.into()),
            }],
            materials: Vec::new(),
            render: BfresRenderGraph {
                bones: Vec::new(),
                matrix_to_bone: Vec::new(),
                meshes,
            },
            format: "FBX".into(),
        })
    }
    pub fn open(
        path: &Path,
    ) -> Option<(
        crate::file_format::BinTextFile::OpenedFile<'static>,
        crate::Open_and_Save::SendData,
    )> {
        // #[cfg(not(debug_assertions))]
        // {
        //     return None;
        // }

        let data = std::fs::read(path).ok()?;
        if !data.starts_with(b"Kaydara FBX Binary") {
            return None;
        }
        let mut opened = crate::file_format::BinTextFile::OpenedFile::default();
        opened.path = crate::Settings::Pathlib::new(path);
        opened.file_type = crate::Zstd::TotkFileType::Other;
        let mut send = crate::Open_and_Save::SendData::default();
        send.path = crate::Settings::Pathlib::new(path);
        send.file_label = format!("{} [FBX]", send.path.name);
        send.file_metadata = "[3D MODEL] [READ ONLY]".into();
        send.status_text = format!("Opened FBX {}", path.display());
        send.tab = "3D".into();
        send.read_only = true;
        Some((opened, send))
    }
}

fn mesh_dom(handle: fbxcel_dom::v7400::object::geometry::MeshHandle<'_>) -> io::Result<BfresMesh> {
    let name = handle.name().unwrap_or("Mesh").to_string();
    let polygons = handle
        .polygon_vertices()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    let control: Vec<[f32; 3]> = polygons
        .raw_control_points()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?
        .map(|p| [p.x as f32, p.y as f32, p.z as f32])
        .collect();
    let mut poly = Vec::new();
    let mut all = Vec::new();
    for &raw in polygons.raw_polygon_vertices() {
        let end = raw < 0;
        poly.push(if end { (!raw) as usize } else { raw as usize });
        if end {
            all.push(std::mem::take(&mut poly));
        }
    }
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut indices = Vec::new();
    for polygon in all {
        for i in 1..polygon.len().saturating_sub(1) {
            let p = [
                *control
                    .get(polygon[0])
                    .ok_or_else(|| invalid("control point out of range"))?,
                *control
                    .get(polygon[i])
                    .ok_or_else(|| invalid("control point out of range"))?,
                *control
                    .get(polygon[i + 1])
                    .ok_or_else(|| invalid("control point out of range"))?,
            ];
            let n = normal(p[0], p[1], p[2]);
            for point in p {
                positions.push(point);
                normals.push(n);
                indices.push(indices.len() as u32)
            }
        }
    }
    Ok(BfresMesh {
        name,
        material_index: 0,
        bone_index: 0,
        vertex_skin_count: 0,
        positions,
        normals,
        uv0: Vec::new(),
        uv_maps: Vec::new(),
        colors: Vec::new(),
        bone_indices: Vec::new(),
        bone_weights: Vec::new(),
        indices,
        skin_bones: Vec::new(),
    })
}
fn normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2])
        .sqrt()
        .max(f32::EPSILON);
    [n[0] / l, n[1] / l, n[2] / l]
}
fn invalid(v: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, v)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_ten_deterministic_random_samples() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tmp/fbx");
        let mut files: Vec<_> = std::fs::read_dir(root)
            .unwrap()
            .filter_map(Result::ok)
            .map(|v| v.path())
            .filter(|p| p.extension().and_then(|v| v.to_str()) == Some("fbx"))
            .collect();
        files.sort_by_key(|p| fxhash(p.to_string_lossy().as_bytes()));
        for path in files.into_iter().take(10) {
            let f = FbxFile::from_path(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert!(!f.render.meshes.is_empty(), "{}", path.display());
            assert!(
                f.render.meshes.iter().all(|m| !m.indices.is_empty()),
                "{}",
                path.display()
            );
        }
    }
    fn fxhash(v: &[u8]) -> u64 {
        v.iter().fold(0xcbf29ce484222325, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x100000001b3)
        })
    }
}
