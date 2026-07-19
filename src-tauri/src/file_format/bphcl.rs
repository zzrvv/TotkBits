use crate::parser::bphcl::BphclDocument;
use roead::aamp::{ParameterIO, ParameterList};
use serde::Serialize;
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
                path: format!("Cloth/{:03} {}.yaml", c.index, c.name),
                yaml: serde_yaml::to_string(c).map_err(io::Error::other)?,
                viewer_type: "Cloth".into(),
                read_only: true,
            })
        }
        for c in &self.document.collidables {
            out.push(BphclLeaf {
                path: format!("Collidables/{:03} {}.yaml", c.index, c.name),
                yaml: serde_yaml::to_string(c).map_err(io::Error::other)?,
                viewer_type: "Collidable".into(),
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

fn display_name(hash: u32) -> String {
    AAMP_TOTK_NAMES
        .get(&hash)
        .cloned()
        .unwrap_or_else(|| hash.to_string())
}

fn safe_aamp_yaml(pio: &ParameterIO) -> io::Result<String> {
    fn list(value: &ParameterList) -> serde_yaml::Value {
        let mut root = serde_yaml::Mapping::new();
        let mut lists = serde_yaml::Mapping::new();
        for (name, child) in value.lists.iter() {
            lists.insert(display_name(name.hash()).into(), list(child));
        }
        let mut objects = serde_yaml::Mapping::new();
        for (name, object) in value.objects.iter() {
            let mut params = serde_yaml::Mapping::new();
            for (parameter_name, parameter) in object.iter() {
                // Debug is exhaustive over Parameter and never invokes the
                // unsafe AAMP name-recovery table. Binary buffers remain
                // numeric arrays, never base64 strings.
                params.insert(
                    display_name(parameter_name.hash()).into(),
                    format!("{parameter:?}").into(),
                );
            }
            objects.insert(display_name(name.hash()).into(), params.into());
        }
        root.insert("lists".into(), lists.into());
        root.insert("objects".into(), objects.into());
        root.into()
    }
    let mut root = serde_yaml::Mapping::new();
    root.insert("version".into(), pio.version.into());
    root.insert("dataType".into(), pio.data_type.to_string().into());
    root.insert("paramRoot".into(), list(&pio.param_root));
    serde_yaml::to_string(&root).map_err(io::Error::other)
}
