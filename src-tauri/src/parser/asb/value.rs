use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExbValue {
    Bool(bool),
    Integer(u32),
    Float(f32),
    String(String),
    Vec3f([f32; 3]),
}
