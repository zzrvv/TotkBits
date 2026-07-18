use super::{
    actor::Actor,
    entry_point::EntryPoint,
    event::Event,
    radix_tree::{read_keys, read_string_ptr},
};
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{self, ErrorKind},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Flowchart {
    pub name: String,
    pub actors: Vec<Actor>,
    pub events: Vec<Event>,
    pub entry_points: BTreeMap<String, EntryPoint>,
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
        reader.skip(12)?;
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
        let mut entry_points = BTreeMap::new();
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
        })
    }
}
