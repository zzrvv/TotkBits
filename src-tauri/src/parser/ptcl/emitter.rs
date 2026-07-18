use super::animation::AnimKeyFrame;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct Emitter {
    pub const_color0: [f32; 4],
    pub const_color1: [f32; 4],
    pub color_anim0: Vec<AnimKeyFrame>,
    pub color_anim1: Vec<AnimKeyFrame>,
    pub alpha_anim0: Vec<AnimKeyFrame>,
    pub alpha_anim1: Vec<AnimKeyFrame>,
}

#[derive(Clone, Debug)]
pub(crate) struct EmitterLocation {
    pub section: usize,
    pub data: usize,
}
