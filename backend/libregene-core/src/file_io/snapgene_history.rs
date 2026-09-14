//! SnapGene history decoding — Block 7 (history tree XML) + Block 11
//! (sequence snapshots), ported from GenePad
//! (https://github.com/GenePad) by the GenePad team.
//!
//! Layout facts (consistent with GenePad / sgffp on real files):
//! - Block 7: xz-compressed XML for export_version >= 15, plain XML in older
//!   files. The root node is the current state; child nodes are the inputs
//!   that produced it (the tree grows towards the past).
//! - Block 11: `u32BE node_index + u8 seq_type + payload`. seq_type 0/21/32
//!   plain (`u32BE length + ASCII`), 1 compressed DNA, 29 modifier-only
//!   (sequence reconstructed from the nested Manipulation XML undo actions).
//! - Compressed DNA (same shape as the top-level sequence block):
//!   `[u32BE cl][u32BE ul][stamp][chunk count][lowercase count]
//!   [first marker][first count][chunks][lowercase ranges]`; chunk markers:
//!   0x01 ACGT 2-bit (G=0,A=1,T=2,C=3), 0x02 IUPAC 4-bit, 0x03 N-run (no
//!   data).
//! - Node attributes on `<Node>`: ID, name, seqLen, circular ("1"),
//!   operation, plus `<InputSummary manipulation val1 val2/>` entries (one
//!   per child input; val1/val2 are 0-based inclusive).

use std::collections::BTreeMap;

use crate::file_io::dna::{features_from_xml, parse_dna_primers};
use crate::models::ProjectData;

const XZ_MAGIC: [u8; 6] = [0xFD, 0x37, 0x7A, 0x58, 0x5A, 0x00];

/// record types inside a Block 11 nested TLV area
const NESTED_FEATURES: u8 = 0x0a;
const NESTED_PRIMERS: u8 = 0x05;
const NESTED_DOC_BUNDLE: u8 = 0x1e;
const NESTED_EXTERNAL_RESIDUES: u8 = 0x1f;

use crate::models::{HistoryEntry, HistoryInputSummary, HistoryNode, SnapGeneHistoryData};

struct RawSnapshot {
    sequence: String,
    seq_type: u8,
    nested: Option<Vec<u8>>,
}

struct RawParts {
    sequence: String,
    root: HistoryNode,
    snapshots: BTreeMap<u32, RawSnapshot>,
}

// ---------------------------------------------------------------------------
// Byte helpers
// ---------------------------------------------------------------------------

fn be_u32(data: &[u8], offset: usize) -> Option<u32> {
    data.get(offset..offset + 4)
        .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn xz_decompress(data: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    lzma_rs::xz_decompress(&mut std::io::Cursor::new(data), &mut out).ok()?;
    Some(out)
}

fn is_xz(data: &[u8]) -> bool {
    data.len() >= XZ_MAGIC.len() && data[..XZ_MAGIC.len()] == XZ_MAGIC
}

// ---------------------------------------------------------------------------
// Compressed DNA (seq_type 1 payload, leading cl/ul prefix included)
// ---------------------------------------------------------------------------

const DNA_BASE_TO_BITS: [(char, u8); 4] = [('G', 0), ('A', 1), ('T', 2), ('C', 3)];

const IUPAC_TO_NIBBLE: [(char, u8); 11] = [
    ('N', 0x04),
    ('B', 0x05),
    ('D', 0x06),
    ('H', 0x07),
    ('K', 0x08),
    ('M', 0x09),
    ('R', 0x0A),
    ('S', 0x0B),
    ('V', 0x0C),
    ('W', 0x0D),
    ('Y', 0x0E),
];

fn base_from_bits(bits: u8) -> char {
    for &(b, v) in &DNA_BASE_TO_BITS {
        if v == bits {
            return b;
        }
    }
    'G'
}

fn iupac_from_nibble(nibble: u8) -> char {
    for &(b, n) in &IUPAC_TO_NIBBLE {
        if n == nibble {
            return b;
        }
    }
    'N'
}

fn octet_to_dna(data: &[u8], count: usize) -> String {
    let mut out = String::with_capacity(count);
    for i in 0..count {
        let chunk = i / 4;
        // trailing partial groups keep low-alignment (sgffp write convention)
        let chunk_len = (count - chunk * 4).min(4);
        let shift = 2 * (chunk_len - 1 - i % 4);
        out.push(base_from_bits((data[chunk] >> shift) & 0x03));
    }
    out
}

fn nibbles_to_iupac(data: &[u8], count: usize) -> String {
    let mut out = String::with_capacity(count);
    let full_pairs = count / 2;
    for i in 0..count {
        let nibble = if i / 2 < full_pairs {
            let byte = data[i / 2];
            if i % 2 == 0 {
                (byte >> 4) & 0x0F
            } else {
                byte & 0x0F
            }
        } else {
            // odd trailing char lives in the low nibble of the last byte
            data[full_pairs] & 0x0F
        };
        out.push(iupac_from_nibble(nibble));
    }
    out
}

fn decode_compressed_dna(data: &[u8]) -> Option<String> {
    if data.len() < 22 {
        return None;
    }
    let uncompressed_length = be_u32(data, 4)? as usize;
    let chunk_count = be_u32(data, 9)? as usize;
    let lowercase_count = be_u32(data, 13)? as usize;
    let first_marker = data[17];
    let first_count = be_u32(data, 18)? as usize;
    let payload = &data[22..];
    let mut pay_off = 0usize;

    let read_section = |payload: &[u8], pay_off: &mut usize, marker: u8, count: usize| -> Option<String> {
        match marker {
            0x01 => {
                let nb = count.div_ceil(4);
                if *pay_off + nb > payload.len() {
                    return None;
                }
                let s = octet_to_dna(&payload[*pay_off..*pay_off + nb], count);
                *pay_off += nb;
                Some(s)
            }
            0x02 => {
                let nb = count.div_ceil(2);
                if *pay_off + nb > payload.len() {
                    return None;
                }
                let s = nibbles_to_iupac(&payload[*pay_off..*pay_off + nb], count);
                *pay_off += nb;
                Some(s)
            }
            0x03 => Some("N".repeat(count)),
            _ => None,
        }
    };

    let mut chars: Vec<char> = Vec::new();
    if chunk_count >= 1 {
        let first = read_section(payload, &mut pay_off, first_marker, first_count)?;
        chars.extend(first.chars());
        for _ in 1..chunk_count {
            if pay_off + 5 > payload.len() {
                break;
            }
            let marker = payload[pay_off];
            let count = be_u32(payload, pay_off + 1)? as usize;
            pay_off += 5;
            chars.extend(read_section(payload, &mut pay_off, marker, count)?.chars());
        }
    }

    for _ in 0..lowercase_count {
        if pay_off + 8 > payload.len() {
            break;
        }
        let start = be_u32(payload, pay_off)? as usize;
        let end = be_u32(payload, pay_off + 4)? as usize;
        pay_off += 8;
        let limit = (end + 1).min(chars.len());
        for c in chars.iter_mut().take(limit).skip(start) {
            *c = c.to_ascii_lowercase();
        }
    }

    let sequence: String = chars.into_iter().collect();
    if uncompressed_length != 0 && sequence.chars().count() != uncompressed_length {
        return None;
    }
    Some(sequence)
}

// ---------------------------------------------------------------------------
// Block 7 tree XML (plain or xz), iterative parse
// ---------------------------------------------------------------------------

fn parse_tree(xml: &str) -> Option<HistoryNode> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<HistoryNode> = Vec::new();
    let mut root: Option<HistoryNode> = None;

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let tag = e.name();
                let tag = String::from_utf8_lossy(tag.as_ref()).to_string();
                if tag == "Node" {
                    let node = node_from_event(&e)?;
                    stack.push(node);
                } else if tag == "InputSummary" {
                    let summary = summary_from_event(&e)?;
                    if let Some(parent) = stack.last_mut() {
                        parent.input_summaries.push(summary);
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                let tag = e.name();
                let tag = String::from_utf8_lossy(tag.as_ref()).to_string();
                if tag == "InputSummary" {
                    let summary = summary_from_event(&e)?;
                    if let Some(parent) = stack.last_mut() {
                        parent.input_summaries.push(summary);
                    }
                }
                // `<Node .../>` (leaf without children) — parse and close at once
                else if tag == "Node" {
                    let node = node_from_event(&e)?;
                    close_node(&mut stack, node, &mut root);
                }
            }
            Ok(Event::End(e)) => {
                // Only `</Node>` closes a stack frame — Node elements embed
                // foreign children (<Features>, <RegeneratedSite>, …) whose
                // end tags must not pop the node stack.
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag == "Node" {
                    if let Some(node) = stack.pop() {
                        close_node(&mut stack, node, &mut root);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    if stack.is_empty() {
        root
    } else {
        None
    }
}

/// Attach a finished node to its parent (or record it as the tree root).
fn close_node(stack: &mut Vec<HistoryNode>, node: HistoryNode, root: &mut Option<HistoryNode>) {
    match stack.last_mut() {
        Some(parent) => parent.children.push(node),
        None => *root = Some(node),
    }
}

fn node_from_event(e: &quick_xml::events::BytesStart) -> Option<HistoryNode> {
    let mut node = HistoryNode::default();
    for attr in e.attributes() {
        let attr = attr.ok()?;
        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
        let value = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?.to_string();
        match key.as_str() {
            "ID" => node.id = value.parse().unwrap_or(0),
            "name" => node.name = value,
            "seqLen" => node.seq_len = value.parse().unwrap_or(0),
            "circular" => node.circular = value == "1",
            "operation" => node.operation = value,
            _ => {}
        }
    }
    Some(node)
}

fn summary_from_event(e: &quick_xml::events::BytesStart) -> Option<HistoryInputSummary> {
    let mut summary = HistoryInputSummary::default();
    for attr in e.attributes() {
        let attr = attr.ok()?;
        let key = String::from_utf8_lossy(attr.key.as_ref()).to_string();
        let value = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok()?.to_string();
        match key.as_str() {
            "manipulation" => summary.manipulation = value,
            "val1" => summary.val1 = value.parse().unwrap_or(0),
            "val2" => summary.val2 = value.parse().unwrap_or(0),
            _ => {}
        }
    }
    Some(summary)
}

// ---------------------------------------------------------------------------
// seq_type 29 reconstruction (Manipulation XML + EXTERNAL residues)
// ---------------------------------------------------------------------------

enum HistoryAction {
    Insert {
        position: usize,
        residues: ResiduesSource,
    },
    Remove {
        start: usize,
        end: usize,
    },
}

enum ResiduesSource {
    External { id: u32, length: usize },
    Literal(String),
}

/// Walk a nested TLV area (`[u8 type][u32BE length][payload]`), stopping at
/// the first out-of-bounds record.
fn walk_nested_tlv(nested: &[u8]) -> Vec<(u8, &[u8])> {
    let mut records = Vec::new();
    let mut offset = 0usize;
    while offset + 5 <= nested.len() {
        let Some(length) = be_u32(nested, offset + 1).map(|v| v as usize) else {
            break;
        };
        let start = offset + 5;
        let Some(end) = start.checked_add(length) else { break };
        if end > nested.len() {
            break;
        }
        records.push((nested[offset], &nested[start..end]));
        offset = end;
    }
    records
}

/// Split the `[u32BE xmlLen][xz Manipulation XML]` prefix off a seq_type 29
/// nested payload; returns (undo actions, remaining nested TLV bytes).
fn split_manipulation_payload(nested: &[u8]) -> Option<(Vec<HistoryAction>, &[u8])> {
    if nested.len() < 4 + XZ_MAGIC.len() {
        return None;
    }
    let xml_len = be_u32(nested, 0)? as usize;
    if xml_len == 0 || xml_len > nested.len() - 4 || !is_xz(&nested[4..4 + XZ_MAGIC.len()]) {
        return None;
    }
    let xml = xz_decompress(&nested[4..4 + xml_len])?;
    let xml = String::from_utf8(xml).ok()?;
    let actions = parse_manipulation_xml(&xml)?;
    Some((actions, &nested[4 + xml_len..]))
}

fn parse_manipulation_xml(xml: &str) -> Option<Vec<HistoryAction>> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    let mut actions = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag != "Action" {
                    continue;
                }
                let get = |name: &str| {
                    e.attributes()
                        .flatten()
                        .find(|a| String::from_utf8_lossy(a.key.as_ref()) == name)
                        .and_then(|a| a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.to_string()))
                };
                match get("type").as_deref() {
                    Some("INSERT") => {
                        let position: usize = get("position")?.parse().ok()?;
                        // child Residues element carries the bases
                        let mut residues = ResiduesSource::Literal(String::new());
                        if let Ok(Event::Start(res)) = reader.read_event() {
                            if String::from_utf8_lossy(res.name().as_ref()) == "Residues" {
                                let rget = |name: &str| {
                                    res.attributes()
                                        .flatten()
                                        .find(|a| String::from_utf8_lossy(a.key.as_ref()) == name)
                                        .and_then(|a| {
                                            a.normalized_value(quick_xml::XmlVersion::Implicit1_0).ok().map(|v| v.to_string())
                                        })
                                };
                                if rget("type").as_deref() == Some("EXTERNAL") {
                                    let id: u32 = rget("ID")?.parse().ok()?;
                                    let length: usize = rget("length")?.parse().ok()?;
                                    residues = ResiduesSource::External { id, length };
                                } else {
                                    let mut text = String::new();
                                    if let Ok(Event::Text(t)) = reader.read_event() {
                                        text = String::from_utf8_lossy(t.as_ref()).to_string();
                                    }
                                    residues = ResiduesSource::Literal(text);
                                }
                            }
                        }
                        actions.push(HistoryAction::Insert { position, residues });
                    }
                    Some("REMOVE") => {
                        let range = get("range")?;
                        let (start, end) = range.split_once('-')?;
                        actions.push(HistoryAction::Remove {
                            start: start.parse().ok()?,
                            end: end.parse().ok()?,
                        });
                    }
                    _ => return None,
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    Some(actions)
}

/// Extract EXTERNAL residues (0x1f records, 2-bit packed) for `id`.
/// Empirical record layout: `[u32BE id][head][u32BE length][packed
/// ceil(length/4) bytes][u32BE 0][u32BE length-1]` — the declared length
/// marker just before the packed bytes self-verifies the position.
fn extract_external_residues(nested: &[u8], id: u32, length: usize) -> Option<String> {
    if length == 0 {
        return Some(String::new());
    }
    let packed_len = length.div_ceil(4);
    for (record_type, data) in walk_nested_tlv(nested) {
        if record_type != NESTED_EXTERNAL_RESIDUES || data.len() < 4 + packed_len + 8 {
            continue;
        }
        if be_u32(data, 0)? != id {
            continue;
        }
        let packed_start = data.len() - 8 - packed_len;
        if be_u32(data, packed_start - 4)? as usize != length {
            continue;
        }
        let packed = &data[packed_start..packed_start + packed_len];
        let mut out = String::with_capacity(length);
        'outer: for &byte in packed {
            for slot in [byte >> 6, (byte >> 4) & 3, (byte >> 2) & 3, byte & 3] {
                if out.chars().count() >= length {
                    break 'outer;
                }
                out.push(base_from_bits(slot));
            }
        }
        return Some(out);
    }
    None
}

/// Apply a snapshot's undo actions to the parent-state sequence (0-based
/// inclusive coordinates) to recover this node's sequence.
fn apply_undo_actions(sequence: &str, actions: &[HistoryAction], nested: &[u8]) -> Option<String> {
    let mut chars: Vec<char> = sequence.chars().collect();
    for action in actions {
        match action {
            HistoryAction::Insert { position, residues } => {
                let residues: Vec<char> = match residues {
                    ResiduesSource::Literal(text) => text.chars().collect(),
                    ResiduesSource::External { id, length } => {
                        extract_external_residues(nested, *id, *length)?.chars().collect()
                    }
                };
                let position = (*position).min(chars.len());
                chars.splice(position..position, residues);
            }
            HistoryAction::Remove { start, end } => {
                if start > end || *end >= chars.len() {
                    return None;
                }
                chars.drain(*start..=*end);
            }
        }
    }
    Some(chars.into_iter().collect())
}

// ---------------------------------------------------------------------------
// Snapshot-time annotations (nested TLV / 0x1e doc bundle)
// ---------------------------------------------------------------------------

/// Decode a snapshot's nested TLV into (features XML, primers XML). Handles
/// both the classic per-record TLV and the modern xz-compressed 0x1e bundle.
fn decode_snapshot_annotation_xml(nested: &[u8]) -> (String, String) {
    let mut features_xml = String::new();
    let mut primers_xml = String::new();
    // seq_type 29 payloads lead with the Manipulation prefix — strip it
    let records = split_manipulation_payload(nested)
        .map(|(_, rest)| rest.to_vec())
        .unwrap_or_else(|| nested.to_vec());
    for (record_type, data) in walk_nested_tlv(&records) {
        match record_type {
            NESTED_FEATURES => {
                if let Ok(text) = String::from_utf8(data.to_vec()) {
                    if !text.trim().is_empty() {
                        features_xml = text;
                    }
                }
            }
            NESTED_PRIMERS => {
                if let Ok(text) = String::from_utf8(data.to_vec()) {
                    if !text.trim().is_empty() {
                        primers_xml = text;
                    }
                }
            }
            NESTED_DOC_BUNDLE => {
                let Some(docs) = xz_decompress(data) else { continue };
                for (doc_type, doc) in walk_nested_tlv(&docs) {
                    let text = String::from_utf8(doc.to_vec()).unwrap_or_default();
                    if text.trim().is_empty() {
                        continue;
                    }
                    match doc_type {
                        NESTED_FEATURES => features_xml = text,
                        NESTED_PRIMERS => primers_xml = text,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    (features_xml, primers_xml)
}

// ---------------------------------------------------------------------------
// Top-level assembly
// ---------------------------------------------------------------------------

/// Decode one Block 11 payload into (node index, raw snapshot).
fn decode_snapshot_packet(data: &[u8]) -> Option<(u32, RawSnapshot)> {
    if data.len() < 5 {
        return None;
    }
    let index = be_u32(data, 0)?;
    let seq_type = data[4];
    let body = &data[5..];
    match seq_type {
        29 => Some((
            index,
            RawSnapshot {
                sequence: String::new(),
                seq_type,
                nested: Some(body.to_vec()),
            },
        )),
        1 => {
            // leading u32 = cl (compressed segment length, excluding itself)
            let cl = be_u32(body, 0)? as usize;
            let end = 4usize.checked_add(cl)?;
            if end > body.len() {
                return None;
            }
            let sequence = decode_compressed_dna(&body[..end])?;
            Some((
                index,
                RawSnapshot {
                    sequence,
                    seq_type,
                    nested: (end < body.len()).then(|| body[end..].to_vec()),
                },
            ))
        }
        0 | 21 | 32 => {
            let n = be_u32(body, 0)? as usize;
            let end = 4usize.checked_add(n)?;
            if end > body.len() {
                return None;
            }
            let sequence = String::from_utf8(body[4..end].to_vec()).ok()?;
            Some((
                index,
                RawSnapshot {
                    sequence,
                    seq_type,
                    nested: (end < body.len()).then(|| body[end..].to_vec()),
                },
            ))
        }
        _ => None,
    }
}

/// Walk the .dna TLV blocks and decode everything history-related.
fn collect_parts(data: &[u8]) -> Option<RawParts> {
    let mut offset = 0usize;
    let mut sequence = String::new();
    let mut tree_packet: Option<Vec<u8>> = None;
    let mut snapshots: BTreeMap<u32, RawSnapshot> = BTreeMap::new();

    while offset + 5 <= data.len() {
        let block_type = data[offset];
        let Some(block_len) = be_u32(data, offset + 1).map(|v| v as usize) else {
            break;
        };
        let start = offset + 5;
        let Some(end) = start.checked_add(block_len) else { break };
        if end > data.len() {
            break;
        }
        let payload = &data[start..end];
        match block_type {
            0 | 0x15 | 0x20 => {
                // leading byte is a flags bitfield (bit 0 = circular), the
                // rest is the plain-text sequence
                let body = payload.get(1..).unwrap_or(&[]);
                sequence = String::from_utf8(body.to_vec()).ok()?;
            }
            0x07 => {
                tree_packet.get_or_insert_with(|| payload.to_vec());
            }
            0x0b => {
                if let Some((index, snapshot)) = decode_snapshot_packet(payload) {
                    snapshots.insert(index, snapshot);
                }
            }
            _ => {}
        }
        offset = end;
    }

    let tree = tree_packet?;
    let xml = if is_xz(&tree) {
        String::from_utf8(xz_decompress(&tree)?).ok()?
    } else {
        String::from_utf8(tree).ok()?
    };
    let root = parse_tree(&xml)?;
    Some(RawParts {
        sequence,
        root,
        snapshots,
    })
}

/// Resolve every node's snapshot sequence by walking the tree from the root
/// (whose state is the file's current sequence): explicit snapshot payloads
/// win, seq_type 29 nodes reconstruct from their undo actions, nodes without
/// a snapshot inherit the parent state. Unreconstructable seq_type 29 nodes
/// stay empty (the frontend shows them as not openable).
fn resolve_sequences(parts: &RawParts) -> BTreeMap<u32, String> {
    let mut resolved: BTreeMap<u32, String> = BTreeMap::new();
    let mut stack: Vec<(&HistoryNode, String)> = vec![(&parts.root, parts.sequence.clone())];
    while let Some((node, seq)) = stack.pop() {
        for child in &node.children {
            let child_seq = match parts.snapshots.get(&child.id) {
                Some(s) if !s.sequence.is_empty() => s.sequence.clone(),
                Some(s) if s.seq_type == 29 => s
                    .nested
                    .as_deref()
                    .and_then(|nested| {
                        let (actions, rest) = split_manipulation_payload(nested)?;
                        apply_undo_actions(&seq, &actions, rest)
                    })
                    .unwrap_or_default(),
                _ => seq.clone(),
            };
            resolved.insert(child.id, child_seq.clone());
            stack.push((child, child_seq));
        }
    }
    resolved
}

fn find_node<'a>(node: &'a HistoryNode, id: u32) -> Option<&'a HistoryNode> {
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if current.id == id {
            return Some(current);
        }
        stack.extend(current.children.iter());
    }
    None
}

/// Flatten the tree pre-order (root first) into list rows.
fn flatten_entries(root: &HistoryNode, sequences: &BTreeMap<u32, String>) -> Vec<HistoryEntry> {
    let mut entries = Vec::new();
    let mut stack: Vec<(&HistoryNode, usize, Option<HistoryInputSummary>)> = vec![(root, 0, None)];
    while let Some((node, depth, edge)) = stack.pop() {
        let has_snapshot = if entries.is_empty() {
            true // root = current on-disk state, always openable
        } else {
            sequences.get(&node.id).is_some_and(|s| !s.is_empty())
        };
        entries.push(HistoryEntry {
            id: node.id,
            depth,
            name: node.name.clone(),
            seq_len: node.seq_len,
            circular: node.circular,
            operation: node.operation.clone(),
            edge,
            has_snapshot,
        });
        for (idx, child) in node.children.iter().enumerate().rev() {
            let child_edge = node.input_summaries.get(idx).cloned();
            stack.push((child, depth + 1, child_edge));
        }
    }
    entries
}

/// Parse the history of a `.dna` file: tree, flattened list rows, every
/// node's resolved snapshot sequence and annotation payload. `None` when the
/// file carries no history tree.
pub fn parse_snapgene_history(data: &[u8]) -> Option<SnapGeneHistoryData> {
    let parts = collect_parts(data)?;
    let sequences = resolve_sequences(&parts);
    let entries = flatten_entries(&parts.root, &sequences);
    let nested = parts
        .snapshots
        .iter()
        .filter_map(|(id, s)| s.nested.clone().map(|n| (*id, n)))
        .collect();
    Some(SnapGeneHistoryData {
        root: parts.root,
        entries,
        sequences,
        root_sequence: parts.sequence,
        nested,
    })
}

/// Cut the subtree rooted at `node_id` into a standalone history (GenePad's
/// "open snapshot as independent document" semantics): the node becomes the
/// new root (its state = [`SnapGeneHistoryData::root_sequence`]), and the
/// snapshots of its descendants come along so the opened project's own
/// history dialog — and nested snapshot opening — stays complete.
pub fn history_subtree(
    history: &SnapGeneHistoryData,
    node_id: u32,
) -> Option<SnapGeneHistoryData> {
    let node = find_node(&history.root, node_id)?;
    let mut ids: Vec<u32> = Vec::new();
    collect_descendant_ids(node, &mut ids);

    let root_sequence = if node_id == history.root.id {
        history.root_sequence.clone()
    } else {
        history.sequences.get(&node_id)?.clone()
    };
    let sequences: BTreeMap<u32, String> = history
        .sequences
        .iter()
        .filter(|(id, _)| ids.contains(id))
        .map(|(id, s)| (*id, s.clone()))
        .collect();
    let nested: BTreeMap<u32, Vec<u8>> = history
        .nested
        .iter()
        .filter(|(id, _)| ids.contains(id))
        .map(|(id, n)| (*id, n.clone()))
        .collect();
    let root = node.clone();
    let entries = flatten_entries(&root, &sequences);
    Some(SnapGeneHistoryData {
        root,
        entries,
        sequences,
        root_sequence,
        nested,
    })
}

fn collect_descendant_ids(node: &HistoryNode, ids: &mut Vec<u32>) {
    for child in &node.children {
        ids.push(child.id);
        collect_descendant_ids(child, ids);
    }
}

/// Build a standalone project from one node of an already-parsed history
/// (sequence + snapshot-time annotations), for "open snapshot and Save As".
/// The project keeps the node's subtree history so nested snapshots travel
/// with it. Works both on file-parsed histories and on the subtree a
/// snapshot project carries in memory.
pub fn snapshot_project_from_history(
    history: &SnapGeneHistoryData,
    node_id: u32,
) -> Option<ProjectData> {
    let node = find_node(&history.root, node_id)?;
    let sequence = if node_id == history.root.id {
        history.root_sequence.clone()
    } else {
        history.sequences.get(&node_id).filter(|s| !s.is_empty()).cloned()?
    };
    if sequence.is_empty() {
        return None;
    }

    let (features_xml, primers_xml) = history
        .nested
        .get(&node_id)
        .map(|n| decode_snapshot_annotation_xml(n))
        .unwrap_or_default();
    let topology = if node.circular { "circular" } else { "linear" };
    let features = features_from_xml(&features_xml, &sequence, topology);
    let primers = parse_dna_primers(&primers_xml, &sequence);
    let subtree = history_subtree(history, node_id);

    Some(ProjectData {
        name: node.name.clone(),
        sequence,
        length: 0,
        topology: topology.to_string(),
        molecule_type: "dna".to_string(),
        features,
        primers,
        snapgene_history: subtree,
        ..Default::default()
    })
    .map(|mut p| {
        p.length = p.sequence.len() as i64;
        p
    })
}

/// Parse a `.dna` file and build a standalone project from one history
/// snapshot (sequence + snapshot-time annotations + subtree history).
pub fn snapgene_snapshot_project(data: &[u8], node_id: u32) -> Option<ProjectData> {
    let history = parse_snapgene_history(data)?;
    snapshot_project_from_history(&history, node_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_data(path: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("test_data")
            .join(path)
    }

    #[test]
    fn compressed_dna_decodes_acgt_chunks() {
        // 4 bases G(0) A(1) T(2) C(3) -> 2-bit packed 00 01 10 11 = 0x1B.
        // Header: cl, ul, stamp, chunk_count=1, lowercase_count=0,
        // first marker 0x01 + count 4, then the packed byte.
        let mut data = Vec::new();
        data.extend_from_slice(&19u32.to_be_bytes()); // cl = 14 (inner header) + 1 (packed) + 4 (ul)
        data.extend_from_slice(&4u32.to_be_bytes()); // ul
        data.extend_from_slice(&[
            30, 0, 0, 0, 1, // stamp, chunk_count
            0, 0, 0, 0, // lowercase_count
            0x01, 0, 0, 0, 4, // first marker + count
        ]);
        data.push(0x1B);
        assert_eq!(decode_compressed_dna(&data).unwrap(), "GATC");
    }

    #[test]
    fn compressed_dna_n_run() {
        let mut data = Vec::new();
        data.extend_from_slice(&4u32.to_be_bytes()); // cl (unchecked by decoder)
        data.extend_from_slice(&6u32.to_be_bytes()); // ul
        data.push(30); // stamp
        data.extend_from_slice(&1u32.to_be_bytes()); // chunk_count
        data.extend_from_slice(&0u32.to_be_bytes()); // lowercase_count
        data.push(0x03); // N-run marker
        data.extend_from_slice(&6u32.to_be_bytes()); // count = 6
        assert_eq!(decode_compressed_dna(&data).unwrap(), "NNNNNN");
    }

    #[test]
    fn history_from_real_dna_file() {
        let data = std::fs::read(test_data("BlueScribe-mEGFP.dna")).unwrap();
        let history = parse_snapgene_history(&data).expect("history parses");
        assert!(history.entries.len() > 1);
        // Root first, current on-disk state, always openable
        assert!(history.entries[0].has_snapshot);
        // Snapshot 0 declares ul = 2746 (compressed DNA payload in the file)
        let seq0 = history
            .sequences
            .get(&0)
            .expect("node 0 snapshot resolves");
        assert_eq!(seq0.len(), 2746);
        // Resolved sequences are pure DNA bases
        for (id, seq) in &history.sequences {
            assert!(
                seq.chars().all(|c| c.is_ascii_alphabetic()),
                "node {id} has non-alphabetic bases"
            );
        }
        // At least one deeper node is openable
        assert!(history.entries.iter().skip(1).any(|e| e.has_snapshot));
    }

    #[test]
    fn snapshot_project_opens_original() {
        let data = std::fs::read(test_data("BlueScribe-mEGFP.dna")).unwrap();
        let history = parse_snapgene_history(&data).unwrap();
        let target = history
            .entries
            .iter()
            .find(|e| e.depth > 0 && e.has_snapshot)
            .unwrap();
        let project = snapgene_snapshot_project(&data, target.id).unwrap();
        assert_eq!(project.sequence.len() as i64, project.length);
        assert_eq!(project.topology, if target.circular { "circular" } else { "linear" });
    }

    #[test]
    fn snapshot_project_root_is_current_sequence() {
        let data = std::fs::read(test_data("BlueScribe-mEGFP.dna")).unwrap();
        let history = parse_snapgene_history(&data).unwrap();
        let root_id = history.entries[0].id;
        let project = snapgene_snapshot_project(&data, root_id).unwrap();
        let parsed = crate::file_io::dna::parse_snapgene(&test_data("BlueScribe-mEGFP.dna")).unwrap();
        assert_eq!(project.sequence.to_uppercase(), parsed.sequence.to_uppercase());
    }

    #[test]
    fn plain_xml_tree_parses() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?><HistoryTree><Node name="a.dna" type="DNA" seqLen="99" strandedness="double" ID="0" circular="1" operation="invalid"/></HistoryTree>"#;
        let root = parse_tree(xml).unwrap();
        assert_eq!(root.id, 0);
        assert_eq!(root.seq_len, 99);
        assert!(root.circular);
        assert_eq!(root.operation, "invalid");
    }

    #[test]
    fn nested_tree_with_input_summary() {
        let xml = r#"<HistoryTree><Node ID="1" name="edit.dna" seqLen="50" circular="1" operation="insert"><InputSummary manipulation="insertAt" val1="10" val2="0"/><Node ID="0" name="orig.dna" seqLen="49" circular="1" operation="invalid"/></Node></HistoryTree>"#;
        let root = parse_tree(xml).unwrap();
        assert_eq!(root.id, 1);
        assert_eq!(root.children.len(), 1);
        assert_eq!(root.input_summaries.len(), 1);
        assert_eq!(root.input_summaries[0].manipulation, "insertAt");
        assert_eq!(root.input_summaries[0].val1, 10);
        assert_eq!(root.children[0].id, 0);
    }

    // -----------------------------------------------------------------------
    // Snapshot subtree carrying (open snapshot as independent document)
    // -----------------------------------------------------------------------

    /// root(9) -> a(1) -> b(0); a's sibling c(2). Sequences for 0/1/2.
    fn sample_history() -> SnapGeneHistoryData {
        let node = |id: u32, name: &str, children: Vec<HistoryNode>| HistoryNode {
            id,
            name: name.to_string(),
            seq_len: 9,
            circular: true,
            operation: "insert".to_string(),
            input_summaries: Vec::new(),
            children,
        };
        SnapGeneHistoryData {
            root: node(9, "current", vec![node(1, "a", vec![node(0, "b", vec![])]) , node(2, "c", vec![])]),
            entries: Vec::new(),
            sequences: BTreeMap::from([
                (0, "AAA".to_string()),
                (1, "TTT".to_string()),
                (2, "CCC".to_string()),
            ]),
            root_sequence: "GGGGGGGGG".to_string(),
            nested: BTreeMap::from([(0, vec![1, 2, 3]), (1, vec![4, 5])]),
        }
    }

    #[test]
    fn subtree_keeps_descendants_drops_siblings() {
        let history = sample_history();
        let subtree = history_subtree(&history, 1).unwrap();
        // cut node becomes the new root, keeping its name
        assert_eq!(subtree.root.id, 1);
        assert_eq!(subtree.root.name, "a");
        // root state = the cut node's snapshot sequence
        assert_eq!(subtree.root_sequence, "TTT");
        // descendants survive with their snapshots…
        assert!(subtree.sequences.contains_key(&0));
        assert!(subtree.nested.contains_key(&0));
        // …siblings don't
        assert!(!subtree.sequences.contains_key(&2));
        assert!(!subtree.nested.contains_key(&2));
        // flattened rows: root first (openable), then b
        assert_eq!(subtree.entries[0].id, 1);
        assert!(subtree.entries[0].has_snapshot);
        assert_eq!(subtree.entries[1].id, 0);
        assert!(subtree.entries[1].has_snapshot);
    }

    #[test]
    fn subtree_of_root_is_whole_history() {
        let history = sample_history();
        let subtree = history_subtree(&history, 9).unwrap();
        assert_eq!(subtree.root.id, 9);
        assert_eq!(subtree.root_sequence, "GGGGGGGGG");
        assert_eq!(subtree.sequences.len(), history.sequences.len());
    }

    #[test]
    fn project_from_subtree_opens_nested_snapshots() {
        let history = sample_history();
        let project = snapshot_project_from_history(&history, 1).unwrap();
        // sequence + name come from the snapshot
        assert_eq!(project.sequence, "TTT");
        assert_eq!(project.name, "a");
        assert_eq!(project.length, 3);
        // the project carries the subtree so nested snapshots stay openable
        let carried = project.snapgene_history.as_ref().unwrap();
        assert_eq!(carried.root.id, 1);
        let nested = snapshot_project_from_history(carried, 0).unwrap();
        assert_eq!(nested.sequence, "AAA");
        assert_eq!(nested.name, "b");
        // no snapshot sequence -> not openable
        assert!(snapshot_project_from_history(&history, 99).is_none());
    }
}
