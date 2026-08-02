use super::{
    actor::Actor,
    container::{Container, ContainerItem},
    document::BfevDocument,
    entry_point::{EntryPoint, VariableDef},
    event::Event,
    flowchart::Flowchart,
    timeline::Timeline,
};
use crate::parser::binary::BinaryWriter;
use indexmap::IndexMap;
use std::{collections::HashMap, io};

pub fn write_document(document: &BfevDocument) -> io::Result<Vec<u8>> {
    let mut w = EvflWriter::new();
    w.bytes(b"BFEVFL");
    w.u16(0);
    let version = document
        .version
        .split('.')
        .map(|part| part.parse::<u8>().map_err(|e| invalid(e.to_string())))
        .collect::<io::Result<Vec<_>>>()?;
    if version.len() != 4 {
        return Err(invalid("EVFL version must contain four bytes"));
    }
    w.bytes(&version);
    w.u16(0xfeff);
    w.u8(3);
    w.u8(0);
    w.string_ref(&document.file_name, true);
    w.u16(0);
    let first_block = w.reserve_u16();
    let relocation_offset = w.reserve_u32();
    let file_size = w.reserve_u32();
    w.u16(document.flowcharts.len() as u16);
    w.u16(document.timelines.len() as u16);
    w.u32(0);
    let flow_offsets = w.reserve_ptr(false);
    let flow_dictionary = w.reserve_ptr(false);
    let timeline_offsets = w.reserve_ptr(false);
    let timeline_dictionary = w.reserve_ptr(false);

    let flow_slots = if !document.flowcharts.is_empty() {
        w.patch_u64(flow_offsets, w.pos() as u64);
        let slots = (0..document.flowcharts.len())
            .map(|_| w.reserve_ptr(false))
            .collect::<Vec<_>>();
        slots
    } else {
        Vec::new()
    };
    w.patch_u64(flow_dictionary, w.pos() as u64);
    w.write_radix(document.flowcharts.keys().map(String::as_str))?;

    let timeline_slots = if !document.timelines.is_empty() {
        w.patch_u64(timeline_offsets, w.pos() as u64);
        (0..document.timelines.len())
            .map(|_| w.reserve_ptr(false))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    w.patch_u64(timeline_dictionary, w.pos() as u64);
    w.write_radix(document.timelines.keys().map(String::as_str))?;

    for ((_, flowchart), slot) in document.flowcharts.iter().zip(flow_slots) {
        w.patch_u64(slot, w.pos() as u64);
        w.patch_u16(first_block, w.pos() as u16);
        w.write_flowchart(flowchart)?;
    }
    for ((_, timeline), slot) in document.timelines.iter().zip(timeline_slots) {
        w.write_timeline(timeline, first_block, slot)?;
    }

    w.write_string_pool()?;
    for pointer in &document.relocation_removals {
        w.pointers.retain(|existing| *existing != *pointer as usize);
    }
    w.pointers.extend(
        document
            .relocation_additions
            .iter()
            .map(|pointer| *pointer as usize),
    );
    w.write_relocation_table(relocation_offset);
    w.patch_u32(file_size, w.pos() as u32);
    Ok(w.finish())
}

struct EvflWriter {
    out: BinaryWriter,
    pointers: Vec<usize>,
    strings: HashMap<String, Vec<(usize, bool)>>,
}

impl EvflWriter {
    fn new() -> Self {
        let mut strings = HashMap::new();
        strings.insert(String::new(), Vec::new());
        Self {
            out: BinaryWriter::new(),
            pointers: Vec::new(),
            strings,
        }
    }
    fn finish(self) -> Vec<u8> {
        self.out.into_inner()
    }
    fn pos(&self) -> usize {
        self.out.position()
    }
    fn seek(&mut self, pos: usize) {
        self.out.seek(pos);
    }
    fn bytes(&mut self, value: &[u8]) {
        self.out.write_bytes(value);
    }
    fn u8(&mut self, value: u8) {
        self.out.write_u8(value);
    }
    fn u16(&mut self, value: u16) {
        self.out.write_u16(value);
    }
    fn i16(&mut self, value: i16) {
        self.out.write_i16(value);
    }
    fn u32(&mut self, value: u32) {
        self.out.write_u32(value);
    }
    fn i32(&mut self, value: i32) {
        self.out.write_i32(value);
    }
    fn u64(&mut self, value: u64) {
        self.out.write_u64(value);
    }
    fn f32(&mut self, value: f32) {
        self.out.write_f32(value);
    }
    fn align(&mut self, alignment: usize) -> io::Result<()> {
        self.out.align(alignment)
    }
    fn patch_u16(&mut self, at: usize, value: u16) {
        let pos = self.pos();
        self.seek(at);
        self.u16(value);
        self.seek(pos);
    }
    fn patch_u32(&mut self, at: usize, value: u32) {
        let pos = self.pos();
        self.seek(at);
        self.u32(value);
        self.seek(pos);
    }
    fn patch_u64(&mut self, at: usize, value: u64) {
        let pos = self.pos();
        self.seek(at);
        self.u64(value);
        self.seek(pos);
    }
    fn reserve_u16(&mut self) -> usize {
        let at = self.pos();
        self.u16(0);
        at
    }
    fn reserve_u32(&mut self) -> usize {
        let at = self.pos();
        self.u32(0);
        at
    }
    fn reserve_ptr(&mut self, null: bool) -> usize {
        self.reserve_ptr_with_null_registration(null, false)
    }
    fn reserve_ptr_with_null_registration(&mut self, null: bool, register_null: bool) -> usize {
        let at = self.pos();
        if !null || register_null {
            self.pointers.push(at);
        }
        self.u64(0);
        at
    }
    fn string_ref(&mut self, value: &str, header: bool) {
        let at = self.pos();
        if header {
            self.u32(0);
        } else {
            self.pointers.push(at);
            self.u64(0);
        }
        self.strings
            .entry(value.to_owned())
            .or_default()
            .push((at, header));
    }
    fn inline_strings(&mut self, alignment: usize, values: &[&str]) -> io::Result<()> {
        let slots = (0..values.len())
            .map(|_| self.reserve_ptr(false))
            .collect::<Vec<_>>();
        for (index, (slot, value)) in slots.into_iter().zip(values).enumerate() {
            if alignment == 2 && index != 0 {
                self.align(2)?;
            }
            self.patch_u64(slot, self.pos() as u64);
            self.pascal(value);
        }
        Ok(())
    }
    fn pascal(&mut self, value: &str) {
        self.u16(value.len() as u16);
        self.bytes(value.as_bytes());
        self.u8(0);
    }

    fn write_flowchart(&mut self, flow: &Flowchart) -> io::Result<()> {
        self.bytes(b"EVFL");
        let pool_relative = self.reserve_u32();
        self.u64(0);
        self.u16(flow.actors.len() as u16);
        self.u16(flow.actors.iter().map(|a| a.actions.len()).sum::<usize>() as u16);
        self.u16(flow.actors.iter().map(|a| a.queries.len()).sum::<usize>() as u16);
        self.u16(flow.events.len() as u16);
        self.u16(flow.entry_points.len() as u16);
        self.bytes(&[0; 6]);
        self.string_ref(&flow.name, false);
        let actors_ptr = self.reserve_ptr_with_null_registration(flow.actors.is_empty(), true);
        let events_ptr = self.reserve_ptr_with_null_registration(flow.events.is_empty(), true);
        let entry_dict_ptr = self.reserve_ptr(false);
        let entries_ptr =
            self.reserve_ptr_with_null_registration(flow.entry_points.is_empty(), true);

        let actor_deferred = if flow.actors.is_empty() {
            Vec::new()
        } else {
            self.patch_u64(actors_ptr, self.pos() as u64);
            flow.actors
                .iter()
                .map(|actor| self.write_actor_header(actor))
                .collect::<io::Result<Vec<_>>>()?
        };
        let event_deferred = if flow.events.is_empty() {
            Vec::new()
        } else {
            self.patch_u64(events_ptr, self.pos() as u64);
            let mut deferred = Vec::with_capacity(flow.events.len());
            for event in &flow.events {
                deferred.push(self.write_event_header(event)?);
            }
            deferred
        };
        self.patch_u64(entry_dict_ptr, self.pos() as u64);
        self.write_radix(flow.entry_points.keys().map(String::as_str))?;
        self.align(8)?;
        let entry_deferred = if flow.entry_points.is_empty() {
            Vec::new()
        } else {
            self.patch_u64(entries_ptr, self.pos() as u64);
            flow.entry_points
                .values()
                .map(|entry| self.write_entry_header(entry))
                .collect::<Vec<_>>()
        };

        for deferred in event_deferred {
            self.align(8)?;
            self.write_event_extra(deferred)?;
        }
        for (actor, deferred) in flow.actors.iter().zip(actor_deferred) {
            self.align(8)?;
            self.write_actor_extra(actor, deferred)?;
        }
        for (index, (entry, deferred)) in flow.entry_points.values().zip(entry_deferred).enumerate()
        {
            self.align(8)?;
            self.write_entry_extra(
                entry,
                deferred,
                flow.empty_entry_point_trailers.contains(&index),
                flow.omitted_variable_entry_point_trailers.contains(&index),
            )?;
        }
        self.align(8)?;
        self.strings
            .entry(format!("__POOL_PATCH_{pool_relative}"))
            .or_default();
        Ok(())
    }

    fn write_timeline(
        &mut self,
        timeline: &Timeline,
        first_block: usize,
        timeline_slot: usize,
    ) -> io::Result<()> {
        let mut actor_data = Vec::with_capacity(timeline.actors.len());
        for actor in &timeline.actors {
            self.align(8)?;
            let parameters = if let Some(parameters) = &actor.parameters {
                let offset = self.pos();
                self.write_container(parameters)?;
                offset
            } else {
                0
            };
            self.align(8)?;
            let actions = if actor.actions.is_empty() {
                0
            } else {
                let offset = self.pos();
                for action in &actor.actions {
                    self.string_ref(action, false);
                }
                offset
            };
            self.align(8)?;
            let queries = if actor.queries.is_empty() {
                0
            } else {
                let offset = self.pos();
                for query in &actor.queries {
                    self.string_ref(query, false);
                }
                offset
            };
            actor_data.push((parameters, actions, queries));
            self.align(8)?;
        }

        let timeline_parameters = if let Some(parameters) = timeline
            .parameters
            .as_ref()
            .filter(|parameters| !parameters.is_empty())
        {
            let offset = self.pos();
            self.write_container(parameters)?;
            offset
        } else {
            0
        };

        self.align(8)?;
        self.patch_u64(timeline_slot, self.pos() as u64);
        self.patch_u16(first_block, self.pos() as u16);
        self.bytes(b"TLIN");
        let pool_relative = self.reserve_u32();
        self.u64(0);
        self.f32(timeline.duration);
        self.u16(timeline.actors.len() as u16);
        self.u16(
            timeline
                .actors
                .iter()
                .map(|actor| actor.actions.len())
                .sum::<usize>() as u16,
        );
        self.u16(timeline.clips.len() as u16);
        self.u16(timeline.oneshots.len() as u16);
        self.u16(timeline.sub_timelines.len() as u16);
        self.u16(timeline.cuts.len() as u16);
        self.string_ref(&timeline.name, false);
        let actors = self.reserve_ptr_with_null_registration(timeline.actors.is_empty(), true);
        let clips = self.reserve_ptr_with_null_registration(timeline.clips.is_empty(), true);
        let oneshots = self.reserve_ptr_with_null_registration(timeline.oneshots.is_empty(), true);
        let triggers = self.reserve_ptr_with_null_registration(timeline.triggers.is_empty(), true);
        let sub_timelines =
            self.reserve_ptr_with_null_registration(timeline.sub_timelines.is_empty(), true);
        let cuts = self.reserve_ptr_with_null_registration(timeline.cuts.is_empty(), true);
        self.pointers.push(self.pos());
        self.u64(timeline_parameters as u64);

        if !timeline.actors.is_empty() {
            self.patch_u64(actors, self.pos() as u64);
            for (actor, (parameters, actions, queries)) in timeline.actors.iter().zip(actor_data) {
                self.string_ref(&actor.name, false);
                self.string_ref(&actor.secondary_name, false);
                self.string_ref(&actor.argument_name, false);
                self.pointers.push(self.pos());
                self.u64(actions as u64);
                self.pointers.push(self.pos());
                self.u64(queries as u64);
                if parameters != 0 {
                    self.pointers.push(self.pos());
                }
                self.u64(parameters as u64);
                self.u16(actor.actions.len() as u16);
                self.u16(actor.queries.len() as u16);
                self.i16(actor.entry_point_index);
                self.u8(actor.cut_number);
                self.u8(0);
            }
        }
        self.align(8)?;

        let mut clip_params = Vec::new();
        if !timeline.clips.is_empty() {
            self.patch_u64(clips, self.pos() as u64);
            for clip in &timeline.clips {
                self.f32(clip.start_time);
                self.f32(clip.duration);
                self.i16(clip.actor_index);
                self.i16(clip.actor_action_index);
                self.u8(clip.unknown);
                self.bytes(&[0; 3]);
                clip_params.push(
                    self.reserve_ptr(clip.parameters.as_ref().is_none_or(IndexMap::is_empty)),
                );
            }
        }
        self.align(8)?;

        let mut oneshot_params = Vec::new();
        if !timeline.oneshots.is_empty() {
            self.patch_u64(oneshots, self.pos() as u64);
            for oneshot in &timeline.oneshots {
                self.f32(oneshot.time);
                self.i16(oneshot.actor_index);
                self.i16(oneshot.actor_action_index);
                self.u64(0);
                oneshot_params.push(
                    self.reserve_ptr(oneshot.parameters.as_ref().is_none_or(IndexMap::is_empty)),
                );
            }
        }
        self.align(8)?;

        if !timeline.sub_timelines.is_empty() {
            self.patch_u64(sub_timelines, self.pos() as u64);
            for sub_timeline in &timeline.sub_timelines {
                self.string_ref(&sub_timeline.name, false);
            }
        }
        self.align(8)?;
        if !timeline.triggers.is_empty() {
            self.patch_u64(triggers, self.pos() as u64);
            for trigger in &timeline.triggers {
                self.i16(trigger.clip_index);
                self.u8(trigger.trigger_type);
                self.u8(0);
            }
        }
        self.align(8)?;

        let mut cut_params = Vec::new();
        if !timeline.cuts.is_empty() {
            self.patch_u64(cuts, self.pos() as u64);
            for cut in &timeline.cuts {
                self.f32(cut.start_time);
                self.u32(0);
                self.string_ref(&cut.name, false);
                cut_params
                    .push(self.reserve_ptr(cut.parameters.as_ref().is_none_or(IndexMap::is_empty)));
            }
        }
        self.align(8)?;

        for (clip, slot) in timeline.clips.iter().zip(clip_params) {
            if let Some(parameters) = &clip.parameters {
                if !parameters.is_empty() {
                    self.align(8)?;
                    self.patch_u64(slot, self.pos() as u64);
                    self.write_container(parameters)?;
                }
            }
        }
        for (oneshot, slot) in timeline.oneshots.iter().zip(oneshot_params) {
            if let Some(parameters) = &oneshot.parameters {
                if !parameters.is_empty() {
                    self.align(8)?;
                    self.patch_u64(slot, self.pos() as u64);
                    self.write_container(parameters)?;
                }
            }
        }
        for (cut, slot) in timeline.cuts.iter().zip(cut_params) {
            if let Some(parameters) = &cut.parameters {
                if !parameters.is_empty() {
                    self.align(8)?;
                    self.patch_u64(slot, self.pos() as u64);
                    self.write_container(parameters)?;
                }
            }
        }
        self.align(8)?;
        self.strings
            .entry(format!("__POOL_PATCH_{pool_relative}"))
            .or_default();
        Ok(())
    }

    fn write_actor_header(&mut self, actor: &Actor) -> io::Result<ActorDeferred> {
        self.string_ref(&actor.name, false);
        self.string_ref(&actor.secondary_name, false);
        self.string_ref(&actor.argument_name, false);
        let actions = self.reserve_ptr_with_null_registration(actor.actions.is_empty(), true);
        let queries = self.reserve_ptr_with_null_registration(actor.queries.is_empty(), true);
        let params = self.reserve_ptr(actor.parameters.is_none());
        self.u16(actor.actions.len() as u16);
        self.u16(actor.queries.len() as u16);
        self.i16(actor.entry_point_index);
        self.u8(actor.cut_number);
        self.u8(0);
        Ok(ActorDeferred {
            actions,
            queries,
            params,
        })
    }
    fn write_actor_extra(&mut self, actor: &Actor, d: ActorDeferred) -> io::Result<()> {
        if let Some(parameters) = &actor.parameters {
            self.patch_u64(d.params, self.pos() as u64);
            self.write_container(parameters)?;
        }
        if !actor.actions.is_empty() {
            self.align(8)?;
            self.patch_u64(d.actions, self.pos() as u64);
            for value in &actor.actions {
                self.string_ref(value, false);
            }
        }
        if !actor.queries.is_empty() {
            self.align(8)?;
            self.patch_u64(d.queries, self.pos() as u64);
            for value in &actor.queries {
                self.string_ref(value, false);
            }
        }
        Ok(())
    }

    fn write_event_header<'a>(&mut self, event: &'a Event) -> io::Result<EventDeferred<'a>> {
        self.string_ref(event.name(), false);
        match event {
            Event::Action(e) => {
                self.u8(0);
                self.u8(0);
                self.i16(e.next_event_index);
                self.i16(e.actor_index);
                self.i16(e.actor_action_index);
                let params = self.reserve_ptr(e.parameters.as_ref().is_none_or(IndexMap::is_empty));
                self.u64(0);
                self.u64(0);
                Ok(EventDeferred::Action(params, e.parameters.as_ref()))
            }
            Event::Switch(e) => {
                self.u8(1);
                self.u8(0);
                self.u16(e.switch_cases.len() as u16);
                self.i16(e.actor_index);
                self.i16(e.actor_query_index);
                let params = self.reserve_ptr(e.parameters.as_ref().is_none_or(IndexMap::is_empty));
                let cases =
                    self.reserve_ptr_with_null_registration(e.switch_cases.is_empty(), true);
                self.u64(0);
                Ok(EventDeferred::Switch(params, cases, e))
            }
            Event::Fork(e) => {
                self.u8(2);
                self.u8(0);
                self.u16(e.fork_event_indicies.len() as u16);
                self.i16(e.join_event_index);
                self.u16(0);
                let indices = self.reserve_ptr(false);
                self.u64(0);
                self.u64(0);
                Ok(EventDeferred::Fork(indices, e))
            }
            Event::Join(e) => {
                self.u8(3);
                self.u8(0);
                self.i16(e.next_event_index);
                self.u16(0);
                self.u16(0);
                self.u64(0);
                self.u64(0);
                self.u64(0);
                Ok(EventDeferred::None)
            }
            Event::Subflow(e) => {
                self.u8(4);
                self.u8(0);
                self.i16(e.next_event_index);
                self.u16(0);
                self.u16(0);
                let params = self.reserve_ptr(e.parameters.as_ref().is_none_or(IndexMap::is_empty));
                self.string_ref(&e.flowchart_name, false);
                self.string_ref(&e.entry_point_name, false);
                Ok(EventDeferred::Action(params, e.parameters.as_ref()))
            }
        }
    }
    fn write_event_extra(&mut self, event: EventDeferred<'_>) -> io::Result<()> {
        match event {
            EventDeferred::Action(slot, Some(params)) if !params.is_empty() => {
                self.patch_u64(slot, self.pos() as u64);
                self.write_container(params)?;
            }
            EventDeferred::Switch(params_slot, cases_slot, event) => {
                if !event.switch_cases.is_empty() {
                    self.align(8)?;
                    self.patch_u64(cases_slot, self.pos() as u64);
                    for case in &event.switch_cases {
                        self.i32(case.value);
                        self.u16(case.event_index);
                        self.u16(0);
                    }
                }
                if let Some(params) = &event.parameters {
                    if !params.is_empty() {
                        self.patch_u64(params_slot, self.pos() as u64);
                        self.write_container(params)?;
                    }
                }
            }
            EventDeferred::Fork(slot, event) if !event.fork_event_indicies.is_empty() => {
                self.patch_u64(slot, self.pos() as u64);
                for index in &event.fork_event_indicies {
                    self.i16(*index);
                }
                self.align(8)?;
            }
            _ => {}
        }
        Ok(())
    }

    fn write_entry_header(&mut self, entry: &EntryPoint) -> EntryDeferred {
        let subflows =
            self.reserve_ptr_with_null_registration(entry.sub_flow_event_indices.is_empty(), true);
        let has_variables = entry.variables.as_ref().is_some_and(|v| !v.is_empty());
        let dictionary = self.reserve_ptr_with_null_registration(
            !has_variables,
            entry.register_null_variable_dictionary,
        );
        let variables = self.reserve_ptr_with_null_registration(
            !has_variables,
            entry.register_null_variable_definitions,
        );
        self.u16(entry.sub_flow_event_indices.len() as u16);
        self.u16(entry.variables.as_ref().map_or(0, IndexMap::len) as u16);
        self.i16(entry.event_index);
        self.u16(0);
        EntryDeferred {
            subflows,
            dictionary,
            variables,
        }
    }
    fn write_entry_extra(
        &mut self,
        entry: &EntryPoint,
        d: EntryDeferred,
        empty_trailer: bool,
        omit_variable_trailer: bool,
    ) -> io::Result<()> {
        if !entry.sub_flow_event_indices.is_empty() {
            self.patch_u64(d.subflows, self.pos() as u64);
            for index in &entry.sub_flow_event_indices {
                self.i16(*index);
            }
            self.align(8)?;
        }
        if let Some(variables) = &entry.variables {
            if !variables.is_empty() {
                self.patch_u64(d.dictionary, self.pos() as u64);
                self.write_radix(variables.keys().map(String::as_str))?;
                self.align(8)?;
                self.patch_u64(d.variables, self.pos() as u64);
                let deferred = variables
                    .values()
                    .map(|value| self.write_variable(value))
                    .collect::<io::Result<Vec<_>>>()?;
                for item in deferred {
                    self.align(8)?;
                    if let Some((slot, value)) = item {
                        self.patch_u64(slot, self.pos() as u64);
                        self.write_variable_array(value);
                    }
                }
                if !omit_variable_trailer {
                    self.seek(self.pos() + 0x18);
                }
            }
        }
        if entry.variables.as_ref().is_none_or(IndexMap::is_empty) && empty_trailer {
            self.seek(self.pos() + 0x18);
        }
        Ok(())
    }

    fn write_variable<'a>(
        &mut self,
        variable: &'a VariableDef,
    ) -> io::Result<Option<(usize, &'a ContainerItem)>> {
        let item = &variable.value;
        if let Some(value) = item.int {
            self.u64(value as u64);
            self.u16(1);
            self.u8(2);
            self.bytes(&[0; 5]);
            return Ok(None);
        }
        if let Some(value) = item.bool {
            self.u64(value as u64);
            self.u16(1);
            self.u8(3);
            self.bytes(&[0; 5]);
            return Ok(None);
        }
        if let Some(value) = item.float {
            self.u64(value as i64 as u64);
            self.u16(1);
            self.u8(4);
            self.bytes(&[0; 5]);
            return Ok(None);
        }
        let (count, kind) = if let Some(value) = &item.int_array {
            (value.len(), 7)
        } else if let Some(value) = &item.bool_array {
            (value.len(), 8)
        } else if let Some(value) = &item.float_array {
            (value.len(), 9)
        } else {
            return Err(invalid("unsupported EVFL variable definition"));
        };
        let slot = self.reserve_ptr(false);
        self.u16(count as u16);
        self.u8(kind);
        self.bytes(&[0; 5]);
        Ok(Some((slot, item)))
    }
    fn write_variable_array(&mut self, item: &ContainerItem) {
        if let Some(values) = &item.int_array {
            for value in values {
                self.i32(*value);
            }
        } else if let Some(values) = &item.bool_array {
            for value in values {
                self.i32(*value as i32);
            }
        } else if let Some(values) = &item.float_array {
            for value in values {
                self.f32(*value);
            }
        }
    }

    fn write_container(&mut self, container: &Container) -> io::Result<()> {
        self.u8(1);
        self.u8(0);
        self.u16(container.len() as u16);
        self.u32(0);
        let dictionary = self.reserve_ptr(false);
        let item_slots = (0..container.len())
            .map(|_| self.reserve_ptr(false))
            .collect::<Vec<_>>();
        self.patch_u64(dictionary, self.pos() as u64);
        self.write_radix(container.keys().map(String::as_str))?;
        for ((_, item), slot) in container.iter().zip(item_slots) {
            self.align(8)?;
            self.patch_u64(slot, self.pos() as u64);
            self.write_container_item(item)?;
        }
        Ok(())
    }
    fn write_container_item(&mut self, item: &ContainerItem) -> io::Result<()> {
        let (kind, count) = item_kind_count(item)?;
        self.u8(kind);
        self.u8(0);
        self.u16(count as u16);
        self.u32(0);
        self.u64(0);
        match kind {
            0 => self.inline_strings(
                2,
                &[item
                    .argument
                    .as_deref()
                    .ok_or_else(|| invalid("missing EVFL argument"))?],
            )?,
            1 => {
                let items = item
                    .items
                    .as_ref()
                    .ok_or_else(|| invalid("missing EVFL container"))?;
                self.write_radix(items.keys().map(String::as_str))?;
                for value in items.values() {
                    self.write_container_item(value)?;
                }
            }
            2 => self.i32(item.int.ok_or_else(|| invalid("missing EVFL integer"))?),
            3 => self.u32(
                if item.bool.ok_or_else(|| invalid("missing EVFL boolean"))? {
                    0x8000_0001
                } else {
                    0
                },
            ),
            4 => self.f32(item.float.ok_or_else(|| invalid("missing EVFL float"))?),
            5 => self.inline_strings(
                2,
                &[item
                    .string
                    .as_deref()
                    .ok_or_else(|| invalid("missing EVFL string"))?],
            )?,
            6 => self.inline_strings(
                2,
                &[item
                    .w_string
                    .as_deref()
                    .ok_or_else(|| invalid("missing EVFL wide string"))?],
            )?,
            7 => item
                .int_array
                .as_ref()
                .ok_or_else(|| invalid("missing EVFL integer array"))?
                .iter()
                .for_each(|v| self.i32(*v)),
            8 => item
                .bool_array
                .as_ref()
                .ok_or_else(|| invalid("missing EVFL boolean array"))?
                .iter()
                .for_each(|v| self.u32(if *v { 0x8000_0001 } else { 0 })),
            9 => item
                .float_array
                .as_ref()
                .ok_or_else(|| invalid("missing EVFL float array"))?
                .iter()
                .for_each(|v| self.f32(*v)),
            10 => self.inline_strings(
                8,
                &item
                    .string_array
                    .as_ref()
                    .ok_or_else(|| invalid("missing EVFL string array"))?
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )?,
            11 => self.inline_strings(
                8,
                &item
                    .w_string_array
                    .as_ref()
                    .ok_or_else(|| invalid("missing EVFL wide string array"))?
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            )?,
            12 => {
                let id = item
                    .actor_identifier
                    .as_ref()
                    .ok_or_else(|| invalid("missing EVFL actor identifier"))?;
                self.inline_strings(2, &[&id.item1, &id.item2])?;
            }
            _ => return Err(invalid("unsupported EVFL container item type")),
        }
        Ok(())
    }

    fn write_radix<'a>(&mut self, keys: impl Iterator<Item = &'a str>) -> io::Result<()> {
        let keys = keys.collect::<Vec<_>>();
        let nodes = radix_nodes(&keys)?;
        self.bytes(b"DIC ");
        self.u32(keys.len() as u32);
        for node in nodes {
            self.i32(node.bit);
            self.u16(node.child[0] as u16);
            self.u16(node.child[1] as u16);
            self.string_ref(&node.name, false);
        }
        Ok(())
    }

    fn write_string_pool(&mut self) -> io::Result<()> {
        self.align(8)?;
        let pool_start = self.pos();
        // Patch each EVFL relative pool offset recorded as a synthetic key.
        let patches = self
            .strings
            .keys()
            .filter_map(|key| key.strip_prefix("__POOL_PATCH_")?.parse::<usize>().ok())
            .collect::<Vec<_>>();
        for patch in patches {
            self.patch_u32(patch, (pool_start - (patch - 4)) as u32);
            self.strings.remove(&format!("__POOL_PATCH_{patch}"));
        }
        self.bytes(b"STR ");
        self.u32(0);
        self.u64(0);
        self.u32((self.strings.len() - 1) as u32);
        let mut strings = self.strings.keys().cloned().collect::<Vec<_>>();
        strings.sort_by(|a, b| reverse_bits(a).cmp(&reverse_bits(b)));
        for value in strings {
            let at = self.pos();
            if let Some(refs) = self.strings.get(&value).cloned() {
                for (slot, header) in refs {
                    if header {
                        self.patch_u32(slot, at as u32 + 2);
                    } else {
                        self.patch_u64(slot, at as u64);
                    }
                }
            }
            self.pascal(&value);
            self.align(2)?;
        }
        Ok(())
    }

    fn write_relocation_table(&mut self, header_slot: usize) {
        let data_end = self.pos() as u32;
        let _ = self.align(8);
        self.patch_u32(header_slot, self.pos() as u32);
        self.bytes(b"RELT");
        self.u32((self.pos() - 4) as u32);
        self.u32(1);
        self.u32(0);
        self.u64(0);
        self.u32(0);
        self.u32(data_end);
        self.u32(0);
        let count_slot = self.reserve_u32();
        self.pointers.sort_unstable();
        self.pointers.dedup();
        let mut remaining = self.pointers.clone();
        let mut entries = 0u32;
        while let Some(pointer) = remaining.first().copied() {
            let mut flags = 0u32;
            for bit in 0..32 {
                let address = pointer + bit * 8;
                if let Ok(index) = remaining.binary_search(&address) {
                    flags |= 1 << bit;
                    remaining.remove(index);
                }
            }
            self.u32(pointer as u32);
            self.u32(flags);
            entries += 1;
        }
        self.patch_u32(count_slot, entries);
    }
}

#[derive(Clone, Copy)]
struct ActorDeferred {
    actions: usize,
    queries: usize,
    params: usize,
}
enum EventDeferred<'a> {
    None,
    Action(usize, Option<&'a Container>),
    Switch(usize, usize, &'a super::event::SwitchEvent),
    Fork(usize, &'a super::event::ForkEvent),
}
#[derive(Clone, Copy)]
struct EntryDeferred {
    subflows: usize,
    dictionary: usize,
    variables: usize,
}

fn item_kind_count(item: &ContainerItem) -> io::Result<(u8, usize)> {
    if item.argument.is_some() {
        Ok((0, 1))
    } else if let Some(v) = &item.items {
        Ok((1, v.len()))
    } else if item.int.is_some() {
        Ok((2, 1))
    } else if item.bool.is_some() {
        Ok((3, 1))
    } else if item.float.is_some() {
        Ok((4, 1))
    } else if item.string.is_some() {
        Ok((5, 1))
    } else if item.w_string.is_some() {
        Ok((6, 1))
    } else if let Some(v) = &item.int_array {
        Ok((7, v.len()))
    } else if let Some(v) = &item.bool_array {
        Ok((8, v.len()))
    } else if let Some(v) = &item.float_array {
        Ok((9, v.len()))
    } else if let Some(v) = &item.string_array {
        Ok((10, v.len()))
    } else if let Some(v) = &item.w_string_array {
        Ok((11, v.len()))
    } else if item.actor_identifier.is_some() {
        Ok((12, 2))
    } else {
        Err(invalid("empty EVFL container item"))
    }
}

#[derive(Clone)]
struct RadixNode {
    name: String,
    bit: i32,
    child: [usize; 2],
    parent: usize,
}

fn radix_nodes(keys: &[&str]) -> io::Result<Vec<RadixNode>> {
    let mut nodes = vec![RadixNode {
        name: String::new(),
        bit: -1,
        child: [0, 0],
        parent: 0,
    }];
    for key in keys {
        let mut previous = 0;
        let mut current = nodes[0].child[0];
        if current != 0 {
            loop {
                previous = current;
                current = nodes[current].child[next_bit(key, nodes[current].bit)];
                if nodes[current].bit <= nodes[previous].bit {
                    break;
                }
            }
        }
        current = previous;
        let mut bit = differing_bit(key, &nodes[current].name);
        while bit < nodes[nodes[current].parent].bit {
            current = nodes[current].parent;
        }
        let index = nodes.len();
        let mut entry = RadixNode {
            name: (*key).to_owned(),
            bit,
            child: [index, index],
            parent: current,
        };
        if bit < nodes[current].bit {
            entry.parent = nodes[current].parent;
            let direction = next_bit(key, bit);
            entry.child[direction ^ 1] = current;
            let parent = nodes[current].parent;
            let parent_direction = next_bit(key, nodes[parent].bit);
            nodes[parent].child[parent_direction] = index;
            nodes[current].parent = index;
        } else if bit > nodes[current].bit {
            let direction = next_bit(key, bit);
            entry.child[direction ^ 1] = if next_bit(&nodes[current].name, bit) == direction ^ 1 {
                current
            } else {
                0
            };
            let current_direction = next_bit(key, nodes[current].bit);
            nodes[current].child[current_direction] = index;
        } else {
            let direction = next_bit(key, bit);
            bit = first_set_bit(key);
            if nodes[current].child[direction] != 0 {
                bit = differing_bit(&nodes[nodes[current].child[direction]].name, key);
            }
            entry.bit = bit;
            entry.child[next_bit(key, bit) ^ 1] = nodes[current].child[direction];
            nodes[current].child[direction] = index;
        }
        nodes.push(entry);
    }
    Ok(nodes)
}

fn next_bit(value: &str, bit: i32) -> usize {
    if value.is_empty() || bit < 0 {
        return 0;
    }
    // BfevLibrary's tree algorithm operates on .NET UTF-16 `char` values,
    // while assigning eight bit positions per character. Preserve that exact
    // behavior, including for non-ASCII entry-point and parameter names.
    let chars = value.encode_utf16().collect::<Vec<_>>();
    let byte = (bit as usize) >> 3;
    if byte >= chars.len() {
        0
    } else {
        ((chars[chars.len() - 1 - byte] >> (bit & 7)) & 1) as usize
    }
}
fn differing_bit(left: &str, right: &str) -> i32 {
    let left = left.encode_utf16().collect::<Vec<_>>();
    let right = right.encode_utf16().collect::<Vec<_>>();
    let len = left.len().max(right.len());
    let mut bit = 0;
    for index in 0..len {
        let a = left.len().checked_sub(index + 1).map_or(0, |i| left[i]);
        let b = right.len().checked_sub(index + 1).map_or(0, |i| right[i]);
        let diff = a ^ b;
        if diff == 0 {
            bit += 8;
        } else {
            bit += diff.trailing_zeros() as i32;
            break;
        }
    }
    bit
}
fn first_set_bit(value: &str) -> i32 {
    let mut bit = 0;
    for character in value.encode_utf16().collect::<Vec<_>>().into_iter().rev() {
        if character == 0 {
            bit += 8;
        } else {
            bit += character.trailing_zeros() as i32;
            break;
        }
    }
    bit
}
fn reverse_bits(value: &str) -> Vec<u8> {
    value
        .as_bytes()
        .iter()
        .rev()
        .map(|byte| byte.reverse_bits())
        .collect()
}
fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
