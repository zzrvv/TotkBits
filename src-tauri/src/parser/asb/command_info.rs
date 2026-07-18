use super::{data_type::DataType, instruction::Instruction};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandInfo {
    #[serde(rename = "Base Index Pre-Command Entry")]
    pub base_index_pre_command_entry: i32,
    #[serde(
        rename = "Pre-Entry Static Memory Usage",
        skip_serializing_if = "Option::is_none"
    )]
    pub pre_entry_static_memory_usage: Option<u32>,
    #[serde(rename = "Output Data Type")]
    pub output_data_type: DataType,
    #[serde(rename = "Input Data Type")]
    pub input_data_type: DataType,
    #[serde(rename = "Instructions")]
    pub instructions: Vec<Instruction>,
}
