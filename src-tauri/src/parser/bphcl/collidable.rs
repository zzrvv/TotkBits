use super::{CollidableShape, Vector4};
use serde::Serialize;
#[derive(Clone, Debug, Serialize)]
pub struct Collidable {
    pub index: usize,
    pub name: String,
    pub item_index: usize,
    pub class_name: String,
    pub translation: Vector4,
    pub axis_x: Vector4,
    pub axis_y: Vector4,
    pub axis_z: Vector4,
    pub enabled: bool,
    pub shape: CollidableShape,
}
