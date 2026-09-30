//! Shared MCP helpers: coordinate conversion, 1-based JSON serializers,
//! response envelopes, sequence-hash injection.

use libregene_core::models::Feature;

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

pub(crate) fn ok_envelope(project_id: &str, message: String, region_view: Option<String>) -> serde_json::Value {
    let mut v = serde_json::json!({
        "ok": true,
        "message": message,
        "projectId": project_id,
    });
    if let Some(rv) = region_view {
        v["regionView"] = serde_json::json!(rv);
    }
    v
}

pub(crate) fn fail_envelope(project_id: &str, message: String) -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "message": message,
        "projectId": project_id,
    })
}

/// Inject the `sequenceHash`/`revCompHash` pair into a tool response JSON.
pub(crate) fn insert_seq_hashes(v: &mut serde_json::Value, hashes: &(String, Option<String>)) {
    v["sequenceHash"] = serde_json::json!(hashes.0);
    v["revCompHash"] = match &hashes.1 {
        Some(h) => serde_json::json!(h),
        None => serde_json::Value::Null,
    };
}
