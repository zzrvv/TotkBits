use super::{
    asb::Asb,
    event::AsbEvent,
    node_tables::{BoneGroup, X38Entry, X40Entry},
    node_type::NodeType,
    x2c::X2cEntry,
};
use crate::parser::binary::BinaryWriter;
use serde_yaml::{Mapping, Value};
use std::{collections::BTreeMap, io};

struct StringPool {
    offsets: BTreeMap<String, u32>,
    bytes: Vec<u8>,
}

impl Default for StringPool {
    fn default() -> Self {
        let mut offsets = BTreeMap::new();
        offsets.insert(String::new(), 0);
        Self {
            offsets,
            bytes: vec![0],
        }
    }
}

impl StringPool {
    fn offset(&mut self, value: &str) -> u32 {
        if let Some(offset) = self.offsets.get(value) {
            return *offset;
        }
        let offset = self.bytes.len() as u32;
        self.bytes.extend_from_slice(value.as_bytes());
        self.bytes.push(0);
        self.offsets.insert(value.to_owned(), offset);
        offset
    }
}

pub struct AsbWriter<'a> {
    document: &'a Asb,
    version: u32,
    strings: StringPool,
    events: Vec<AsbEvent>,
    x2c: Vec<X2cEntry>,
    bone_groups: Vec<BoneGroup>,
    markings: Vec<Vec<String>>,
    x38: Vec<X38Entry>,
    x40: Vec<X40Entry>,
    tag_groups: Vec<Vec<String>>,
}

impl<'a> AsbWriter<'a> {
    pub fn new(document: &'a Asb) -> io::Result<Self> {
        let version = u32::from_str_radix(document.info.version.trim_start_matches("0x"), 16)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if !matches!(version, 0x40f | 0x417) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported ASB version {version:#x}"),
            ));
        }
        let mut writer = Self {
            document,
            version,
            strings: StringPool::default(),
            events: document.events.clone(),
            x2c: document.x2c_entries.clone(),
            bone_groups: document.bone_groups.clone(),
            markings: document.as_markings.clone(),
            x38: document.x38_entries.clone(),
            x40: document.x40_entries.clone(),
            tag_groups: Vec::new(),
        };
        writer.reconstruct_hidden_tables()?;
        Ok(writer)
    }

    fn reconstruct_hidden_tables(&mut self) -> io::Result<()> {
        let reconstruct_events = self.events.is_empty();
        let reconstruct_x40 = self.x40.is_empty();
        for node in self.document.nodes.values() {
            for entry in &node.x38_entries {
                if !self.x38.contains(entry) {
                    self.x38.push(entry.clone());
                }
            }
            if reconstruct_x40 {
                self.x40.extend(node.x40_entries.iter().cloned());
            }
            if let Some(tags) = &node.tags {
                push_unique(&mut self.tag_groups, tags.clone());
            }
            if let Some(marking) = &node.as_markings {
                if !self.markings.contains(marking) {
                    self.markings.push(marking.clone());
                }
            }
            let Some(Value::Mapping(body)) = &node.body else {
                continue;
            };
            if node.node_type == "Event" && reconstruct_events {
                if let Some(value) = get(body, "Event") {
                    self.events
                        .push(serde_yaml::from_value(value.clone()).map_err(io::Error::other)?);
                }
            }
            if node.node_type == "PreviousTagSelector" {
                if let Some(Value::Sequence(children)) = get(body, "Child Nodes") {
                    for child in children {
                        if let Some(tags) = child
                            .as_mapping()
                            .and_then(|m| get(m, "Tags"))
                            .and_then(Value::as_sequence)
                        {
                            push_unique(&mut self.tag_groups, strings(tags));
                        }
                    }
                }
            }
            if node.node_type == "InitialFrame" {
                if let Some(tags) = get(body, "Tags").and_then(Value::as_sequence) {
                    push_unique(&mut self.tag_groups, strings(tags));
                }
            }
            if node.node_type == "BoneBlender" {
                if let Some(value) = get(body, "Bone Group") {
                    let group: BoneGroup =
                        serde_yaml::from_value(value.clone()).map_err(io::Error::other)?;
                    if !self.bone_groups.contains(&group) {
                        self.bone_groups.push(group);
                    }
                }
            }
            if let Some(Value::Sequence(entries)) = get(body, "0x2C Connections") {
                for connection in entries {
                    let Some(map) = connection.as_mapping() else {
                        continue;
                    };
                    let Some(value) = get(map, "0x2C Entry") else {
                        continue;
                    };
                    if value.as_mapping().is_some_and(Mapping::is_empty) {
                        continue;
                    }
                    let entry: X2cEntry =
                        serde_yaml::from_value(value.clone()).map_err(io::Error::other)?;
                    if !self.x2c.contains(&entry) {
                        self.x2c.push(entry);
                    }
                }
            }
        }
        for command in &self.document.commands {
            if let Some(tags) = &command.tags {
                push_unique(&mut self.tag_groups, tags.clone());
            }
        }
        Ok(())
    }

    pub fn write(mut self) -> io::Result<Vec<u8>> {
        let mut w = BinaryWriter::new();
        w.write_bytes(b"ASB ");
        w.write_u32(self.version);
        w.write_u32(self.strings.offset(&self.document.info.filename));
        w.write_u32(self.document.commands.len() as u32);
        w.write_u32(self.document.nodes.len() as u32);
        w.write_u32(self.events.len() as u32);
        w.write_u32(self.document.animation_slots.len() as u32);
        w.write_u32(self.x38.len() as u32);
        let header_patches = reserve_u32(&mut w, if self.version == 0x417 { 19 } else { 18 });
        let mut tag_patches: Vec<(usize, Vec<String>)> = Vec::new();

        for command in &self.document.commands {
            w.write_u32(self.strings.offset(&command.name));
            write_tag_ref(&mut w, command.tags.as_ref(), &mut tag_patches);
            write_parameter(&mut w, &mut self.strings, &command.unknown_1)?;
            write_parameter(&mut w, &mut self.strings, &command.unknown_2)?;
            w.write_u32(command.unknown_3);
            write_guid(&mut w, &command.guid)?;
            w.write_u16(command.left_node_index);
            w.write_u16((command.right_node_index + 1) as u16);
        }

        let mut body_patches = Vec::new();
        let mut x40_index = 0u16;
        let mut x38_index = 0u16;
        for node in self.document.nodes.values() {
            w.write_u16(NodeType::from_name(&node.node_type)?.as_u16());
            w.write_u8(node.x38_entries.len() as u8);
            w.write_u8(node.unknown);
            write_tag_ref(&mut w, node.tags.as_ref(), &mut tag_patches);
            body_patches.push(reserve_one(&mut w));
            w.write_u16(x40_index);
            w.write_u16(node.x40_entries.len() as u16);
            x40_index += node.x40_entries.len() as u16;
            w.write_u16(x38_index);
            x38_index += node.x38_entries.len() as u16;
            let marking = node
                .as_markings
                .as_ref()
                .and_then(|v| self.markings.iter().position(|x| x == v))
                .map_or(0, |i| i as u16 + 1);
            w.write_u16(marking);
            write_guid(&mut w, &node.guid)?;
        }
        let event_offsets_pos = w.position();
        let event_offset_patches = reserve_u32(&mut w, self.events.len());
        patch(&mut w, header_patches[4], event_offsets_pos as u32);

        for ((_, node), at) in self.document.nodes.iter().zip(body_patches) {
            let position = w.position() as u32;
            patch(&mut w, at, position);
            if let Some(Value::Mapping(body)) = &node.body {
                self.write_body(&mut w, &node.node_type, body, &mut tag_patches)?;
            }
        }

        let x38_index_offset = w.position();
        for node in self.document.nodes.values() {
            for entry in &node.x38_entries {
                w.write_u32(index_of(&self.x38, entry)? as u32);
            }
        }
        let x38_offset = w.position();
        let values_start = x38_offset + 0x18 * self.x38.len();
        let mut value_at = values_start as u32;
        for entry in &self.x38 {
            w.write_u32(entry.kind);
            w.write_u32(value_at);
            write_guid(&mut w, &entry.guid)?;
            value_at += match entry.kind {
                0 => 12,
                1 => 24,
                _ => 0,
            };
        }
        for entry in &self.x38 {
            if !matches!(entry.kind, 0 | 1) {
                continue;
            }
            let m = mapping(&entry.entry)?;
            write_parameter(&mut w, &mut self.strings, required(m, "Start Frame")?)?;
            if entry.kind == 0 {
                w.write_u32(as_u32(required(m, "Unknown 2")?)?);
            } else {
                write_parameter(&mut w, &mut self.strings, required(m, "End Frame")?)?;
                write_parameter(&mut w, &mut self.strings, required(m, "Unknown 3")?)?;
            }
        }

        let x2c_offset = w.position();
        w.write_u32(self.x2c.len() as u32);
        for entry in &self.x2c {
            w.write_u16(entry.source_node);
            w.write_u16(entry.target_node);
            w.write_u32(entry.unknown_1);
            w.write_u32(entry.unknown_2);
            w.write_u32(entry.unknown_3);
            for sub in &entry.entries {
                w.write_u16(sub.entry_type);
                w.write_u16(sub.unknown_type);
                if sub.entry_type == 0 {
                    w.write_bytes(&[0; 16]);
                } else {
                    write_parameter(
                        &mut w,
                        &mut self.strings,
                        sub.unknown_1.as_ref().unwrap_or(&Value::Null),
                    )?;
                    write_parameter(
                        &mut w,
                        &mut self.strings,
                        sub.unknown_2.as_ref().unwrap_or(&Value::Null),
                    )?;
                }
            }
        }

        for (event, at) in self.events.clone().iter().zip(event_offset_patches) {
            let position = w.position() as u32;
            patch(&mut w, at, position);
            self.write_event(&mut w, event)?;
        }
        let (transitions_offset, command_groups_offset) = self.write_transitions(&mut w)?;
        let blackboard_offset = self.write_blackboard(&mut w)?;
        let slots_offset = self.write_slots(&mut w);
        let bones_offset = self.write_bones(&mut w);
        let x40_offset = w.position();
        for entry in &self.x40 {
            w.write_u32(entry.unknown_1);
            w.write_f32(entry.angle as f32);
            w.write_u32(entry.kind.unwrap_or(0));
            w.write_f32(entry.unknown_2 as f32);
            w.write_f32(entry.rate as f32);
            w.write_f32(entry.unknown_3 as f32);
            w.write_f32(entry.min as f32);
            w.write_f32(entry.max as f32);
        }
        let tag_list_offset = w.position();
        w.write_u32(self.document.valid_tag_list.len() as u32);
        for tag in &self.document.valid_tag_list {
            w.write_u32(self.strings.offset(tag));
        }
        for tags in &self.tag_groups {
            let at = w.position() as u32;
            for (patch_at, wanted) in &tag_patches {
                if wanted == tags {
                    patch(&mut w, *patch_at, at);
                }
            }
            w.write_u32(tags.len() as u32);
            for tag in tags {
                w.write_u32(self.strings.offset(tag));
            }
        }
        let exb_offset = if let Some(exb) = &self.document.exb {
            let at = w.position() as u32;
            w.write_bytes(&exb.to_bytes(0)?);
            at
        } else {
            0
        };
        let markings_offset = w.position();
        w.write_u32(self.markings.len() as u32);
        for group in &self.markings {
            for value in group {
                w.write_u32(self.strings.offset(value));
            }
        }
        let x68_offset = w.position();
        if self.version == 0x417 {
            w.write_u32(self.document.x68_section.len() as u32);
            for entry in &self.document.x68_section {
                w.write_u32(self.strings.offset(&entry.name));
                w.write_f32(entry.unknown as f32);
            }
        }
        let enum_offset = w.position();
        w.write_u32(0);
        let strings_offset = w.position();
        w.write_bytes(&self.strings.bytes);

        let values = [
            blackboard_offset,
            strings_offset as u32,
            enum_offset as u32,
            x2c_offset as u32,
            event_offsets_pos as u32,
            slots_offset,
            x38_offset as u32,
            x38_index_offset as u32,
            x40_offset as u32,
            self.x40.len() as u32,
            bones_offset,
            self.bone_groups.len() as u32,
            self.strings.bytes.len() as u32,
            transitions_offset,
            tag_list_offset as u32,
            markings_offset as u32,
            exb_offset,
            command_groups_offset,
        ];
        for (at, value) in header_patches.iter().take(18).zip(values) {
            patch(&mut w, *at, value);
        }
        if self.version == 0x417 {
            patch(&mut w, header_patches[18], x68_offset as u32);
        }
        Ok(w.into_inner())
    }

    fn write_body(
        &mut self,
        w: &mut BinaryWriter,
        kind: &str,
        b: &Mapping,
        tags: &mut Vec<(usize, Vec<String>)>,
    ) -> io::Result<()> {
        macro_rules! p {
            ($k:literal) => {
                write_parameter(w, &mut self.strings, required(b, $k)?)?
            };
        }
        macro_rules! u {
            ($k:literal) => {
                w.write_u32(as_u32(required(b, $k)?)?)
            };
        }
        match kind {
            "FloatSelector" | "StringSelector" | "IntSelector" | "BoolSelector" => {
                p!("Parameter");
                p!("Unknown 1");
                w.write_u32(as_bool(required(b, "Unknown 2")?)? as u32);
            }
            "SkeletalAnimation" => {
                p!("Animation");
                u!("Unknown 1");
                u!("Unknown 2");
                p!("Unknown 3");
                p!("Unknown 4");
            }
            "State" | "Unknown2" | "Unknown4" | "SubtractAnimation" | "Unknown7" => {}
            "OneDimensionalBlender" => {
                p!("Parameter");
                u!("Unknown");
            }
            "Sequential" => {
                p!("Unknown 1");
                p!("Unknown 2");
                p!("Unknown 3");
            }
            "Simultaneous" => u!("Unknown"),
            "Event" => {
                let event = required(b, "Event")?;
                let parsed: AsbEvent =
                    serde_yaml::from_value(event.clone()).map_err(io::Error::other)?;
                w.write_u32(index_of(&self.events, &parsed)? as u32);
            }
            "MaterialAnimation" => {
                u!("Unknown 1");
                p!("Animation");
                p!("Unknown 2");
            }
            "FrameController" => {
                for k in ["Animation Rate", "Start Frame", "End Frame"] {
                    write_parameter(w, &mut self.strings, required(b, k)?)?
                }
                u!("Unknown Flag");
                for k in [
                    "Loop Cancel Flag",
                    "Unknown 2",
                    "Unknown 3",
                    "Unknown 4",
                    "Unknown 5",
                    "Unknown 6",
                    "Unknown 7",
                    "Unknown 8",
                ] {
                    write_parameter(w, &mut self.strings, required(b, k)?)?
                }
                w.write_u32(as_bool(required(b, "Unknown 9")?)? as u32);
                p!("Unknown 10");
                if self.version == 0x417 {
                    p!("Unknown 11");
                }
                u!("Unknown 12");
                u!("Unknown 13");
            }
            "DummyAnimation" => {
                p!("Frame");
                p!("Unknown");
            }
            "RandomSelector" => {
                u!("Unknown 1");
                p!("Unknown 2");
                p!("Unknown 3");
                w.write_u32(as_bool(required(b, "Unknown 4")?)? as u32);
            }
            "PreviousTagSelector" => u!("Unknown"),
            "BonePositionSelector" => {
                p!("Bone 1");
                p!("Bone 2");
                u!("Unknown 1");
                u!("Unknown 2");
                p!("Unknown 3");
            }
            "BoneAnimation" => {
                p!("Animation");
                p!("Unknown 1");
                u!("Unknown 2");
                u!("Unknown 3");
                p!("Unknown 4");
            }
            "InitialFrame" => {
                u!("Flag");
                let ts = get(b, "Tags")
                    .and_then(Value::as_sequence)
                    .map(|v| strings(v));
                if let Some(ts) = ts {
                    let at = reserve_one(w);
                    tags.push((at, ts));
                } else {
                    w.write_u32(0);
                }
                p!("Unknown 1");
                p!("Bone 1");
                p!("Bone 2");
                u!("Unknown 2");
                p!("Unknown 3");
                p!("Unknown 4");
            }
            "BoneBlender" => {
                let group: BoneGroup = serde_yaml::from_value(required(b, "Bone Group")?.clone())
                    .map_err(io::Error::other)?;
                write_parameter(w, &mut self.strings, &Value::String(group.name))?;
                u!("Unknown 1");
                p!("Unknown 2");
                u!("Unknown 3");
                u!("Unknown 4");
            }
            "Alert" => p!("Message"),
            "ShapeAnimation" => p!("Animation"),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("unsupported node body {kind}"),
                ))
            }
        }
        if !matches!(kind, "Unknown2" | "Unknown4") {
            self.write_connections(w, b, kind, tags)?;
        }
        Ok(())
    }

    fn write_connections(
        &mut self,
        w: &mut BinaryWriter,
        b: &Mapping,
        kind: &str,
        tags: &mut Vec<(usize, Vec<String>)>,
    ) -> io::Result<()> {
        let keys = [
            "State Nodes",
            "Unknown Connection",
            "Child Nodes",
            "0x2C Connections",
            "Event Node Connections",
            "Frame Node Connections",
        ];
        let lists: Vec<Vec<Value>> = keys
            .iter()
            .map(|k| {
                get(b, k)
                    .and_then(Value::as_sequence)
                    .cloned()
                    .unwrap_or_default()
            })
            .collect();
        let mut base = 0u8;
        for list in &lists {
            w.write_u8(list.len() as u8);
            w.write_u8(base);
            base = base.wrapping_add(list.len() as u8);
        }
        let mut patches: Vec<Vec<usize>> = Vec::new();
        for list in &lists {
            patches.push(reserve_u32(w, list.len()));
        }
        for (group, list) in lists.iter().enumerate() {
            for (value, at) in list.iter().zip(&patches[group]) {
                patch(w, *at, w.position() as u32);
                match group {
                    0 | 4 | 5 => w.write_u32(as_u32(value)?),
                    2 => self.write_child(w, kind, value, tags)?,
                    3 => {
                        if self.version == 0x417 {
                            let m = mapping(value)?;
                            let e = required(m, "0x2C Entry")?;
                            if e.as_mapping().is_some_and(Mapping::is_empty) {
                                w.write_i32(-1)
                            } else {
                                let x: X2cEntry =
                                    serde_yaml::from_value(e.clone()).map_err(io::Error::other)?;
                                w.write_u32(index_of(&self.x2c, &x)? as u32)
                            }
                            w.write_u32(as_u32(required(m, "Node Index")?)?);
                        } else {
                            w.write_u32(as_u32(value)?);
                        }
                    }
                    _ => w.write_u32(as_u32(value)?),
                }
            }
        }
        Ok(())
    }

    fn write_child(
        &mut self,
        w: &mut BinaryWriter,
        kind: &str,
        value: &Value,
        tags: &mut Vec<(usize, Vec<String>)>,
    ) -> io::Result<()> {
        if let Some(i) = value.as_u64() {
            w.write_u32(i as u32);
            return Ok(());
        }
        let m = mapping(value)?;
        match kind {
            "FloatSelector" | "BonePositionSelector" | "OneDimensionalBlender" => {
                if let Some(v) = get(m, "Default Condition") {
                    write_parameter(w, &mut self.strings, v)?;
                    w.write_u64(0);
                    w.write_u32(as_u32(required(m, "Node Index")?)?)
                } else {
                    write_parameter(w, &mut self.strings, required(m, "Condition Min")?)?;
                    write_parameter(w, &mut self.strings, required(m, "Condition Max")?)?;
                    w.write_u32(as_u32(required(m, "Node Index")?)?)
                }
            }
            "RandomSelector" => {
                write_parameter(w, &mut self.strings, required(m, "Weight")?)?;
                w.write_u32(as_u32(required(m, "Node Index")?)?)
            }
            "IntSelector" | "StringSelector" => {
                let v = get(m, "Condition")
                    .or_else(|| get(m, "Default Condition"))
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "selector condition missing")
                    })?;
                write_parameter(w, &mut self.strings, v)?;
                w.write_u32(as_u32(required(m, "Node Index")?)?)
            }
            "PreviousTagSelector" => {
                let ts = required(m, "Tags")?
                    .as_sequence()
                    .map(|v| strings(v))
                    .unwrap_or_default();
                if ts.is_empty() {
                    w.write_i32(-1)
                } else {
                    let at = reserve_one(w);
                    tags.push((at, ts));
                }
                w.write_u32(as_u32(required(m, "Node Index")?)?)
            }
            "BoolSelector" => w.write_u32(as_u32(
                get(m, "Condition True")
                    .or_else(|| get(m, "Condition False"))
                    .ok_or_else(|| {
                        io::Error::new(io::ErrorKind::InvalidData, "bool selector child missing")
                    })?,
            )?),
            _ => w.write_u32(as_u32(value)?),
        }
        Ok(())
    }

    fn write_event(&mut self, w: &mut BinaryWriter, event: &AsbEvent) -> io::Result<()> {
        w.write_u32(event.trigger_events.len() as u32);
        w.write_u32(event.hold_events.len() as u32);
        let mut param_patches = Vec::new();
        for e in &event.trigger_events {
            w.write_u32(self.strings.offset(&e.name));
            w.write_u32(e.unknown_1);
            param_patches.push(reserve_one(w));
            w.write_u32((e.parameters.len() * 8) as u32);
            w.write_u32(parse_hex(&e.unknown_hash)?);
            w.write_f32(e.start_frame as f32);
        }
        for e in &event.hold_events {
            w.write_u32(self.strings.offset(&e.name));
            w.write_u32(e.unknown_1);
            param_patches.push(reserve_one(w));
            w.write_u32((e.parameters.len() * 8) as u32);
            w.write_u32(parse_hex(&e.unknown_hash)?);
            w.write_f32(e.start_frame as f32);
            w.write_f32(e.end_frame as f32);
        }
        let lists = event
            .trigger_events
            .iter()
            .map(|e| &e.parameters)
            .chain(event.hold_events.iter().map(|e| &e.parameters));
        let mut value_patches = Vec::new();
        for (params, at) in lists.clone().zip(param_patches) {
            patch(w, at, w.position() as u32);
            w.write_u32(params.len() as u32);
            for p in params {
                let flag = match p {
                    Value::Bool(_) => 0x10,
                    Value::Number(n) if n.is_i64() || n.is_u64() => 0x20,
                    Value::Number(_) => 0x30,
                    Value::String(_) => 0x40,
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid event parameter",
                        ))
                    }
                };
                let at = reserve_one(w);
                value_patches.push((at, flag, p));
            }
        }
        for (at, flag, p) in value_patches {
            let target = w.position() as u32;
            patch(w, at, target | (flag << 24));
            write_parameter(w, &mut self.strings, p)?;
        }
        Ok(())
    }

    fn write_transitions(&mut self, w: &mut BinaryWriter) -> io::Result<(u32, u32)> {
        let start = w.position() as u32;
        w.write_u32(self.document.transitions.len() as u32);
        w.write_u32(0);
        let mut entry_patches = Vec::new();
        for t in &self.document.transitions {
            w.write_u32(t.entries.len() as u32);
            w.write_i32(t.unknown);
            entry_patches.push(reserve_one(w));
        }
        let mut groups: Vec<Vec<String>> = Vec::new();
        for (t, at) in self.document.transitions.iter().zip(entry_patches) {
            patch(w, at, w.position() as u32);
            for e in &t.entries {
                w.write_u32(self.strings.offset(&e.command_1));
                w.write_u32(self.strings.offset(&e.command_2));
                w.write_u8(match e.parameter_type.as_str() {
                    "int" => 0,
                    "string" => 1,
                    "float" => 2,
                    "bool" => 3,
                    "vec3f" => 4,
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid transition type",
                        ))
                    }
                });
                w.write_u8(e.allow_multiple_matches as u8);
                let gi = if let Some(g) = &e.command_group {
                    push_unique(&mut groups, g.clone());
                    groups.iter().position(|x| x == g).ok_or_else(|| {
                        io::Error::other("transition command group was not retained")
                    })? as u16
                        + 1
                } else {
                    0
                };
                w.write_u16(gi);
                w.write_u32(self.strings.offset(&e.parameter));
                write_parameter(w, &mut self.strings, &e.value)?;
                if e.parameter_type != "vec3f" {
                    w.write_u64(0);
                }
            }
        }
        let groups_at = if groups.is_empty() {
            0
        } else {
            let s = w.position() as u32;
            w.write_u32(groups.len() as u32);
            let ps = reserve_u32(w, groups.len());
            for (g, at) in groups.iter().zip(ps) {
                patch(w, at, w.position() as u32);
                w.write_u32(g.len() as u32);
                for x in g {
                    w.write_u32(self.strings.offset(x));
                }
            }
            s
        };
        Ok((start, groups_at))
    }

    fn write_blackboard(&mut self, w: &mut BinaryWriter) -> io::Result<u32> {
        let start = w.position() as u32;
        let kinds = ["string", "int", "float", "bool", "vec3f", "userdefined"];
        let mut index = 0u16;
        let mut off = 0u16;
        for kind in kinds {
            let group = self.document.local_blackboard_parameters.0.get(kind);
            let n = group.map_or(0, Vec::len) as u16;
            w.write_u16(n);
            w.write_u16(index);
            index += n;
            w.write_u16(off);
            off += n * if kind == "vec3f" { 12 } else { 4 };
            w.write_u16(0);
        }
        let mut refs: Vec<String> = Vec::new();
        for kind in kinds {
            if let Some(group) = self.document.local_blackboard_parameters.0.get(kind) {
                for e in group {
                    let mut v = self.strings.offset(&e.name);
                    if let Some(f) = &e.file_reference {
                        push_unique(&mut refs, f.filename.clone());
                        let reference_index =
                            refs.iter().position(|x| x == &f.filename).ok_or_else(|| {
                                io::Error::other("blackboard reference was not retained")
                            })?;
                        v |= 0x8000_0000 | ((reference_index as u32) << 24);
                    }
                    w.write_u32(v);
                }
            }
        }
        for kind in kinds {
            if let Some(group) = self.document.local_blackboard_parameters.0.get(kind) {
                for e in group {
                    match kind {
                        "string" => {
                            w.write_u32(self.strings.offset(e.init_value.as_str().unwrap_or("")))
                        }
                        "int" => w.write_u32(as_u32(&e.init_value)?),
                        "float" => w.write_f32(e.init_value.as_f64().unwrap_or(0.0) as f32),
                        "bool" => w.write_u32(e.init_value.as_bool().unwrap_or(false) as u32),
                        "vec3f" => {
                            if let Some(values) = e.init_value.as_sequence() {
                                for x in values {
                                    w.write_f32(x.as_f64().unwrap_or(0.0) as f32)
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        for f in refs {
            w.write_u32(self.strings.offset(&f));
            w.write_bytes(&[0; 12]);
        }
        Ok(start)
    }

    fn write_slots(&mut self, w: &mut BinaryWriter) -> u32 {
        let start = w.position() as u32;
        for e in &self.document.animation_slots {
            w.write_u16(e.entries.len() as u16);
            w.write_u16(e.unknown);
            w.write_u32(self.strings.offset(&e.partial_1));
            w.write_u32(self.strings.offset(&e.partial_2));
            for x in &e.entries {
                w.write_u32(self.strings.offset(&x.bone));
                w.write_u16(x.unknown_1);
                w.write_u16(x.unknown_2);
            }
        }
        start
    }
    fn write_bones(&mut self, w: &mut BinaryWriter) -> u32 {
        let start = w.position() as u32;
        let ps: Vec<_> = self
            .bone_groups
            .iter()
            .map(|g| {
                let p = reserve_one(w);
                w.write_u32(self.strings.offset(&g.name));
                w.write_u32(g.bones.len() as u32);
                w.write_u32(g.unknown);
                p
            })
            .collect();
        for (g, p) in self.bone_groups.iter().zip(ps) {
            patch(w, p, w.position() as u32);
            for b in &g.bones {
                w.write_u32(self.strings.offset(&b.name));
                w.write_f32(b.unknown as f32);
            }
        }
        start
    }
}

fn get<'a>(map: &'a Mapping, key: &str) -> Option<&'a Value> {
    map.get(Value::String(key.to_owned()))
}

fn strings(values: &[Value]) -> Vec<String> {
    values
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn push_unique<T: PartialEq>(values: &mut Vec<T>, value: T) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn reserve_one(w: &mut BinaryWriter) -> usize {
    let at = w.position();
    w.write_u32(0);
    at
}
fn reserve_u32(w: &mut BinaryWriter, count: usize) -> Vec<usize> {
    (0..count).map(|_| reserve_one(w)).collect()
}
fn patch(w: &mut BinaryWriter, at: usize, value: u32) {
    let ret = w.position();
    w.seek(at);
    w.write_u32(value);
    w.seek(ret);
}
fn write_tag_ref(
    w: &mut BinaryWriter,
    tags: Option<&Vec<String>>,
    patches: &mut Vec<(usize, Vec<String>)>,
) {
    if let Some(tags) = tags {
        let at = reserve_one(w);
        patches.push((at, tags.clone()));
    } else {
        w.write_u32(0);
    }
}
fn mapping(value: &Value) -> io::Result<&Mapping> {
    value
        .as_mapping()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "expected YAML mapping"))
}
fn required<'a>(map: &'a Mapping, key: &str) -> io::Result<&'a Value> {
    get(map, key).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("missing ASB field {key}"),
        )
    })
}
fn as_u32(v: &Value) -> io::Result<u32> {
    v.as_u64()
        .map(|x| x as u32)
        .or_else(|| v.as_i64().map(|x| x as u32))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "expected integer"))
}
fn as_bool(v: &Value) -> io::Result<bool> {
    v.as_bool()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "expected bool"))
}
fn write_guid(w: &mut BinaryWriter, value: &str) -> io::Result<()> {
    let p: Vec<_> = value.split('-').collect();
    if p.len() != 5
        || p[0].len() != 8
        || p[1].len() != 4
        || p[2].len() != 4
        || p[3].len() != 4
        || p[4].len() != 12
        || !p
            .iter()
            .all(|part| part.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "invalid GUID"));
    }
    w.write_u32(u32::from_str_radix(p[0], 16).map_err(io::Error::other)?);
    w.write_u16(u16::from_str_radix(p[1], 16).map_err(io::Error::other)?);
    w.write_u16(u16::from_str_radix(p[2], 16).map_err(io::Error::other)?);
    w.write_u16(u16::from_str_radix(p[3], 16).map_err(io::Error::other)?);
    let bytes = (0..p[4].len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&p[4][i..i + 2], 16).map_err(io::Error::other))
        .collect::<io::Result<Vec<_>>>()?;
    if bytes.len() != 6 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid GUID tail",
        ));
    }
    w.write_bytes(&bytes);
    Ok(())
}
fn write_parameter(w: &mut BinaryWriter, pool: &mut StringPool, value: &Value) -> io::Result<()> {
    if let Some(m) = value.as_mapping() {
        let flags = if let Some(s) = get(m, "Flags").and_then(Value::as_str) {
            u32::from_str_radix(s.trim_start_matches("0x"), 16).map_err(io::Error::other)? << 16
        } else {
            0x8100_0000
        };
        let index = get(m, "Index")
            .or_else(|| get(m, "Local Blackboard Index"))
            .or_else(|| get(m, "EXB Index"))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "parameter index missing"))?;
        w.write_u32(flags | as_u32(index)? & 0xffff);
        write_parameter_value(w, pool, get(m, "Default Value").unwrap_or(&Value::Null))
    } else {
        w.write_u32(0);
        write_parameter_value(w, pool, value)
    }
}
fn write_parameter_value(
    w: &mut BinaryWriter,
    pool: &mut StringPool,
    value: &Value,
) -> io::Result<()> {
    match value {
        Value::Null => w.write_u32(0),
        Value::Bool(v) => w.write_u32(*v as u32),
        Value::Number(v) => {
            if let Some(i) = v.as_i64() {
                w.write_i32(i as i32)
            } else {
                w.write_f32(v.as_f64().unwrap_or(0.0) as f32)
            }
        }
        Value::String(v) => w.write_u32(pool.offset(v)),
        Value::Sequence(v) => {
            for x in v {
                w.write_f32(x.as_f64().unwrap_or(0.0) as f32)
            }
        }
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid parameter value",
            ))
        }
    };
    Ok(())
}
fn index_of<T: PartialEq>(values: &[T], wanted: &T) -> io::Result<usize> {
    values
        .iter()
        .position(|x| x == wanted)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "ASB table entry missing"))
}
fn parse_hex(value: &str) -> io::Result<u32> {
    u32::from_str_radix(value.trim_start_matches("0x"), 16).map_err(io::Error::other)
}
