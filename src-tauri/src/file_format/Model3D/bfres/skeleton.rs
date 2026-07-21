use super::{f32_at, i16_at, read_string, u16_at, u32_at, u64_at, BfresBone, BfresError, Endian};

pub fn parse_skeleton(
    data: &[u8],
    offset: usize,
    endian: Endian,
) -> Result<(Vec<BfresBone>, Vec<u16>), BfresError> {
    let bones_offset = u64_at(data, offset + 16, endian)? as usize;
    let palette_offset = u64_at(data, offset + 24, endian)? as usize;
    let count = u16_at(data, offset + 56, endian)? as usize;
    let palette_count =
        u16_at(data, offset + 58, endian)? as usize + u16_at(data, offset + 60, endian)? as usize;
    let skeleton_flags = u32_at(data, offset + 48, endian)?;
    let mut bones = Vec::with_capacity(count);
    for index in 0..count {
        let entry = bones_offset + index * 88;
        bones.push(BfresBone {
            name: read_string(data, u64_at(data, entry, endian)?)
                .unwrap_or_else(|| format!("Bone_{index}")),
            parent_index: i16_at(data, entry + 34, endian)?,
            smooth_matrix_index: i16_at(data, entry + 36, endian)?,
            rigid_matrix_index: i16_at(data, entry + 38, endian)?,
            // Rotation mode belongs to FSKL, not each Bone. BfresLibrary's
            // SkeletonFlagsRotation uses zero for EulerXYZ and one for Quaternion.
            rotation_mode: if skeleton_flags & 1 != 0 {
                "quaternion".into()
            } else {
                "euler_xyz".into()
            },
            scale: [
                f32_at(data, entry + 48, endian)?,
                f32_at(data, entry + 52, endian)?,
                f32_at(data, entry + 56, endian)?,
            ],
            rotation: [
                f32_at(data, entry + 60, endian)?,
                f32_at(data, entry + 64, endian)?,
                f32_at(data, entry + 68, endian)?,
                f32_at(data, entry + 72, endian)?,
            ],
            translation: [
                f32_at(data, entry + 76, endian)?,
                f32_at(data, entry + 80, endian)?,
                f32_at(data, entry + 84, endian)?,
            ],
        });
    }
    let palette = (0..palette_count)
        .map(|index| u16_at(data, palette_offset + index * 2, endian))
        .collect::<Result<Vec<_>, _>>()?;
    Ok((bones, palette))
}
