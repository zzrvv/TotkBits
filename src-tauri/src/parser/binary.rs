use std::io::{self, ErrorKind};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Endian {
    Little,
    Big,
}

pub struct BinaryReader<'a> {
    data: &'a [u8],
    position: usize,
    endian: Endian,
}

impl<'a> BinaryReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            position: 0,
            endian: Endian::Little,
        }
    }

    pub fn with_endian(data: &'a [u8], endian: Endian) -> Self {
        Self {
            data,
            position: 0,
            endian,
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }
    pub fn position(&self) -> usize {
        self.position
    }
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.position)
    }

    pub fn seek(&mut self, position: usize) -> io::Result<()> {
        if position > self.data.len() {
            return Err(self.error("seek exceeds input"));
        }
        self.position = position;
        Ok(())
    }

    pub fn skip(&mut self, count: usize) -> io::Result<()> {
        self.seek(
            self.position
                .checked_add(count)
                .ok_or_else(|| self.error("offset overflow"))?,
        )
    }

    pub fn align(&mut self, alignment: usize) -> io::Result<()> {
        if alignment == 0 {
            return Err(self.error("alignment cannot be zero"));
        }
        let aligned = self
            .position
            .checked_add(alignment - 1)
            .ok_or_else(|| self.error("alignment overflow"))?
            / alignment
            * alignment;
        self.seek(aligned)
    }

    pub fn read_bytes(&mut self, count: usize) -> io::Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| self.error("read overflow"))?;
        if end > self.data.len() {
            return Err(self.error("unexpected end of input"));
        }
        let value = &self.data[self.position..end];
        self.position = end;
        Ok(value)
    }

    pub fn slice(&self, start: usize, end: usize) -> io::Result<&'a [u8]> {
        self.data
            .get(start..end)
            .ok_or_else(|| self.error("slice exceeds input"))
    }

    pub fn read_u8(&mut self) -> io::Result<u8> {
        Ok(self.read_bytes(1)?[0])
    }
    pub fn read_i8(&mut self) -> io::Result<i8> {
        Ok(self.read_u8()? as i8)
    }
    pub fn read_u16(&mut self) -> io::Result<u16> {
        let mut b = [0; 2];
        b.copy_from_slice(self.read_bytes(2)?);
        Ok(match self.endian {
            Endian::Little => u16::from_le_bytes(b),
            Endian::Big => u16::from_be_bytes(b),
        })
    }
    pub fn read_i16(&mut self) -> io::Result<i16> {
        Ok(self.read_u16()? as i16)
    }
    pub fn read_u32(&mut self) -> io::Result<u32> {
        let mut b = [0; 4];
        b.copy_from_slice(self.read_bytes(4)?);
        Ok(match self.endian {
            Endian::Little => u32::from_le_bytes(b),
            Endian::Big => u32::from_be_bytes(b),
        })
    }
    pub fn read_i32(&mut self) -> io::Result<i32> {
        Ok(self.read_u32()? as i32)
    }
    pub fn read_u64(&mut self) -> io::Result<u64> {
        let mut b = [0; 8];
        b.copy_from_slice(self.read_bytes(8)?);
        Ok(match self.endian {
            Endian::Little => u64::from_le_bytes(b),
            Endian::Big => u64::from_be_bytes(b),
        })
    }
    pub fn read_i64(&mut self) -> io::Result<i64> {
        Ok(self.read_u64()? as i64)
    }
    pub fn read_f32(&mut self) -> io::Result<f32> {
        Ok(f32::from_bits(self.read_u32()?))
    }
    pub fn read_f64(&mut self) -> io::Result<f64> {
        Ok(f64::from_bits(self.read_u64()?))
    }

    pub fn read_c_string_at(&self, offset: usize) -> io::Result<String> {
        // An offset at the end of an empty pool represents an empty string in
        // ASB and several related Nintendo formats.
        if offset == self.data.len() {
            return Ok(String::new());
        }
        let tail = self.data.get(offset..).ok_or_else(|| {
            self.error(&format!(
                "string offset {offset:#x} exceeds input length {:#x}",
                self.data.len()
            ))
        })?;
        let end = tail
            .iter()
            .position(|byte| *byte == 0)
            .ok_or_else(|| self.error("unterminated string"))?;
        String::from_utf8(tail[..end].to_vec())
            .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))
    }

    fn error(&self, message: &str) -> io::Error {
        io::Error::new(
            ErrorKind::UnexpectedEof,
            format!("{message} at {:#x}", self.position),
        )
    }
}

#[derive(Default)]
pub struct BinaryWriter {
    data: Vec<u8>,
    position: usize,
    endian: Endian,
}

impl Default for Endian {
    fn default() -> Self {
        Self::Little
    }
}

impl BinaryWriter {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_endian(endian: Endian) -> Self {
        Self {
            data: Vec::new(),
            position: 0,
            endian,
        }
    }
    pub fn position(&self) -> usize {
        self.position
    }
    pub fn into_inner(self) -> Vec<u8> {
        self.data
    }
    pub fn seek(&mut self, position: usize) {
        self.position = position;
        if position > self.data.len() {
            self.data.resize(position, 0);
        }
    }
    pub fn align(&mut self, alignment: usize) -> io::Result<()> {
        if alignment == 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "alignment cannot be zero",
            ));
        }
        let position = (self.position + alignment - 1) / alignment * alignment;
        self.seek(position);
        Ok(())
    }
    pub fn write_bytes(&mut self, value: &[u8]) {
        let end = self.position + value.len();
        if end > self.data.len() {
            self.data.resize(end, 0);
        }
        self.data[self.position..end].copy_from_slice(value);
        self.position = end;
    }
    pub fn write_u8(&mut self, v: u8) {
        self.write_bytes(&[v]);
    }
    pub fn write_i8(&mut self, v: i8) {
        self.write_u8(v as u8);
    }
    pub fn write_u16(&mut self, v: u16) {
        self.write_bytes(&match self.endian {
            Endian::Little => v.to_le_bytes(),
            Endian::Big => v.to_be_bytes(),
        });
    }
    pub fn write_i16(&mut self, v: i16) {
        self.write_u16(v as u16);
    }
    pub fn write_u32(&mut self, v: u32) {
        self.write_bytes(&match self.endian {
            Endian::Little => v.to_le_bytes(),
            Endian::Big => v.to_be_bytes(),
        });
    }
    pub fn write_i32(&mut self, v: i32) {
        self.write_u32(v as u32);
    }
    pub fn write_u64(&mut self, v: u64) {
        self.write_bytes(&match self.endian {
            Endian::Little => v.to_le_bytes(),
            Endian::Big => v.to_be_bytes(),
        });
    }
    pub fn write_i64(&mut self, v: i64) {
        self.write_u64(v as u64);
    }
    pub fn write_f32(&mut self, v: f32) {
        self.write_u32(v.to_bits());
    }
    pub fn write_f64(&mut self, v: f64) {
        self.write_u64(v.to_bits());
    }
    pub fn write_c_string(&mut self, value: &str) {
        self.write_bytes(value.as_bytes());
        self.write_u8(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_writer_round_trip_primitives_and_seek() {
        let mut writer = BinaryWriter::new();
        writer.write_u8(0x7f);
        writer.write_i16(-1234);
        writer.write_u32(0xdead_beef);
        writer.write_f32(12.5);
        writer.write_c_string("TotkBits");
        writer.seek(1);
        writer.write_i16(-1234);
        let bytes = writer.into_inner();

        let mut reader = BinaryReader::new(&bytes);
        assert_eq!(reader.read_u8().unwrap(), 0x7f);
        assert_eq!(reader.read_i16().unwrap(), -1234);
        assert_eq!(reader.read_u32().unwrap(), 0xdead_beef);
        assert_eq!(reader.read_f32().unwrap(), 12.5);
        assert_eq!(
            reader.read_c_string_at(reader.position()).unwrap(),
            "TotkBits"
        );
    }

    #[test]
    fn big_endian_is_supported() {
        let mut writer = BinaryWriter::with_endian(Endian::Big);
        writer.write_u32(0x0102_0304);
        let bytes = writer.into_inner();
        assert_eq!(bytes, [1, 2, 3, 4]);
        assert_eq!(
            BinaryReader::with_endian(&bytes, Endian::Big)
                .read_u32()
                .unwrap(),
            0x0102_0304
        );
    }

    #[test]
    fn string_at_end_of_input_is_empty() {
        let reader = BinaryReader::new(&[]);
        assert_eq!(reader.read_c_string_at(0).unwrap(), "");
    }
}
