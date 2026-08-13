pub fn crc32(value: impl AsRef<[u8]>) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &byte in value.as_ref() {
        crc ^= byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0xedb8_8320 } else { 0 };
        }
    }
    !crc
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ieee_vector() {
        assert_eq!(crc32("123456789"), 0xcbf4_3926)
    }
}
