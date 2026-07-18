use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct AnimKeyFrame {
    pub value: [f32; 3],
    pub keyframe: f32,
}

pub(crate) const ANIMATION_SLOT_SIZE: usize = 0x80;
