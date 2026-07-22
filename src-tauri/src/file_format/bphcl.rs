use crate::parser::bphcl::BphclDocument;
use roead::aamp::{Parameter, ParameterIO, ParameterList};
use serde::Serialize;
use serde_yaml::{
    value::{Tag, TaggedValue},
    Mapping, Value,
};
use std::{collections::HashMap, io, path::Path, sync::LazyLock};

static AAMP_TOTK_NAMES: LazyLock<HashMap<u32, String>> = LazyLock::new(|| {
    let source = include_str!("../../../ext_projects/bphcl/aamp_totk_hashes.py");
    let json = source
        .split_once('=')
        .map(|(_, value)| value.trim())
        .unwrap_or("{}");
    serde_json::from_str::<HashMap<String, String>>(json)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(hash, name)| hash.parse().ok().map(|hash| (hash, name)))
        .collect()
});

#[derive(Clone, Debug, Serialize)]
pub struct BphclLeaf {
    pub path: String,
    pub yaml: String,
    pub viewer_type: String,
    pub read_only: bool,
}
pub struct BphclFile {
    pub source_path: Option<String>,
    pub document: BphclDocument,
}
impl BphclFile {
    pub fn send_data(
        &self,
        path: &Path,
        status_text: String,
    ) -> io::Result<crate::Open_and_Save::SendData> {
        let root_name = path
            .file_name()
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "BPHCL path has no file name")
            })?
            .to_string_lossy();
        let mut data = crate::Open_and_Save::SendData::default();
        data.path = crate::Settings::Pathlib::new(path);
        data.tab = "SARC".into();
        data.read_only = false;
        data.sarc_paths.paths = self
            .leaves()?
            .into_iter()
            .map(|leaf| format!("{root_name}/{}", leaf.path))
            .collect();
        data.sarc_paths.read_only = true;
        data.get_file_label(crate::Zstd::TotkFileType::Bphcl, None);
        data.status_text = status_text;
        Ok(data)
    }

    pub fn open(
        path: &Path,
    ) -> Option<(
        crate::file_format::BinTextFile::OpenedFile<'static>,
        crate::Open_and_Save::SendData,
    )> {
        let bytes = std::fs::read(path).ok()?;
        let file = Self::from_binary(&bytes, Some(path)).ok()?;
        let mut opened = crate::file_format::BinTextFile::OpenedFile::default();
        opened.file_type = crate::Zstd::TotkFileType::Bphcl;
        opened.path = crate::Settings::Pathlib::new(path);
        opened.bphcl = Some(file);
        let data = opened
            .bphcl
            .as_ref()?
            .send_data(path, "Opened BPHCL structure".into())
            .ok()?;
        Some((opened, data))
    }

    pub fn leaf(&self, path: &str) -> io::Result<BphclLeaf> {
        let path = path.split_once('/').map(|(_, leaf)| leaf).unwrap_or(path);
        self.leaves()?
            .into_iter()
            .find(|leaf| leaf.path == path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "BPHCL leaf not found"))
    }
    pub fn from_binary(data: &[u8], path: Option<&Path>) -> io::Result<Self> {
        Ok(Self {
            source_path: path.map(|p| p.to_string_lossy().into_owned()),
            document: BphclDocument::parse(data)?,
        })
    }
    pub fn raw_binary(&self) -> Vec<u8> {
        self.document.to_bytes()
    }
    pub fn leaves(&self) -> io::Result<Vec<BphclLeaf>> {
        let mut out = vec![];
        for c in &self.document.cloth {
            out.push(BphclLeaf {
                path: format!("Cloth/{:03} {}.bin", c.index, c.name),
                yaml: serde_yaml::to_string(c).map_err(io::Error::other)?,
                viewer_type: "Cloth".into(),
                read_only: true,
            })
        }
        for c in &self.document.collidables {
            out.push(BphclLeaf {
                path: format!("Collidables/{:03} {}.bin", c.index, c.name),
                yaml: serde_yaml::to_string(c).map_err(io::Error::other)?,
                viewer_type: "Collidable".into(),
                read_only: true,
            })
        }
        for skeleton in &self.document.skeletons {
            out.push(BphclLeaf {
                path: format!("Skeletons/{:03} {}.bin", skeleton.index, skeleton.name),
                yaml: serde_yaml::to_string(skeleton).map_err(io::Error::other)?,
                viewer_type: "Skeleton".into(),
                read_only: true,
            })
        }
        if let Some(a) = &self.document.aamp {
            let pio = ParameterIO::from_binary(&a.raw).map_err(io::Error::other)?;
            out.push(BphclLeaf {
                path: "Section.aamp".into(),
                yaml: safe_aamp_yaml(&pio)?,
                viewer_type: "AAMP".into(),
                read_only: true,
            })
        }
        Ok(out)
    }
}

fn yaml_name(hash: u32) -> Value {
    match AAMP_TOTK_NAMES.get(&hash) {
        Some(name) => Value::String(name.clone()),
        None => Value::Number(hash.into()),
    }
}

fn tagged(tag: &str, value: Value) -> Value {
    Value::Tagged(Box::new(TaggedValue {
        tag: Tag::new(tag),
        value,
    }))
}

fn float(value: f32) -> Value {
    Value::from(value as f64)
}

fn sequence(values: impl IntoIterator<Item = Value>) -> Value {
    Value::Sequence(values.into_iter().collect())
}

fn curves<const N: usize>(values: &[roead::types::Curve; N]) -> Value {
    tagged(
        "!curve",
        sequence(values.iter().flat_map(|curve| {
            std::iter::once(Value::from(curve.a))
                .chain(std::iter::once(Value::from(curve.b)))
                .chain(curve.floats.iter().copied().map(Value::from))
        })),
    )
}

fn parameter_yaml(parameter: &Parameter) -> Value {
    match parameter {
        Parameter::Bool(value) => Value::Bool(*value),
        Parameter::F32(value) => float(*value),
        Parameter::I32(value) => Value::Number((*value).into()),
        Parameter::Vec2(value) => tagged("!vec2", sequence([float(value.x), float(value.y)])),
        Parameter::Vec3(value) => tagged(
            "!vec3",
            sequence([float(value.x), float(value.y), float(value.z)]),
        ),
        Parameter::Vec4(value) => tagged(
            "!vec4",
            sequence([
                float(value.x),
                float(value.y),
                float(value.z),
                float(value.t),
            ]),
        ),
        Parameter::Color(value) => tagged(
            "!color",
            sequence([
                float(value.r),
                float(value.g),
                float(value.b),
                float(value.a),
            ]),
        ),
        Parameter::String32(value) => tagged("!str32", Value::String(value.to_string())),
        Parameter::String64(value) => tagged("!str64", Value::String(value.to_string())),
        Parameter::Curve1(value) => curves(value),
        Parameter::Curve2(value) => curves(value),
        Parameter::Curve3(value) => curves(value),
        Parameter::Curve4(value) => curves(value),
        Parameter::BufferInt(values) => tagged(
            "!buffer_int",
            sequence(values.iter().copied().map(Value::from)),
        ),
        Parameter::BufferF32(values) => {
            tagged("!buffer_f32", sequence(values.iter().copied().map(float)))
        }
        Parameter::String256(value) => tagged("!str256", Value::String(value.to_string())),
        Parameter::Quat(value) => tagged(
            "!quat",
            sequence([
                float(value.a),
                float(value.b),
                float(value.c),
                float(value.d),
            ]),
        ),
        Parameter::U32(value) => tagged("!u", Value::Number((*value).into())),
        Parameter::BufferU32(values) => tagged(
            "!buffer_u32",
            sequence(values.iter().copied().map(Value::from)),
        ),
        Parameter::BufferBinary(values) => tagged(
            "!buffer_binary",
            sequence(values.iter().copied().map(Value::from)),
        ),
        Parameter::StringRef(value) => Value::String(value.to_string()),
    }
}

pub(crate) fn safe_aamp_yaml(pio: &ParameterIO) -> io::Result<String> {
    fn list(value: &ParameterList) -> Value {
        let mut root = Mapping::new();
        let mut lists = Mapping::new();
        for (name, child) in value.lists.iter() {
            lists.insert(yaml_name(name.hash()), list(child));
        }
        let mut objects = Mapping::new();
        for (name, object) in value.objects.iter() {
            let mut params = Mapping::new();
            for (parameter_name, parameter) in object.iter() {
                params.insert(yaml_name(parameter_name.hash()), parameter_yaml(parameter));
            }
            objects.insert(yaml_name(name.hash()), tagged("!obj", params.into()));
        }
        root.insert("lists".into(), lists.into());
        root.insert("objects".into(), objects.into());
        tagged("!list", root.into())
    }
    let mut root = Mapping::new();
    root.insert("version".into(), pio.version.into());
    root.insert("type".into(), pio.data_type.to_string().into());
    root.insert("param_root".into(), list(&pio.param_root));
    serde_yaml::to_string(&tagged("!io", root.into())).map_err(io::Error::other)
}

pub(crate) fn aamp_from_yaml(yaml: &str) -> io::Result<ParameterIO> {
    ParameterIO::from_text(yaml).map_err(io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::yaml_name;
    use serde_yaml::Value;

    #[test]
    fn unknown_aamp_name_falls_back_to_numeric_hash() {
        let unknown = (0..=u32::MAX)
            .find(|hash| !super::AAMP_TOTK_NAMES.contains_key(hash))
            .expect("the AAMP name table cannot contain every u32 hash");
        assert_eq!(yaml_name(unknown), Value::Number(unknown.into()));
    }
}
