use serde::Deserialize;
use serde_yaml::Value;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
struct Root {
    msbt: Config,
}

#[derive(Debug, Deserialize)]
struct Config {
    tags: Vec<TagConfig>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TagConfig {
    pub name: String,
    pub group: u16,
    #[serde(rename = "type")]
    pub kind: u16,
    #[serde(default)]
    pub arguments: Vec<TagArgument>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TagArgument {
    pub name: String,
    #[serde(rename = "dataType")]
    pub data_type: String,
    #[serde(default, rename = "valueMap")]
    pub value_map: HashMap<Value, Value>,
}

impl TagArgument {
    pub fn mapped_name(&self, value: i64) -> Option<String> {
        self.value_map.iter().find_map(|(key, mapped)| {
            let key = key.as_i64().or_else(|| key.as_u64().map(|x| x as i64))?;
            (key == value).then(|| value_text(mapped))
        })
    }

    pub fn mapped_value(&self, name: &str) -> Option<i64> {
        self.value_map.iter().find_map(|(key, mapped)| {
            (value_text(mapped) == name).then(|| {
                key.as_i64()
                    .or_else(|| key.as_u64().map(|x| x as i64))
                    .unwrap_or_default()
            })
        })
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Bool(value) => value.to_string().to_lowercase(),
        Value::Number(value) => value.to_string(),
        _ => String::new(),
    }
}

fn tags() -> &'static [TagConfig] {
    static TAGS: OnceLock<Vec<TagConfig>> = OnceLock::new();
    TAGS.get_or_init(|| {
        serde_yaml::from_str::<Root>(include_str!("totk_tags.gcf"))
            .map(|root| root.msbt.tags)
            .unwrap_or_default()
    })
}

pub fn by_id(group: u16, kind: u16) -> Option<&'static TagConfig> {
    tags()
        .iter()
        .find(|tag| tag.group == group && tag.kind == kind)
}

pub fn by_name(name: &str) -> Option<&'static TagConfig> {
    tags()
        .iter()
        .find(|tag| tag.name.eq_ignore_ascii_case(name))
}
