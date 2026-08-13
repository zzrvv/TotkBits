use super::{data_type::DataType, opcode::Opcode, source::Source, value::ExbValue};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Instruction {
    #[serde(rename = "Type")]
    pub opcode: Opcode,
    #[serde(rename = "Data Type", skip_serializing_if = "Option::is_none")]
    pub data_type: Option<DataType>,
    #[serde(rename = "LHS Source", skip_serializing_if = "Option::is_none")]
    pub lhs_source: Option<Source>,
    #[serde(rename = "RHS Source", skip_serializing_if = "Option::is_none")]
    pub rhs_source: Option<Source>,
    #[serde(rename = "LHS Index/Value", skip_serializing_if = "Option::is_none")]
    pub lhs_index: Option<u16>,
    #[serde(rename = "RHS Index/Value", skip_serializing_if = "Option::is_none")]
    pub rhs_index: Option<u16>,
    #[serde(rename = "LHS Value", skip_serializing_if = "Option::is_none")]
    pub lhs_value: Option<ExbValue>,
    #[serde(rename = "RHS Value", skip_serializing_if = "Option::is_none")]
    pub rhs_value: Option<ExbValue>,
    #[serde(
        rename = "Static Memory Index",
        skip_serializing_if = "Option::is_none"
    )]
    pub static_memory_index: Option<u16>,
    #[serde(rename = "Signature", skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}
impl Instruction {
    pub fn terminator() -> Self {
        Self {
            opcode: Opcode::Terminator,
            data_type: None,
            lhs_source: None,
            rhs_source: None,
            lhs_index: None,
            rhs_index: None,
            lhs_value: None,
            rhs_value: None,
            static_memory_index: None,
            signature: None,
        }
    }
}
