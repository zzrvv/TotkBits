use super::{
    blackboard::{binary_size as blackboard_size, write_blackboard},
    common::{murmur3_32, AinbWriter},
    document::{computed_multi_count, AinbDocument},
    model::{Action, Attachment},
    node::{node_type_value, AinbNode},
    parameter::{
        write_property_set, InputParam, OutputParam, ParamSet, ParamSource, ParamType, Property,
        PropertySet,
    },
    plug::{plug_size, transition_from_plug, write_plug, Transition, PLUG_NAMES},
};
use std::{collections::BTreeMap, io};

#[derive(Default)]
struct WriteContext {
    command_count: u32,
    node_count: u32,
    query_count: u32,
    attachment_count: u32,
    output_count: u32,
    blackboard_offset: u32,
    string_pool_offset: u32,
    enum_resolve_offset: u32,
    property_offset: u32,
    transition_offset: u32,
    io_param_offset: u32,
    multi_param_offset: u32,
    attachment_offset: u32,
    attachment_index_offset: u32,
    expression_offset: u32,
    replacement_offset: u32,
    query_offset: u32,
    section_50_offset: u32,
    section_58_offset: u32,
    module_offset: u32,
    action_offset: u32,
    section_6c_offset: u32,
    blackboard_id_offset: u32,

    node_param_offsets: Vec<u32>,
    node_expression_counts: Vec<u16>,
    node_expression_sizes: Vec<u16>,
    node_multi_counts: Vec<u16>,
    attachment_base_indices: Vec<u32>,
    query_base_indices: Vec<u16>,
    attachment_indices: Vec<u32>,
    attachments: Vec<Attachment>,
    attachment_expression_counts: Vec<u16>,
    attachment_expression_sizes: Vec<u16>,
    properties: PropertySet,
    parameters: ParamSet,
    multi_sources: Vec<ParamSource>,
    transitions: Vec<Transition>,
    queries: Vec<u32>,
    actions: Vec<(i32, Action)>,
    expression_binary: Vec<u8>,
    expression_sizes: Vec<super::expression::ExpressionSizes>,
}

impl WriteContext {
    fn build(document: &AinbDocument) -> io::Result<Self> {
        let mut context = Self {
            command_count: document.commands.len() as u32,
            node_count: document.nodes.len() as u32,
            query_count: document
                .nodes
                .iter()
                .filter(|node| node.node_flags.iter().any(|flag| flag == "Is Query"))
                .count() as u32,
            output_count: document
                .serialization_metadata
                .output_count
                .unwrap_or_else(|| {
                    document
                        .nodes
                        .iter()
                        .filter(|node| {
                            node_type_value(&node.node_type)
                                .is_ok_and(|value| (200..300).contains(&value))
                        })
                        .count() as u32
                }),
            ..Default::default()
        };
        if let Some(expressions) = &document.expressions {
            let instance_count = expression_instance_count(document);
            (context.expression_binary, context.expression_sizes) =
                expressions.to_bytes(instance_count)?;
        }
        let node_size = if document.version > 0x404 { 0x3c } else { 0x38 };
        context.blackboard_offset =
            0x74 + context.command_count * 0x18 + context.node_count * node_size;
        let blackboard_header = if document.version >= 0x408 {
            0x38
        } else {
            0x30
        };
        let mut parameter_offset = context.blackboard_offset as usize
            + blackboard_header
            + blackboard_size(&document.blackboard, document.version);
        let query_map = document
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.node_flags.iter().any(|flag| flag == "Is Query"))
            .enumerate()
            .map(|(query_index, (node_index, _))| (node_index as u32, query_index as u32))
            .collect::<BTreeMap<_, _>>();
        let mut query_index = 0u16;
        let mut attachment_index = 0u32;
        for (node_index, node) in document.nodes.iter().enumerate() {
            context.node_param_offsets.push(parameter_offset as u32);
            let mut plug_count = 0usize;
            let mut plug_data_size = 0usize;
            for (plug_type, name) in PLUG_NAMES.into_iter().enumerate() {
                for plug in node.plugs.get(name).into_iter().flatten() {
                    plug_count += 1;
                    plug_data_size += plug_size(plug, &node.node_type, &node.name, plug_type)?;
                    if plug_type == 3 {
                        context.transitions.push(transition_from_plug(plug)?);
                    }
                }
            }
            let canonical_size = 0xa4 + plug_count * 4 + plug_data_size;
            parameter_offset += document
                .serialization_metadata
                .node_parameter_sizes
                .get(node_index)
                .copied()
                .map(|size| size as usize)
                .unwrap_or(canonical_size);

            let mut expression_count = 0u16;
            let mut expression_size = 0u16;
            let mut multi_count = 0u16;
            for kind in ParamType::ALL {
                context
                    .properties
                    .entry(kind.name().to_owned())
                    .or_default()
                    .extend(
                        node.properties
                            .get(kind.name())
                            .into_iter()
                            .flatten()
                            .cloned(),
                    );
                for property in node.properties.get(kind.name()).into_iter().flatten() {
                    if let Some(index) = property.flags.expression_index {
                        expression_count += 1;
                        expression_size += context
                            .expression_sizes
                            .get(index as usize)
                            .ok_or_else(|| invalid("property expression index exceeds EXB table"))?
                            .io;
                    }
                }
                for input in node
                    .parameters
                    .inputs
                    .get(kind.name())
                    .into_iter()
                    .flatten()
                {
                    if let Some(sources) = &input.sources {
                        for source in sources {
                            if let Some(index) = source.flags.expression_index {
                                expression_count += 1;
                                expression_size += context
                                    .expression_sizes
                                    .get(index as usize)
                                    .ok_or_else(|| {
                                        invalid("input expression index exceeds EXB table")
                                    })?
                                    .io;
                            }
                        }
                        multi_count += sources.len() as u16;
                        context.multi_sources.extend(sources.iter().cloned());
                    } else if let Some(index) = input
                        .source
                        .as_ref()
                        .and_then(|source| source.flags.expression_index)
                    {
                        expression_count += 1;
                        expression_size += context
                            .expression_sizes
                            .get(index as usize)
                            .ok_or_else(|| invalid("input expression index exceeds EXB table"))?
                            .io;
                    }
                }
                context
                    .parameters
                    .inputs
                    .entry(kind.name().to_owned())
                    .or_default()
                    .extend(
                        node.parameters
                            .inputs
                            .get(kind.name())
                            .into_iter()
                            .flatten()
                            .cloned(),
                    );
                context
                    .parameters
                    .outputs
                    .entry(kind.name().to_owned())
                    .or_default()
                    .extend(
                        node.parameters
                            .outputs
                            .get(kind.name())
                            .into_iter()
                            .flatten()
                            .cloned(),
                    );
            }
            context.node_expression_counts.push(expression_count);
            context.node_expression_sizes.push(expression_size);
            context.node_multi_counts.push(multi_count);

            if node.queries.is_empty() {
                context.query_base_indices.push(0);
            } else {
                context.query_base_indices.push(query_index);
                query_index += node.queries.len() as u16;
                for query in &node.queries {
                    context.queries.push(*query_map.get(query).ok_or_else(|| {
                        invalid(format!("node query {query} is not marked as a query node"))
                    })?);
                }
            }
            context.attachment_base_indices.push(attachment_index);
            for attachment in &node.attachments {
                attachment_index += 1;
                let index = match context
                    .attachments
                    .iter()
                    .position(|existing| existing == attachment)
                {
                    Some(index) => index,
                    None => {
                        context.attachments.push(attachment.clone());
                        context.attachments.len() - 1
                    }
                };
                context.attachment_indices.push(index as u32);
            }
            context.actions.extend(
                node.xlink_actions
                    .iter()
                    .cloned()
                    .map(|action| (node.index as i32, action)),
            );
        }
        if metadata_queries_match(document, &query_map) {
            context.query_base_indices = document
                .serialization_metadata
                .node_query_base_indices
                .clone();
            context.queries = document.serialization_metadata.query_table.clone();
        }
        let current_multi_counts = document
            .nodes
            .iter()
            .map(computed_multi_count)
            .collect::<Vec<_>>();
        if current_multi_counts == document.serialization_metadata.computed_node_multi_counts
            && document.serialization_metadata.node_multi_counts.len() == document.nodes.len()
        {
            context.node_multi_counts = document.serialization_metadata.node_multi_counts.clone();
        }
        for attachment in &context.attachments {
            let mut expression_count = 0u16;
            let mut expression_size = 0u16;
            for kind in ParamType::ALL {
                for property in attachment.properties.get(kind.name()).into_iter().flatten() {
                    if let Some(index) = property.flags.expression_index {
                        expression_count += 1;
                        expression_size += context
                            .expression_sizes
                            .get(index as usize)
                            .ok_or_else(|| {
                                invalid("attachment expression index exceeds EXB table")
                            })?
                            .io;
                    }
                    context
                        .properties
                        .entry(kind.name().to_owned())
                        .or_default()
                        .push(property.clone());
                }
            }
            context.attachment_expression_counts.push(expression_count);
            context.attachment_expression_sizes.push(expression_size);
        }
        context.attachment_index_offset = parameter_offset as u32;
        context.attachment_offset =
            context.attachment_index_offset + context.attachment_indices.len() as u32 * 4;
        context.attachment_count = context.attachments.len() as u32;
        let attachment_size = if document.version > 0x404 { 0x10 } else { 0x0c };
        let attachment_parameters =
            context.attachment_offset + attachment_size * context.attachment_count;
        context.property_offset = attachment_parameters + 0x64 * context.attachment_count;
        context.io_param_offset =
            context.property_offset + 0x18 + property_binary_size(&context.properties) as u32;
        context.multi_param_offset =
            context.io_param_offset + 0x30 + parameter_binary_size(&context.parameters) as u32;
        context.section_50_offset =
            context.multi_param_offset + context.multi_sources.len() as u32 * 8;
        context.transition_offset = context.section_50_offset;
        context.query_offset =
            context.transition_offset + transition_binary_size(&context.transitions) as u32;
        context.expression_offset = if context.expression_binary.is_empty() {
            0
        } else {
            context.query_offset + context.queries.len() as u32 * 4
        };
        context.module_offset = context.query_offset
            + context.queries.len() as u32 * 4
            + context.expression_binary.len() as u32;
        context.action_offset = context.module_offset + 4 + document.modules.len() as u32 * 0x0c;
        context.blackboard_id_offset =
            context.action_offset + 4 + context.actions.len() as u32 * 0x0c;
        context.section_58_offset = document
            .unknown_section_0x58
            .as_ref()
            .map(|_| context.blackboard_id_offset + 8)
            .unwrap_or(0);
        let replacement_start = if context.section_58_offset != 0 {
            context.section_58_offset + 0x10
        } else {
            context.blackboard_id_offset + 8
        };
        context.replacement_offset = replacement_start;
        let replacement_end = replacement_start + 8 + document.replacement_table.len() as u32 * 8;
        context.section_6c_offset = if document.has_section_0x6c {
            replacement_end
        } else {
            0
        };
        context.enum_resolve_offset = replacement_end + u32::from(document.has_section_0x6c) * 4;
        context.string_pool_offset = context.enum_resolve_offset + 4;
        Ok(context)
    }
}

pub fn write_document(document: &AinbDocument) -> io::Result<Vec<u8>> {
    let context = WriteContext::build(document)?;
    let mut writer = AinbWriter::with_strings(&document.serialization_metadata.string_pool);
    write_header(&mut writer, document, &context)?;
    for command in &document.commands {
        writer.write_string_offset(&command.name);
        writer.write_guid(&command.guid)?;
        writer.write_u16(command.root_node_index);
        writer.write_u16(
            command
                .secondary_root_node_index
                .map_or(0, |value| value + 1),
        );
    }
    for (index, node) in document.nodes.iter().enumerate() {
        write_node_header(&mut writer, document, node, index, &context)?;
    }
    write_blackboard(&mut writer, &document.blackboard, document.version)?;
    let mut property_indices = [0u32; 6];
    let mut input_indices = [0u32; 6];
    let mut output_indices = [0u32; 6];
    for (node_index, node) in document.nodes.iter().enumerate() {
        let start = writer.position();
        write_node_parameters(
            &mut writer,
            node,
            &context,
            &mut property_indices,
            &mut input_indices,
            &mut output_indices,
        )?;
        if let Some(target_size) = document
            .serialization_metadata
            .node_parameter_sizes
            .get(node_index)
            .copied()
            .map(|size| size as usize)
        {
            let target_end = start + target_size;
            if writer.position() > target_end {
                writer.truncate(target_end);
            } else if writer.position() < target_end {
                let required = target_end - writer.position();
                let tail = document
                    .serialization_metadata
                    .node_parameter_tail_bytes
                    .get(node_index)
                    .map(String::as_str)
                    .unwrap_or("");
                let decoded = decode_hex(tail)?;
                if decoded.len() >= required {
                    writer.write_bytes(&decoded[decoded.len() - required..]);
                } else {
                    writer.write_bytes(&decoded);
                    writer.write_bytes(&vec![0; required - decoded.len()]);
                }
            }
        }
    }
    for index in &context.attachment_indices {
        writer.write_u32(*index);
    }
    let mut parameter_offset = writer.position() as u32 + context.attachments.len() as u32 * 0x10;
    for (index, attachment) in context.attachments.iter().enumerate() {
        attachment.write_header(
            &mut writer,
            parameter_offset,
            context.attachment_expression_counts[index],
            context.attachment_expression_sizes[index],
            document.version,
        );
        parameter_offset += 0x64;
    }
    for attachment in &context.attachments {
        attachment.write_parameters(&mut writer, &mut property_indices);
    }
    write_property_set(&mut writer, &context.properties)?;
    context
        .parameters
        .write(&mut writer, &context.multi_sources)?;
    for source in &context.multi_sources {
        source.write(&mut writer, false)?;
    }
    let mut transition_offset = writer.position() as u32 + context.transitions.len() as u32 * 4;
    for transition in &context.transitions {
        writer.write_u32(transition_offset);
        transition_offset += if transition.transition_type == 0 {
            8
        } else {
            4
        };
    }
    for transition in &context.transitions {
        writer.write_u32(transition.transition_type | u32::from(transition.update_post_calc) << 31);
        if transition.transition_type == 0 {
            writer.write_string_offset(&transition.command_name);
        }
    }
    for query in &context.queries {
        writer.write_u16(*query as u16);
        writer.write_u16(0);
    }
    writer.write_bytes(&context.expression_binary);
    writer.write_u32(document.modules.len() as u32);
    for module in &document.modules {
        module.write(&mut writer);
    }
    writer.write_u32(context.actions.len() as u32);
    for (index, action) in &context.actions {
        writer.write_i32(*index);
        writer.write_string_offset(&action.action_slot);
        writer.write_string_offset(&action.action);
    }
    writer.write_u32(document.blackboard_id);
    writer.write_u32(document.parent_blackboard_id);
    if let Some(section) = &document.unknown_section_0x58 {
        writer.write_string_offset(&section.description);
        writer.write_u32(section.unknown04);
        writer.write_u32(section.unknown08);
        writer.write_u32(section.unknown0c);
    }
    write_replacements(&mut writer, document, &context)?;
    if document.has_section_0x6c {
        writer.write_u32(0);
    }
    writer.write_u32(0);
    if writer.position() != context.string_pool_offset as usize {
        return Err(invalid(format!(
            "AINB writer offset mismatch: expected string pool at {:#x}, reached {:#x}",
            context.string_pool_offset,
            writer.position()
        )));
    }
    writer.write_string_pool();
    Ok(writer.into_inner())
}

fn decode_hex(value: &str) -> io::Result<Vec<u8>> {
    if value.len() % 2 != 0 {
        return Err(invalid("node parameter tail has an odd hex length"));
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| invalid("node parameter tail contains invalid hex"))
        })
        .collect()
}

fn write_header(
    writer: &mut AinbWriter,
    document: &AinbDocument,
    context: &WriteContext,
) -> io::Result<()> {
    writer.write_bytes(b"AIB ");
    writer.write_u32(document.version);
    writer.write_string_offset(&document.filename);
    for value in [
        context.command_count,
        context.node_count,
        context.query_count,
        context.attachment_count,
        context.output_count,
        context.blackboard_offset,
        context.string_pool_offset,
        context.enum_resolve_offset,
        context.property_offset,
        context.transition_offset,
        context.io_param_offset,
        context.multi_param_offset,
        context.attachment_offset,
        context.attachment_index_offset,
        context.expression_offset,
        context.replacement_offset,
        context.query_offset,
        context.section_50_offset,
        0,
        context.section_58_offset,
        context.module_offset,
    ] {
        writer.write_u32(value);
    }
    writer.write_string_offset(&document.category);
    writer.write_u32(match document.category.as_str() {
        "AI" => 0,
        "Logic" => 1,
        "Sequence" => 2,
        "UniqueSequence" => 3,
        "UniqueSequenceSPL" => 4,
        other => return Err(invalid(format!("unknown AINB category {other}"))),
    });
    writer.write_u32(context.action_offset);
    writer.write_u32(context.section_6c_offset);
    writer.write_u32(context.blackboard_id_offset);
    Ok(())
}

fn write_node_header(
    writer: &mut AinbWriter,
    document: &AinbDocument,
    node: &AinbNode,
    index: usize,
    context: &WriteContext,
) -> io::Result<()> {
    writer.write_u16(node_type_value(&node.node_type)?);
    writer.write_i16(index as i16);
    writer.write_u16(node.attachments.len() as u16);
    writer.write_u8(node.flags()?);
    writer.write_u8(0);
    writer.write_string_offset(&node.name);
    if document.version > 0x404 {
        writer.write_u32(murmur3_32(&node.name));
    }
    writer.write_u32(0);
    writer.write_u32(context.node_param_offsets[index]);
    writer.write_u16(context.node_expression_counts[index]);
    writer.write_u16(context.node_expression_sizes[index]);
    writer.write_u16(context.node_multi_counts[index]);
    writer.write_u16(0);
    writer.write_u32(context.attachment_base_indices[index]);
    writer.write_u16(context.query_base_indices[index]);
    writer.write_u16(node.queries.len() as u16);
    writer.write_u32(0);
    writer.write_guid(&node.guid)
}

fn write_node_parameters(
    writer: &mut AinbWriter,
    node: &AinbNode,
    context: &WriteContext,
    property_indices: &mut [u32; 6],
    input_indices: &mut [u32; 6],
    output_indices: &mut [u32; 6],
) -> io::Result<()> {
    for (index, kind) in ParamType::ALL.into_iter().enumerate() {
        let count = node.properties.get(kind.name()).map_or(0, Vec::len) as u32;
        writer.write_u32(property_indices[index]);
        writer.write_u32(count);
        property_indices[index] += count;
    }
    for (index, kind) in ParamType::ALL.into_iter().enumerate() {
        let input_count = node.parameters.inputs.get(kind.name()).map_or(0, Vec::len) as u32;
        let output_count = node.parameters.outputs.get(kind.name()).map_or(0, Vec::len) as u32;
        writer.write_u32(input_indices[index]);
        writer.write_u32(input_count);
        writer.write_u32(output_indices[index]);
        writer.write_u32(output_count);
        input_indices[index] += input_count;
        output_indices[index] += output_count;
    }
    let mut base = 0u8;
    for name in PLUG_NAMES {
        let count = node.plugs.get(name).map_or(0, Vec::len) as u8;
        writer.write_u8(count);
        writer.write_u8(base);
        base += count;
    }
    let mut offset = writer.position() as u32 + base as u32 * 4;
    for (plug_type, name) in PLUG_NAMES.into_iter().enumerate() {
        for plug in node.plugs.get(name).into_iter().flatten() {
            writer.write_u32(offset);
            offset += plug_size(plug, &node.node_type, &node.name, plug_type)? as u32;
        }
    }
    for (plug_type, name) in PLUG_NAMES.into_iter().enumerate() {
        for plug in node.plugs.get(name).into_iter().flatten() {
            write_plug(
                writer,
                plug,
                &node.node_type,
                &node.name,
                plug_type,
                &context.transitions,
            )?;
        }
    }
    Ok(())
}

fn write_replacements(
    writer: &mut AinbWriter,
    document: &AinbDocument,
    context: &WriteContext,
) -> io::Result<()> {
    writer.write_u16(0);
    writer.write_u16(document.replacement_table.len() as u16);
    let mut has_node = false;
    let mut has_attachment = false;
    let mut nodes = context.node_count as i16;
    let mut attachments = context.attachment_count as i16;
    for replacement in &document.replacement_table {
        match replacement.kind()? {
            super::model::ReplacementType::RemoveAttachment => {
                has_attachment = true;
                attachments -= 1;
            }
            super::model::ReplacementType::RemoveChild => {
                has_node = true;
                nodes -= 1;
            }
            super::model::ReplacementType::ReplaceChild => {
                has_node = true;
                nodes -= 2;
            }
        }
    }
    writer.write_i16(if has_node { nodes } else { -1 });
    writer.write_i16(if has_attachment { attachments } else { -1 });
    for (index, replacement) in document.replacement_table.iter().enumerate() {
        replacement.write_with_raw_new_index(
            writer,
            document
                .serialization_metadata
                .replacement_new_indices
                .get(index)
                .copied(),
        )?;
    }
    Ok(())
}

fn property_binary_size(properties: &PropertySet) -> usize {
    ParamType::ALL
        .into_iter()
        .map(|kind| properties.get(kind.name()).map_or(0, Vec::len) * kind.property_size())
        .sum()
}

fn parameter_binary_size(parameters: &ParamSet) -> usize {
    ParamType::ALL
        .into_iter()
        .map(|kind| {
            parameters.inputs.get(kind.name()).map_or(0, Vec::len) * kind.input_size()
                + parameters.outputs.get(kind.name()).map_or(0, Vec::len) * kind.output_size()
        })
        .sum()
}

fn transition_binary_size(transitions: &[Transition]) -> usize {
    transitions.len() * 4
        + transitions
            .iter()
            .map(|transition| {
                if transition.transition_type == 0 {
                    8
                } else {
                    4
                }
            })
            .sum::<usize>()
}

fn expression_instance_count(document: &AinbDocument) -> u32 {
    document
        .nodes
        .iter()
        .map(|node| {
            ParamType::ALL
                .into_iter()
                .map(|kind| {
                    let properties = node
                        .properties
                        .get(kind.name())
                        .into_iter()
                        .flatten()
                        .filter(|property| property.flags.is_expression())
                        .count();
                    let inputs = node
                        .parameters
                        .inputs
                        .get(kind.name())
                        .into_iter()
                        .flatten()
                        .map(|input| {
                            input.sources.as_ref().map_or_else(
                                || {
                                    input
                                        .source
                                        .as_ref()
                                        .is_some_and(|source| source.flags.is_expression())
                                        as usize
                                },
                                |sources| {
                                    sources
                                        .iter()
                                        .filter(|source| source.flags.is_expression())
                                        .count()
                                },
                            )
                        })
                        .sum::<usize>();
                    properties + inputs
                })
                .sum::<usize>()
                + node
                    .attachments
                    .iter()
                    .map(|attachment| {
                        ParamType::ALL
                            .into_iter()
                            .map(|kind| {
                                attachment
                                    .properties
                                    .get(kind.name())
                                    .into_iter()
                                    .flatten()
                                    .filter(|property| property.flags.is_expression())
                                    .count()
                            })
                            .sum::<usize>()
                    })
                    .sum::<usize>()
        })
        .sum::<usize>() as u32
}

fn metadata_queries_match(document: &AinbDocument, query_map: &BTreeMap<u32, u32>) -> bool {
    if document
        .serialization_metadata
        .node_query_base_indices
        .len()
        != document.nodes.len()
    {
        return false;
    }
    let reverse = query_map
        .iter()
        .map(|(node, query)| (*query, *node))
        .collect::<BTreeMap<_, _>>();
    document.nodes.iter().enumerate().all(|(index, node)| {
        let base = document.serialization_metadata.node_query_base_indices[index] as usize;
        document
            .serialization_metadata
            .query_table
            .get(base..base + node.queries.len())
            .is_some_and(|raw| {
                raw.iter().map(|query| reverse.get(query).copied()).eq(node
                    .queries
                    .iter()
                    .copied()
                    .map(Some))
            })
    })
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}
