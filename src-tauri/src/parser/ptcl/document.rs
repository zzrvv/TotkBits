use super::{
    animation::{AnimKeyFrame, ANIMATION_SLOT_SIZE},
    emitter::{Emitter, EmitterLocation},
    header::{relative, SectionHeader, END},
};
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io};

const MAGIC: &[u8; 8] = b"VFXB    ";
const MAX_SECTIONS: usize = 1_000_000;

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(transparent)]
pub struct PtclDocument(pub BTreeMap<String, BTreeMap<String, Emitter>>);

#[derive(Clone, Debug)]
pub struct Ptcl {
    pub document: PtclDocument,
    original_data: Vec<u8>,
    locations: BTreeMap<(String, String), EmitterLocation>,
}

impl Ptcl {
    pub fn parse(data: &[u8]) -> io::Result<Self> {
        let start = data
            .windows(MAGIC.len())
            .position(|window| window == MAGIC)
            .ok_or_else(|| invalid("PTCL VFXB magic was not found"))?;
        verify_file(data, start)?;
        let mut reader = BinaryReader::new(data);
        reader.seek(start + 0x16)?;
        let header_size = reader.read_u16()? as usize;
        if header_size < 0x18 {
            return Err(invalid("invalid PTCL header size"));
        }
        let first_block = start
            .checked_add(header_size)
            .ok_or_else(|| invalid("PTCL header offset overflow"))?;
        let esta = find_section(data, first_block, "ESTA")?;
        let first_set = relative(
            esta,
            SectionHeader::read(data, esta)?.section_offset,
            "ESET",
        )?;

        let mut document = PtclDocument::default();
        let mut locations = BTreeMap::new();
        walk_sections(data, first_set, "ESET", |set_offset, set_header| {
            let set_data = relative(set_offset, set_header.section_offset, "emitter set data")?;
            let set_name = read_string(data, set_data + 0x10, 0x60)?;
            let mut emitters = BTreeMap::new();
            if set_header.subsection_offset != END {
                let first_emitter = relative(set_offset, set_header.subsection_offset, "emitter")?;
                walk_sections(data, first_emitter, "EMTR", |emitter_offset, header| {
                    let emitter_data =
                        relative(emitter_offset, header.section_offset, "emitter data")?;
                    let name = read_string(data, emitter_data + 0x10, 0x60)?;
                    let emitter = read_emitter(data, emitter_data)?;
                    locations.insert(
                        (set_name.clone(), name.clone()),
                        EmitterLocation {
                            section: emitter_offset,
                            data: emitter_data,
                        },
                    );
                    emitters.insert(name, emitter);
                    Ok(())
                })?;
            }
            document.0.insert(set_name, emitters);
            Ok(())
        })?;
        Ok(Self {
            document,
            original_data: data.to_vec(),
            locations,
        })
    }

    pub fn to_yaml(&self) -> io::Result<String> {
        serde_yaml::to_string(&self.document).map_err(io::Error::other)
    }

    pub fn apply_yaml(&self, yaml: &str) -> io::Result<Vec<u8>> {
        let changes: PtclDocument = serde_yaml::from_str(yaml).map_err(io::Error::other)?;
        self.apply_document(&changes)
    }

    pub fn apply_document(&self, changes: &PtclDocument) -> io::Result<Vec<u8>> {
        let mut output = self.original_data.clone();
        for (set_name, emitters) in &changes.0 {
            for (emitter_name, emitter) in emitters {
                if self
                    .document
                    .0
                    .get(set_name)
                    .and_then(|original| original.get(emitter_name))
                    == Some(emitter)
                {
                    continue;
                }
                let Some(location) = self
                    .locations
                    .get(&(set_name.clone(), emitter_name.clone()))
                else {
                    continue;
                };
                let _section = location.section;
                write_emitter(&mut output, location.data, emitter)?;
            }
        }
        Ok(output)
    }
}

fn verify_file(data: &[u8], start: usize) -> io::Result<()> {
    let mut reader = BinaryReader::new(data);
    reader.seek(start)?;
    if reader.read_bytes(8)? != MAGIC {
        return Err(invalid("invalid PTCL magic"));
    }
    let _unknown = reader.read_u8()?;
    if reader.read_u8()? != 4 {
        return Err(invalid("unsupported PTCL graphics API version"));
    }
    if reader.read_u16()? != 0x33 {
        return Err(invalid("unsupported PTCL resource version"));
    }
    if reader.read_u16()? != 0xfeff {
        return Err(invalid("only little-endian PTCL is supported"));
    }
    Ok(())
}

fn find_section(data: &[u8], mut current: usize, signature: &str) -> io::Result<usize> {
    for _ in 0..MAX_SECTIONS {
        let header = SectionHeader::read(data, current)?;
        if header.signature == signature {
            return Ok(current);
        }
        if header.next_section_offset == END {
            break;
        }
        if header.next_section_offset == 0 {
            return Err(invalid("zero PTCL section link"));
        }
        current = relative(current, header.next_section_offset, "section")?;
    }
    Err(invalid(&format!("PTCL section {signature} was not found")))
}

fn walk_sections<F>(
    data: &[u8],
    mut current: usize,
    signature: &str,
    mut visit: F,
) -> io::Result<()>
where
    F: FnMut(usize, &SectionHeader) -> io::Result<()>,
{
    for _ in 0..MAX_SECTIONS {
        let header = SectionHeader::read(data, current)?;
        if header.signature != signature {
            return Err(invalid(&format!(
                "expected {signature}, found {}",
                header.signature
            )));
        }
        visit(current, &header)?;
        if header.next_section_offset == END {
            return Ok(());
        }
        if header.next_section_offset == 0 {
            return Err(invalid("zero PTCL section link"));
        }
        current = relative(current, header.next_section_offset, "section")?;
    }
    Err(invalid("too many PTCL sections"))
}

fn read_emitter(data: &[u8], base: usize) -> io::Result<Emitter> {
    let counts = read_u32x4(data, base + 0x80)?;
    let mut animation = base + 0x680;
    let color_anim0 = read_animation(data, &mut animation, counts[0])?;
    let alpha_anim0 = read_animation(data, &mut animation, counts[1])?;
    let color_anim1 = read_animation(data, &mut animation, counts[2])?;
    let alpha_anim1 = read_animation(data, &mut animation, counts[3])?;
    Ok(Emitter {
        const_color0: read_f32x4(data, base + 0xf48)?,
        const_color1: read_f32x4(data, base + 0xf58)?,
        color_anim0,
        color_anim1,
        alpha_anim0,
        alpha_anim1,
    })
}

fn read_animation(data: &[u8], offset: &mut usize, count: u32) -> io::Result<Vec<AnimKeyFrame>> {
    let count = if count > 8 { 8 } else { count + 1 } as usize;
    let mut frames = Vec::with_capacity(count);
    for index in 0..count {
        let values = read_f32x4(data, *offset + index * 16)?;
        frames.push(AnimKeyFrame {
            value: [values[0], values[1], values[2]],
            keyframe: values[3],
        });
    }
    *offset = offset
        .checked_add(ANIMATION_SLOT_SIZE)
        .ok_or_else(|| invalid("animation offset overflow"))?;
    Ok(frames)
}

fn write_emitter(data: &mut [u8], base: usize, emitter: &Emitter) -> io::Result<()> {
    write_f32x4(data, base + 0xf48, emitter.const_color0)?;
    write_f32x4(data, base + 0xf58, emitter.const_color1)?;
    let counts = [
        emitter.color_anim0.len().saturating_sub(1).min(8) as u32,
        emitter.alpha_anim0.len().saturating_sub(1).min(8) as u32,
        emitter.color_anim1.len().saturating_sub(1).min(8) as u32,
        emitter.alpha_anim1.len().saturating_sub(1).min(8) as u32,
    ];
    for (index, value) in counts.into_iter().enumerate() {
        write_bytes(data, base + 0x80 + index * 4, &value.to_le_bytes())?;
    }
    let mut animation = base + 0x680;
    for frames in [
        &emitter.color_anim0,
        &emitter.alpha_anim0,
        &emitter.color_anim1,
        &emitter.alpha_anim1,
    ] {
        for (index, frame) in frames.iter().take(8).enumerate() {
            let at = animation + index * 16;
            for (component, value) in frame
                .value
                .iter()
                .chain(std::iter::once(&frame.keyframe))
                .enumerate()
            {
                write_bytes(data, at + component * 4, &value.to_le_bytes())?;
            }
        }
        animation += ANIMATION_SLOT_SIZE;
    }
    Ok(())
}

fn read_string(data: &[u8], offset: usize, max_len: usize) -> io::Result<String> {
    let end = offset
        .checked_add(max_len)
        .ok_or_else(|| invalid("string offset overflow"))?;
    let bytes = data
        .get(offset..end)
        .ok_or_else(|| invalid("string exceeds PTCL data"))?;
    let length = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8(bytes[..length].to_vec())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn read_u32x4(data: &[u8], offset: usize) -> io::Result<[u32; 4]> {
    let mut reader = BinaryReader::new(data);
    reader.seek(offset)?;
    Ok([
        reader.read_u32()?,
        reader.read_u32()?,
        reader.read_u32()?,
        reader.read_u32()?,
    ])
}

fn read_f32x4(data: &[u8], offset: usize) -> io::Result<[f32; 4]> {
    let mut reader = BinaryReader::new(data);
    reader.seek(offset)?;
    Ok([
        reader.read_f32()?,
        reader.read_f32()?,
        reader.read_f32()?,
        reader.read_f32()?,
    ])
}

fn write_f32x4(data: &mut [u8], offset: usize, values: [f32; 4]) -> io::Result<()> {
    for (index, value) in values.into_iter().enumerate() {
        write_bytes(data, offset + index * 4, &value.to_le_bytes())?;
    }
    Ok(())
}

fn write_bytes(data: &mut [u8], offset: usize, bytes: &[u8]) -> io::Result<()> {
    let end = offset
        .checked_add(bytes.len())
        .ok_or_else(|| invalid("write offset overflow"))?;
    let target = data
        .get_mut(offset..end)
        .ok_or_else(|| invalid("write exceeds PTCL data"))?;
    target.copy_from_slice(bytes);
    Ok(())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use roead::byml::Byml;
    use sha2::{Digest, Sha256};
    use std::{fs, path::PathBuf};

    #[test]
    fn parses_and_losslessly_reapplies_all_effect_samples() {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repository root")
            .join("tmp/Effect");
        let mut paths: Vec<_> = fs::read_dir(&directory)
            .expect("Effect samples directory")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect();
        paths.sort();
        assert!(!paths.is_empty(), "no Effect samples found");

        let mut edit_was_tested = false;
        for path in paths {
            let byml_data =
                fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let byml = Byml::from_binary(&byml_data)
                .unwrap_or_else(|error| panic!("{}: invalid BYML: {error}", path.display()));
            let ptcl_data = byml
                .as_map()
                .ok()
                .and_then(|map| map.get("PtclBin"))
                .and_then(|value| match value {
                    Byml::FileData(data) => Some(data),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{}: missing PtclBin", path.display()));
            let ptcl = Ptcl::parse(ptcl_data)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let yaml = ptcl
                .to_yaml()
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let reparsed: PtclDocument = serde_yaml::from_str(&yaml)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(reparsed, ptcl.document, "{}: YAML mismatch", path.display());
            let rebuilt = ptcl
                .apply_yaml(&yaml)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            assert_eq!(
                &rebuilt,
                ptcl_data,
                "{}: PTCL hash mismatch",
                path.display()
            );

            if !edit_was_tested {
                let mut edited = ptcl.document.clone();
                if let Some(emitter) = edited
                    .0
                    .values_mut()
                    .find_map(|emitters| emitters.values_mut().next())
                {
                    emitter.const_color0[0] += 0.125;
                    let edited_binary = ptcl.apply_document(&edited).unwrap_or_else(|error| {
                        panic!("{}: failed to apply edit: {error}", path.display())
                    });
                    assert_ne!(
                        &edited_binary,
                        ptcl_data,
                        "{}: edit changed no bytes",
                        path.display()
                    );
                    let reparsed_edit = Ptcl::parse(&edited_binary).unwrap_or_else(|error| {
                        panic!("{}: failed to reparse edit: {error}", path.display())
                    });
                    assert_eq!(
                        reparsed_edit.document,
                        edited,
                        "{}: edited PTCL mismatch",
                        path.display()
                    );
                    edit_was_tested = true;
                }
            }
        }
        assert!(edit_was_tested, "no emitter was available for edit testing");
    }

    #[test]
    fn entire_effect_esetb_files_rebuild_with_matching_sha256() {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("repository root")
            .join("tmp/Effect");
        let mut paths: Vec<_> = fs::read_dir(&directory)
            .expect("Effect samples directory")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect();
        paths.sort();
        assert!(!paths.is_empty(), "no Effect samples found");

        for path in paths {
            let original =
                fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let endian = if original.starts_with(b"YB") {
                roead::Endian::Big
            } else {
                roead::Endian::Little
            };
            let mut editable = Byml::from_binary(&original)
                .unwrap_or_else(|error| panic!("{}: invalid BYML: {error}", path.display()));
            let map = editable
                .as_mut_map()
                .unwrap_or_else(|error| panic!("{}: root is not a map: {error}", path.display()));
            let ptcl_data = match map.remove("PtclBin") {
                Some(Byml::FileData(data)) => data,
                _ => panic!("{}: missing PtclBin", path.display()),
            };
            let ptcl = Ptcl::parse(&ptcl_data)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let yaml = ptcl
                .to_yaml()
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let yaml_node = Byml::from_text(&yaml)
                .unwrap_or_else(|error| panic!("{}: invalid PTCL YAML: {error}", path.display()));
            map.insert("PTCL_JSON".into(), yaml_node);

            // Exercise the same editable YAML boundary used by the GUI.
            let editable_text = editable.to_text();
            let mut rebuilt_byml = Byml::from_text(&editable_text).unwrap_or_else(|error| {
                panic!("{}: editable YAML failed: {error}", path.display())
            });
            let rebuilt_map = rebuilt_byml.as_mut_map().unwrap_or_else(|error| {
                panic!("{}: rebuilt root is not a map: {error}", path.display())
            });
            let ptcl_yaml = rebuilt_map
                .remove("PTCL_JSON")
                .unwrap_or_else(|| panic!("{}: missing PTCL_JSON", path.display()));
            let rebuilt_ptcl = ptcl
                .apply_yaml(&ptcl_yaml.to_text())
                .unwrap_or_else(|error| panic!("{}: PTCL rebuild failed: {error}", path.display()));
            rebuilt_map.insert("PtclBin".into(), Byml::FileData(rebuilt_ptcl));
            let rebuilt = crate::file_format::Esetb::serialize_preserving_original(
                &rebuilt_byml,
                &original,
                endian,
            );

            assert_eq!(
                Sha256::digest(&rebuilt),
                Sha256::digest(&original),
                "{}: complete ESETB SHA-256 mismatch",
                path.display()
            );
        }
    }
}
