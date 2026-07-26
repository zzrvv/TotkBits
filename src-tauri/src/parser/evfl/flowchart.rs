use super::{
    actor::Actor,
    entry_point::EntryPoint,
    event::Event,
    radix_tree::{read_keys, read_string_ptr},
};
use crate::parser::binary::BinaryReader;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::io::{self, ErrorKind};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Flowchart {
    pub name: String,
    pub actors: Vec<Actor>,
    pub events: Vec<Event>,
    pub entry_points: IndexMap<String, EntryPoint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub empty_entry_point_trailers: Vec<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub omitted_variable_entry_point_trailers: Vec<usize>,
}

impl Flowchart {
    pub fn read(data: &[u8], offset: u64) -> io::Result<Self> {
        let mut reader = BinaryReader::new(data);
        reader.seek(offset as usize)?;
        if reader.read_bytes(4)? != b"EVFL" {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "invalid flowchart magic",
            ));
        }
        let _string_pool_offset = offset as usize + reader.read_u32()? as usize;
        reader.skip(8)?;
        let actor_count = reader.read_u16()? as usize;
        reader.skip(4)?;
        let event_count = reader.read_u16()? as usize;
        let entry_count = reader.read_u16()? as usize;
        reader.skip(6)?;
        let name = read_string_ptr(&mut reader, data)?;
        let actors_offset = reader.read_u64()?;
        let events_offset = reader.read_u64()?;
        let entry_dictionary_offset = reader.read_u64()?;
        let entries_offset = reader.read_u64()?;

        let actors = (0..actor_count)
            .map(|index| Actor::read(data, actors_offset + index as u64 * 56))
            .collect::<io::Result<Vec<_>>>()?;
        let mut events = (0..event_count)
            .map(|index| Event::read(data, events_offset + index as u64 * 40))
            .collect::<io::Result<Vec<_>>>()?;
        let event_names = events
            .iter()
            .map(|event| event.name().to_owned())
            .collect::<Vec<_>>();
        for event in &mut events {
            event.resolve(&event_names, &actors);
        }

        let entry_names = read_keys(data, entry_dictionary_offset)?;
        if entry_names.len() != entry_count {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "entry point dictionary length mismatch",
            ));
        }
        let mut entry_points = IndexMap::new();
        for (index, entry_name) in entry_names.into_iter().enumerate() {
            entry_points.insert(
                entry_name,
                EntryPoint::read(data, entries_offset + index as u64 * 32)?,
            );
        }
        Ok(Self {
            name,
            actors,
            events,
            entry_points,
            empty_entry_point_trailers: Vec::new(),
            omitted_variable_entry_point_trailers: Vec::new(),
        })
    }
}
