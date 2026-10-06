//! Shared MCP helpers: coordinate conversion, the uniform response envelope,
//! naming conventions, 1-based JSON serializers.
//!
//! Response envelope (every tool, success or domain failure):
//! `{ok, message, projectId?, unit?, sequenceHash?, revCompHash?, text?, ...}`
//! - `ok` — true on success, false when the domain rejected the request.
//! - `message` — one-line human-readable summary (always present).
//! - `projectId` — the addressed project (omitted by list_projects; per item
//!   in convert_sequence).
//! - `unit` — "bp" | "nt" | "aa": the molecule's length unit, present on every
//!   response that reports lengths of a project molecule.
//! - `text` — human-readable rendering: a compact multi-section digest for
//!   overview/region/mutation responses, a coordinate-ruled window for
//!   read_sequence. `textBefore` is the pre-edit digest.
//! - `warnings` — strings flagging something possibly wrong; `notes` —
//!   informational strings. Both are arrays, present only when non-empty.
//!
//! Naming conventions:
//! - camelCase fields; 1-based inclusive coordinates everywhere.
//! - Spans are `start`/`end`; a single coordinate is `position`; an offset
//!   relative to a feature/segment is `offset`.
//! - Lengths end in `Length`, counts in `Count`, detail lists in `Details`.
//! - Booleans are plain assertions (`binds`, `unique`, `significant`).
//! - Fractions (identity, CAI) are 0–1; percentages (`gcPercent`) are 0–100
//!   with one decimal; `tm` is °C rounded to 0.1.

use libregene_core::models::{Feature, PrimerBindingSite};

// ---------------------------------------------------------------------------
// Coordinate conversion: the MCP interface is 1-based inclusive, the internal
// model 0-based inclusive. All boundary crossings go through these helpers.
// ---------------------------------------------------------------------------

/// Internal 0-based inclusive coordinate → MCP-visible 1-based inclusive.
pub(crate) fn to1(x: i64) -> i64 {
    x + 1
}

/// MCP-visible 1-based inclusive coordinate → internal 0-based inclusive.
/// Saturating: every caller range-checks the result afterwards, and an
/// i64::MIN input must not panic a debug build.
pub(crate) fn from1(x: i64) -> i64 {
    x.saturating_sub(1)
}

/// Upper bound for caller-supplied `flank` context windows (read_sequence
/// coordinate mode, add_alignment focus). Bounds i64 arithmetic and stops
/// absurd requests; anything larger is clamped at the sequence ends anyway.
pub(crate) const MAX_FLANK: i64 = 10_000;

/// Length unit of a molecule type — the `unit` field of every response that
/// reports lengths.
pub(crate) fn unit_for(molecule_type: &str) -> &'static str {
    match molecule_type {
        "rna" => "nt",
        "protein" => "aa",
        _ => "bp",
    }
}

/// Round to one decimal (Tm/GC reporting). Integers stay integral.
pub(crate) fn round1(x: f64) -> f64 {
    (x * 10.0).round() / 10.0
}

// ---------------------------------------------------------------------------
// Envelope
// ---------------------------------------------------------------------------

/// Start a success response: `{ok: true, message, projectId, text?}`.
pub(crate) fn ok_envelope(
    project_id: &str,
    message: impl Into<String>,
    text: Option<String>,
) -> serde_json::Value {
    let mut v = serde_json::json!({
        "ok": true,
        "message": message.into(),
        "projectId": project_id,
    });
    if let Some(t) = text {
        v["text"] = serde_json::json!(t);
    }
    v
}

/// Start a domain-failure response: `{ok: false, message, projectId}`.
/// Callers add diagnostic fields (e.g. `currentContent`, `unknownEnzymes`).
pub(crate) fn fail_envelope(project_id: &str, message: impl Into<String>) -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "message": message.into(),
        "projectId": project_id,
    })
}

/// Append an informational note to the response's `notes` array.
pub(crate) fn push_note(v: &mut serde_json::Value, note: impl Into<String>) {
    push_string_array(v, "notes", note.into());
}

/// Append a warning to the response's `warnings` array.
pub(crate) fn push_warning(v: &mut serde_json::Value, warning: impl Into<String>) {
    push_string_array(v, "warnings", warning.into());
}

fn push_string_array(v: &mut serde_json::Value, key: &str, item: String) {
    match v.get_mut(key).and_then(|a| a.as_array_mut()) {
        Some(arr) => arr.push(serde_json::json!(item)),
        None => v[key] = serde_json::json!([item]),
    }
}

/// Inject the `sequenceHash`/`revCompHash` pair into a tool response JSON.
pub(crate) fn insert_seq_hashes(v: &mut serde_json::Value, hashes: &(String, Option<String>)) {
    v["sequenceHash"] = serde_json::json!(hashes.0);
    v["revCompHash"] = match &hashes.1 {
        Some(h) => serde_json::json!(h),
        None => serde_json::Value::Null,
    };
}

// ---------------------------------------------------------------------------
// Entity serializers (1-based inclusive)
// ---------------------------------------------------------------------------

/// Serialize a feature for an MCP response with its coordinates bumped to
/// 1-based inclusive (the model stores 0-based inclusive).
pub(crate) fn feature_json_1based(f: &Feature) -> serde_json::Value {
    let mut v = serde_json::to_value(f).unwrap_or_default();
    v["start"] = serde_json::json!(to1(f.start));
    v["end"] = serde_json::json!(to1(f.end));
    if let Some(segs) = v.get_mut("segments").and_then(|s| s.as_array_mut()) {
        for seg in segs.iter_mut() {
            if let Some(s) = seg.get("start").and_then(|x| x.as_i64()) {
                seg["start"] = serde_json::json!(s + 1);
            }
            if let Some(e) = seg.get("end").and_then(|x| x.as_i64()) {
                seg["end"] = serde_json::json!(e + 1);
            }
        }
    }
    v
}

/// Convert a binding-site JSON object coming from a `crate::do_*` core
/// (0-based `templateStart`, 0-based-EXCLUSIVE `templateEnd`) to the 1-based
/// inclusive MCP convention: `templateStart` +1, while `templateEnd` keeps its
/// value (a 0-based exclusive end IS the 1-based inclusive end of the site).
/// A circular site ending exactly at the last base stores `templateEnd` 0
/// (wrapped); report the last base (`tlen`) instead of the out-of-domain 0.
pub(crate) fn site_json_to_1based(site: &mut serde_json::Value, tlen: i64, circular: bool) {
    if let Some(s) = site.get("templateStart").and_then(|v| v.as_i64()) {
        site["templateStart"] = serde_json::json!(s + 1);
    }
    if circular && site.get("templateEnd").and_then(|v| v.as_i64()) == Some(0) {
        site["templateEnd"] = serde_json::json!(tlen);
    }
}

/// Rename a key in place (no-op when absent). Used to map shared core payload
/// field names to the MCP naming conventions.
pub(crate) fn rename_key(v: &mut serde_json::Value, from: &str, to: &str) {
    if let Some(obj) = v.as_object_mut() {
        if let Some(val) = obj.remove(from) {
            obj.insert(to.to_string(), val);
        }
    }
}

/// The single primer binding-site shape shared by add_primer, list_primers and
/// check_primer_binding (1-based inclusive):
/// `{strand, templateStart, templateEnd, tm, annealLength, tailLength,
/// alignedTemplate, matchMask}`. The 3'-most base mismatches exactly when
/// `matchMask` ends with '.'.
pub(crate) fn primer_site_json(
    template: &str,
    topology: &str,
    primer_seq: &str,
    site: &PrimerBindingSite,
    tlen: i64,
) -> serde_json::Value {
    let (aligned_template, match_mask) =
        libregene_core::primer::align::template_coverage(template, topology, primer_seq, site);
    let mut v = serde_json::json!({
        "strand": site.strand,
        "templateStart": site.template_start,
        "templateEnd": site.template_end,
        "tm": round1(site.tm),
        "annealLength": libregene_core::primer::align::anneal_len(
            template, topology, primer_seq, site,
        ),
        "tailLength": site.five_prime_tail.len(),
        "alignedTemplate": aligned_template,
        "matchMask": match_mask,
    });
    site_json_to_1based(&mut v, tlen, topology == "circular");
    v
}
