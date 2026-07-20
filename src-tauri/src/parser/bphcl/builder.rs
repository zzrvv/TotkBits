use super::{BphclDocument, Item, Patch, ReferenceArray, Section};
use std::io::{self, ErrorKind};

/// Mutable TAG0 payload state used by the native merge stages.
pub struct BphclBuilder<'a> {
    document: &'a BphclDocument,
    pub data: Vec<u8>,
    pub items: Vec<Item>,
    pub patches: Vec<Patch>,
    replacement_aamp: Option<Vec<u8>>,
    replacement_type: Option<Vec<u8>>,
}

impl<'a> BphclBuilder<'a> {
    pub fn new(document: &'a BphclDocument) -> io::Result<Self> {
        let section = document
            .tag
            .find("DATA")
            .ok_or_else(|| invalid("BPHCL has no DATA section"))?;
        Ok(Self {
            document,
            data: document.raw[section.payload_offset..section.payload_end()].to_vec(),
            items: document.items.clone(),
            patches: document.patches.clone(),
            replacement_aamp: None,
            replacement_type: None,
        })
    }

    pub fn replace_reference_array(
        &mut self,
        array: &ReferenceArray,
        entries: impl IntoIterator<Item = usize>,
    ) -> io::Result<usize> {
        let entries: Vec<usize> = entries.into_iter().collect();
        replace_array_data(
            &mut self.data,
            &mut self.items,
            &mut self.patches,
            array,
            &entries,
        )
    }

    pub fn append_reference_array(
        &mut self,
        array: &ReferenceArray,
        additions: impl IntoIterator<Item = usize>,
    ) -> io::Result<usize> {
        let mut entries = if array.storage_item_index == usize::MAX {
            Vec::new()
        } else {
            self.document.reference_item_indices(array.field_offset)?
        };
        entries.extend(additions);
        self.replace_reference_array(array, entries)
    }

    pub fn build(self) -> io::Result<Vec<u8>> {
        rebuild(
            self.document,
            &self.data,
            &self.items,
            &self.patches,
            self.replacement_aamp.as_deref(),
            self.replacement_type.as_deref(),
        )
    }

    pub fn replace_aamp(&mut self, bytes: Vec<u8>) {
        self.replacement_aamp = Some(bytes);
    }
    pub fn replace_type(&mut self, bytes: Vec<u8>) {
        self.replacement_type = Some(bytes);
    }
}

fn replace_array_data(
    data: &mut Vec<u8>,
    items: &mut Vec<Item>,
    patches: &mut Vec<Patch>,
    array: &ReferenceArray,
    entries: &[usize],
) -> io::Result<usize> {
    while data.len() % 8 != 0 {
        data.push(0);
    }
    let data_offset = u32::try_from(data.len()).map_err(|_| invalid("BPHCL DATA exceeds u32"))?;
    for entry in entries {
        let entry = u32::try_from(*entry).map_err(|_| invalid("BPHCL ITEM index exceeds u32"))?;
        data.extend_from_slice(&entry.to_le_bytes());
        data.extend_from_slice(&0u32.to_le_bytes());
    }

    let storage_index = items.len();
    let storage_index_u32 =
        u32::try_from(storage_index).map_err(|_| invalid("BPHCL ITEM index exceeds u32"))?;
    let entry_count =
        u32::try_from(entries.len()).map_err(|_| invalid("BPHCL reference array is too large"))?;
    let mut storage = array.storage_item.clone();
    storage.data_offset = data_offset;
    storage.count = entry_count;
    items.push(storage);

    write_u32(data, array.field_offset, storage_index_u32)?;
    write_u32(data, checked_add(array.field_offset, 8)?, entry_count)?;
    let capacity_offset = checked_add(array.field_offset, 12)?;
    let capacity = read_u32(data, capacity_offset)?;
    write_u32(
        data,
        capacity_offset,
        (capacity & 0xc000_0000) | entry_count,
    )?;

    for index in 0..entries.len() {
        let offset = data_offset
            .checked_add(
                (index as u32)
                    .checked_mul(8)
                    .ok_or_else(|| invalid("patch overflow"))?,
            )
            .ok_or_else(|| invalid("patch overflow"))?;
        add_patch(patches, array.entry_patch_type_index, offset);
    }
    Ok(storage_index)
}

fn checked_add(value: u32, addition: u32) -> io::Result<u32> {
    value
        .checked_add(addition)
        .ok_or_else(|| invalid("DATA offset overflow"))
}

fn rebuild(
    document: &BphclDocument,
    data: &[u8],
    items: &[Item],
    patches: &[Patch],
    replacement_aamp: Option<&[u8]>,
    replacement_type: Option<&[u8]>,
) -> io::Result<Vec<u8>> {
    let data_section = document
        .tag
        .find("DATA")
        .ok_or_else(|| invalid("BPHCL has no DATA section"))?;
    let index = document
        .tag
        .find("INDX")
        .ok_or_else(|| invalid("BPHCL has no INDX section"))?;
    let item_section = index
        .find("ITEM")
        .ok_or_else(|| invalid("BPHCL has no ITEM section"))?;
    let patch_section = index
        .find("PTCH")
        .ok_or_else(|| invalid("BPHCL has no PTCH section"))?;

    let rebuilt_data = build_section("DATA", data_section.kind, data)?;
    let rebuilt_items = build_items(item_section.kind, items)?;
    let rebuilt_patches = build_patches(document, patch_section, patches)?;
    let mut index_payload = Vec::new();
    for child in &index.children {
        match child.signature.as_str() {
            "ITEM" => index_payload.extend_from_slice(&rebuilt_items),
            "PTCH" => index_payload.extend_from_slice(&rebuilt_patches),
            _ => copy_section(&mut index_payload, &document.raw, child)?,
        }
    }
    let rebuilt_index = build_section("INDX", index.kind, &index_payload)?;

    let mut tag_payload = Vec::new();
    for child in &document.tag.children {
        if child.offset == data_section.offset {
            tag_payload.extend_from_slice(&rebuilt_data);
        } else if child.offset == index.offset {
            tag_payload.extend_from_slice(&rebuilt_index);
        } else if child.signature == "TYPE" && replacement_type.is_some() {
            tag_payload.extend_from_slice(replacement_type.unwrap());
        } else {
            copy_section(&mut tag_payload, &document.raw, child)?;
        }
    }
    let rebuilt_tag = build_section("TAG0", document.tag.kind, &tag_payload)?;
    let old_tag_end = document
        .tag
        .offset
        .checked_add(document.tag.size)
        .ok_or_else(|| invalid("TAG0 overflow"))?;
    let delta =
        i64::try_from(rebuilt_tag.len()).unwrap() - i64::try_from(document.tag.size).unwrap();
    let mut output = Vec::with_capacity(
        (document.raw.len() as i64 + delta)
            .try_into()
            .map_err(|_| invalid("BPHCL size overflow"))?,
    );
    output.extend_from_slice(&document.raw[..document.tag.offset]);
    output.extend_from_slice(&rebuilt_tag);
    output.extend_from_slice(&document.raw[old_tag_end..]);

    write_adjusted_u32(&mut output, 24, document.header.tag_size, delta)?;
    adjust_offset_after(&mut output, 16, old_tag_end, delta)?;
    adjust_offset_after(&mut output, 20, old_tag_end, delta)?;
    if let Some(aamp) = replacement_aamp {
        let offset = read_header_u32(&output, 16)? as usize;
        let old_size = document.header.parameter_size as usize;
        let end = offset
            .checked_add(old_size)
            .ok_or_else(|| invalid("AAMP range overflow"))?;
        if end > output.len() {
            return Err(invalid("AAMP range exceeds rebuilt BPHCL"));
        }
        let delta = i64::try_from(aamp.len()).unwrap() - i64::try_from(old_size).unwrap();
        output.splice(offset..end, aamp.iter().copied());
        write_u32_at(
            &mut output,
            28,
            u32::try_from(aamp.len()).map_err(|_| invalid("AAMP exceeds u32"))?,
        )?;
        let file_end = read_header_u32(&output, 20)?;
        if file_end as usize >= end {
            write_adjusted_u32(&mut output, 20, file_end, delta)?;
        }
    }
    Ok(output)
}

fn read_header_u32(data: &[u8], offset: usize) -> io::Result<u32> {
    Ok(u32::from_le_bytes(
        data.get(offset..offset + 4)
            .ok_or_else(|| invalid("header read exceeds file"))?
            .try_into()
            .unwrap(),
    ))
}
fn write_u32_at(data: &mut [u8], offset: usize, value: u32) -> io::Result<()> {
    data.get_mut(offset..offset + 4)
        .ok_or_else(|| invalid("header write exceeds file"))?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn build_items(kind: u8, items: &[Item]) -> io::Result<Vec<u8>> {
    let mut payload = Vec::with_capacity(
        items
            .len()
            .checked_mul(12)
            .ok_or_else(|| invalid("ITEM size overflow"))?,
    );
    for item in items {
        payload.extend_from_slice(&item.flags.to_le_bytes());
        payload.extend_from_slice(&item.data_offset.to_le_bytes());
        payload.extend_from_slice(&item.count.to_le_bytes());
    }
    build_section("ITEM", kind, &payload)
}

fn build_patches(
    document: &BphclDocument,
    original: &Section,
    patches: &[Patch],
) -> io::Result<Vec<u8>> {
    let mut payload = Vec::new();
    for patch in patches {
        payload.extend_from_slice(&patch.type_index.to_le_bytes());
        payload.extend_from_slice(
            &u32::try_from(patch.offsets.len())
                .map_err(|_| invalid("PTCH count exceeds u32"))?
                .to_le_bytes(),
        );
        for offset in &patch.offsets {
            payload.extend_from_slice(&offset.to_le_bytes());
        }
    }
    let (terminator, tail) = external_patch_tail(&document.raw, original)?;
    if terminator {
        payload.extend_from_slice(&0u32.to_le_bytes());
    }
    payload.extend_from_slice(tail);
    build_section("PTCH", original.kind, &payload)
}

fn external_patch_tail<'a>(bytes: &'a [u8], section: &Section) -> io::Result<(bool, &'a [u8])> {
    let mut cursor = section.payload_offset;
    let end = section.payload_end();
    while cursor + 4 <= end {
        let type_index = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap());
        cursor += 4;
        if type_index == 0 {
            return Ok((true, &bytes[cursor..end]));
        }
        if cursor + 4 > end {
            return Err(invalid("truncated PTCH count"));
        }
        let count = u32::from_le_bytes(bytes[cursor..cursor + 4].try_into().unwrap()) as usize;
        cursor += 4;
        cursor = cursor
            .checked_add(
                count
                    .checked_mul(4)
                    .ok_or_else(|| invalid("PTCH size overflow"))?,
            )
            .ok_or_else(|| invalid("PTCH size overflow"))?;
        if cursor > end {
            return Err(invalid("PTCH entry exceeds section"));
        }
    }
    if cursor == end {
        Ok((false, &[]))
    } else {
        Err(invalid("truncated PTCH group"))
    }
}

fn build_section(signature: &str, kind: u8, payload: &[u8]) -> io::Result<Vec<u8>> {
    if signature.len() != 4 {
        return Err(invalid("section signature must be four bytes"));
    }
    let size = payload
        .len()
        .checked_add(8)
        .ok_or_else(|| invalid("section size overflow"))?;
    if size > 0x3fff_ffff {
        return Err(invalid("section exceeds 30-bit size limit"));
    }
    let mut result = Vec::with_capacity(size);
    result.extend_from_slice(&(((kind as u32) << 30) | size as u32).to_be_bytes());
    result.extend_from_slice(signature.as_bytes());
    result.extend_from_slice(payload);
    Ok(result)
}

fn copy_section(output: &mut Vec<u8>, source: &[u8], section: &Section) -> io::Result<()> {
    output.extend_from_slice(
        source
            .get(section.offset..section.payload_end())
            .ok_or_else(|| invalid("section exceeds input"))?,
    );
    Ok(())
}

fn add_patch(patches: &mut Vec<Patch>, type_index: u32, offset: u32) {
    if let Some(group) = patches
        .iter_mut()
        .find(|patch| patch.type_index == type_index)
    {
        group.offsets.push(offset);
    } else {
        patches.push(Patch {
            type_index,
            offsets: vec![offset],
        });
    }
}

fn read_u32(data: &[u8], offset: u32) -> io::Result<u32> {
    let offset = offset as usize;
    Ok(u32::from_le_bytes(
        data.get(offset..offset + 4)
            .ok_or_else(|| invalid("DATA read exceeds section"))?
            .try_into()
            .unwrap(),
    ))
}

fn write_u32(data: &mut [u8], offset: u32, value: u32) -> io::Result<()> {
    let offset = offset as usize;
    data.get_mut(offset..offset + 4)
        .ok_or_else(|| invalid("DATA write exceeds section"))?
        .copy_from_slice(&value.to_le_bytes());
    Ok(())
}

fn write_adjusted_u32(data: &mut [u8], offset: usize, value: u32, delta: i64) -> io::Result<()> {
    let adjusted =
        u32::try_from(i64::from(value) + delta).map_err(|_| invalid("header value overflow"))?;
    data.get_mut(offset..offset + 4)
        .ok_or_else(|| invalid("header write exceeds file"))?
        .copy_from_slice(&adjusted.to_le_bytes());
    Ok(())
}

fn adjust_offset_after(
    data: &mut [u8],
    field: usize,
    old_tag_end: usize,
    delta: i64,
) -> io::Result<()> {
    let value = u32::from_le_bytes(
        data.get(field..field + 4)
            .ok_or_else(|| invalid("header read exceeds file"))?
            .try_into()
            .unwrap(),
    );
    if value as usize >= old_tag_end {
        write_adjusted_u32(data, field, value, delta)?;
    }
    Ok(())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_array_replacement_allocates_storage_and_patches_entries() {
        let mut data = vec![0; 20];
        data[12..16].copy_from_slice(&0x8000_0001u32.to_le_bytes());
        let old = Item {
            flags: 0x1234,
            type_index: 0x1234,
            data_offset: 16,
            count: 1,
        };
        let array = ReferenceArray {
            field_offset: 0,
            storage_item_index: 0,
            storage_item: old.clone(),
            entry_patch_type_index: 7,
        };
        let mut items = vec![old];
        let mut patches = vec![Patch {
            type_index: 3,
            offsets: vec![0],
        }];
        let index =
            replace_array_data(&mut data, &mut items, &mut patches, &array, &[4, 9]).unwrap();

        assert_eq!(index, 1);
        assert_eq!(u32::from_le_bytes(data[0..4].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(data[8..12].try_into().unwrap()), 2);
        assert_eq!(
            u32::from_le_bytes(data[12..16].try_into().unwrap()),
            0x8000_0002
        );
        assert_eq!((items[1].data_offset, items[1].count), (24, 2));
        assert_eq!(&data[24..32], &[4, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(&data[32..40], &[9, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(patches[1].offsets, vec![24, 32]);
    }

    #[test]
    fn section_writer_encodes_kind_and_size_big_endian() {
        let section = build_section("DATA", 2, &[1, 2, 3]).unwrap();
        assert_eq!(&section[..4], &0x8000_000bu32.to_be_bytes());
        assert_eq!(&section[4..8], b"DATA");
        assert_eq!(&section[8..], &[1, 2, 3]);
    }
}
