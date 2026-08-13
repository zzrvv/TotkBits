use super::{
    actor::Actor,
    container::{read_container, Container},
    radix_tree::read_string_ptr,
    timeline_clip::Clip,
    timeline_cut::Cut,
    timeline_oneshot::Oneshot,
    timeline_subtimeline::SubTimeline,
    timeline_trigger::Trigger,
};
use crate::parser::binary::BinaryReader;
use serde::{Deserialize, Serialize};
use std::io::{self, ErrorKind};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "PascalCase")]
pub struct Timeline {
    pub name: String,
    pub duration: f32,
    pub actors: Vec<Actor>,
    pub clips: Vec<Clip>,
    pub oneshots: Vec<Oneshot>,
    pub triggers: Vec<Trigger>,
    pub sub_timelines: Vec<SubTimeline>,
    pub cuts: Vec<Cut>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Container>,
}

impl Timeline {
    pub fn read(data: &[u8], offset: u64) -> io::Result<Self> {
        let mut reader = BinaryReader::new(data);
        reader.seek(offset as usize)?;
        if reader.read_bytes(4)? != b"TLIN" {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "invalid timeline magic",
            ));
        }
        reader.skip(12)?;
        let duration = reader.read_f32()?;
        let actor_count = reader.read_u16()? as usize;
        reader.skip(2)?; // Total action count is derived from actors.
        let clip_count = reader.read_u16()? as usize;
        let oneshot_count = reader.read_u16()? as usize;
        let sub_timeline_count = reader.read_u16()? as usize;
        let cut_count = reader.read_u16()? as usize;
        let name = read_string_ptr(&mut reader, data)?;
        let actors_offset = reader.read_u64()?;
        let clips_offset = reader.read_u64()?;
        let oneshots_offset = reader.read_u64()?;
        let triggers_offset = reader.read_u64()?;
        let sub_timelines_offset = reader.read_u64()?;
        let cuts_offset = reader.read_u64()?;
        let parameters_offset = reader.read_u64()?;

        let actors = read_array(actor_count, actors_offset, 56, |at| Actor::read(data, at))?;
        let clips = read_array(clip_count, clips_offset, 24, |at| Clip::read(data, at))?;
        let oneshots = read_array(oneshot_count, oneshots_offset, 24, |at| {
            Oneshot::read(data, at)
        })?;
        let triggers = read_array(clip_count * 2, triggers_offset, 4, |at| {
            Trigger::read(data, at)
        })?;
        let sub_timelines = read_array(sub_timeline_count, sub_timelines_offset, 8, |at| {
            SubTimeline::read(data, at)
        })?;
        let cuts = read_array(cut_count, cuts_offset, 24, |at| Cut::read(data, at))?;
        let parameters = (parameters_offset != 0)
            .then(|| read_container(data, parameters_offset))
            .transpose()?;
        Ok(Self {
            name,
            duration,
            actors,
            clips,
            oneshots,
            triggers,
            sub_timelines,
            cuts,
            parameters,
        })
    }
}

fn read_array<T>(
    count: usize,
    offset: u64,
    stride: u64,
    mut read: impl FnMut(u64) -> io::Result<T>,
) -> io::Result<Vec<T>> {
    (0..count)
        .map(|index| read(offset + index as u64 * stride))
        .collect()
}
