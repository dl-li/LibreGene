//! Mutating MCP tools: edit_sequence + set_feature.

use rmcp::{ErrorData, handler::server::wrapper::Json};
use tauri::Runtime;

use libregene_core::digest::{DigestOptions, project_digest};
use libregene_core::models::{Feature, Segment};

use crate::mcp::LibreGeneMcp;
use crate::mcp::next_id;
use crate::mcp::support::{fail_envelope, from1, insert_seq_hashes, ok_envelope, push_note, to1, unit_for};
use crate::mcp::types::{EditSequenceRequest, FeatureSegmentSpec, SetFeatureRequest};

/// `removedFeatures`/`clippedFeatures` echo for edit_sequence, 1-based
/// inclusive (the impact model is internal 0-based).
fn edit_impact_json(
    impact: &libregene_core::models::FeaturesEditImpact,
) -> (serde_json::Value, serde_json::Value) {
    let removed: Vec<serde_json::Value> = impact
        .removed_features
        .iter()
        .map(|r| {
            let loc = match r.location.split_once("..") {
                Some((a, b)) => match (a.parse::<i64>(), b.parse::<i64>()) {
                    (Ok(s), Ok(e)) => format!("{}..{}", s + 1, e + 1),
                    _ => r.location.clone(),
                },
                None => r.location.clone(),
            };
            serde_json::json!({"name": r.name, "ftype": r.ftype, "location": loc})
        })
        .collect();
    let clipped: Vec<serde_json::Value> = impact
        .clipped_features
        .iter()
        .map(|c| {
            let segs = |v: &[libregene_core::models::EditSpan]| {
                v.iter()
                    .map(|s| serde_json::json!({"start": to1(s.start), "end": to1(s.end)}))
                    .collect::<Vec<_>>()
            };
            serde_json::json!({
                "name": c.name,
                "ftype": c.ftype,
                "before": {"start": to1(c.before.start), "end": to1(c.before.end)},
                "after": {"start": to1(c.after.start), "end": to1(c.after.end)},
                "beforeSegments": segs(&c.before_segments),
                "afterSegments": segs(&c.after_segments),
            })
        })
        .collect();
    (serde_json::json!(removed), serde_json::json!(clipped))
}

/// Stored feature coordinates rendered GenBank-style in the 1-based inclusive
/// MCP convention (e.g. "100..200", "complement(50..80)", "join(1..100,200..300)").
fn stored_location(f: &Feature) -> String {
    let segs: Vec<(i64, i64)> = if f.segments.is_empty() {
        vec![(f.start, f.end)]
    } else {
        f.segments.iter().map(|s| (s.start, s.end)).collect()
    };
    let inner = segs
        .iter()
        .map(|(s, e)| format!("{}..{}", s + 1, e + 1))
        .collect::<Vec<_>>()
        .join(",");
    let loc = if segs.len() > 1 { format!("join({})", inner) } else { inner };
    if f.strand == "-" {
        format!("complement({})", loc)
    } else {
        loc
    }
}

/// Resolve set_feature span parameters (given 1-based inclusive, the MCP
/// interface convention) into model segments and overall bounds (internal
/// 0-based inclusive). Bounds against the project length are checked by the
/// caller (`span_within_bounds`).
fn resolve_feature_span(
    start: Option<i64>,
    end: Option<i64>,
    segments: Option<Vec<FeatureSegmentSpec>>,
) -> Result<(Vec<Segment>, i64, i64), String> {
    if segments.is_some() && (start.is_some() || end.is_some()) {
        return Err("segments is mutually exclusive with start/end".to_string());
    }
    match (start, end, segments) {
        (Some(s), Some(e), None) => {
            if s < 1 || e < s {
                return Err(format!(
                    "invalid span {}..{}: need 1 <= start <= end (1-based inclusive)",
                    s, e
                ));
            }
            Ok((vec![Segment { start: s - 1, end: e - 1, color: None }], s - 1, e - 1))
        }
        (None, None, Some(segs)) => {
            if segs.is_empty() {
                return Err("segments must not be empty".to_string());
            }
            let mut out = Vec::with_capacity(segs.len());
            for seg in &segs {
                if seg.start < 1 || seg.end < seg.start {
                    return Err(format!(
                        "invalid segment {}..{}: need 1 <= start <= end (1-based inclusive)",
                        seg.start, seg.end
                    ));
                }
                out.push(Segment { start: seg.start - 1, end: seg.end - 1, color: None });
            }
            // Encoding order (same rule as project creation): ascending
            // starts; an origin-wrapping feature leads with its tail, the one
            // descending transition marking the origin. Out-of-order segments
            // would make the first/last-derived bounds wrong (a phantom wrap).
            let descents = out.windows(2).filter(|w| w[1].start < w[0].start).count();
            let ordered = descents <= 1
                && (descents == 0 || out.first().unwrap().start > out.last().unwrap().end);
            if !ordered {
                return Err(
                    "segments are not in encoding order (ascending starts; a wrapping feature leads with its tail, e.g. join(8886..9326, 1..219))"
                        .to_string(),
                );
            }
            // Bounds come from the first segment's start and the last
            // segment's end (segments are in encoding order), so an
            // origin-wrapping feature keeps its `start > end` semantics
            // instead of being flattened by min/max.
            let s = out.first().unwrap().start;
            let e = out.last().unwrap().end;
            Ok((out, s, e))
        }
        (Some(_), None, None) | (None, Some(_), None) => {
            Err("start and end must be given together".to_string())
        }
        (None, None, None) => Err("give start+end or segments".to_string()),
        _ => Err("invalid span parameters".to_string()),
    }
}

impl<R: Runtime> LibreGeneMcp<R> {
    pub(crate) async fn edit_sequence_impl(
        &self,
        request: EditSequenceRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project_light(request.project_id).await?;
        self.require_agent_tab(&id).await?;
        let seq_hashes = libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        let fail = |msg: String| -> Json<serde_json::Value> {
            let mut v = fail_envelope(&id, msg);
            insert_seq_hashes(&mut v, &seq_hashes);
            Json(v)
        };
        let len = project.length;
        // Validate the raw 1-based inclusive inputs BEFORE any arithmetic on
        // them (from1 / end+1 would overflow on extreme i64 inputs); the
        // bounds check uses comparisons only, so once it passes, u_end <= len
        // and the wrap check's `u_end + 1` cannot overflow either.
        let (u_start, u_end) = (request.start, request.end);
        if u_start < 1 || u_start > len + 1 || u_end < 0 || u_end > len {
            return Ok(fail(format!(
                "range {}..{} out of bounds for sequence of length {} (1-based inclusive)",
                u_start, u_end, len
            )));
        }
        if u_start > u_end + 1 {
            return Ok(fail(format!(
                "invalid range {}..{}: start > end+1; ranges must not wrap (a pure insertion before base N is start=N, end=N-1)",
                u_start, u_end
            )));
        }
        let start = from1(u_start);
        let end = from1(u_end);

        let (replacement, parsed_annotations) = match (request.replacement, request.replacement_path) {
            (Some(_), Some(_)) => {
                return Ok(fail(
                    "Provide exactly one of `replacement` or `replacementPath`, not both"
                        .to_string(),
                ));
            }
            (None, None) => {
                return Ok(fail(
                    "Provide exactly one of `replacement` (sequence string, empty = delete) or `replacementPath` (sequence file)"
                        .to_string(),
                ));
            }
            (Some(s), None) => (s, None),
            (None, Some(path)) => {
                crate::validate_user_path(&path, crate::SEQ_EXTS).map_err(|e| {
                    ErrorData::invalid_params(format!("invalid replacementPath: {}", e), None)
                })?;
                let parsed = tokio::task::spawn_blocking(move || {
                    libregene_core::file_io::parse_file(std::path::Path::new(&path))
                })
                .await
                .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?;
                match parsed {
                    // Annotations travel with the sequence: features/primers
                    // from the file land on the inserted region below.
                    Ok(data) => {
                        // The file's molecule type must match the target
                        // project (e.g. a protein .gpt into a DNA project
                        // would corrupt the sequence).
                        if project.molecule_type == "protein" && data.molecule_type != "protein" {
                            return Ok(fail(format!(
                                "replacementPath is a {} file but the project is protein — pass a protein file (.gpt/.prot)",
                                data.molecule_type
                            )));
                        }
                        if project.molecule_type != "protein" && data.molecule_type == "protein" {
                            return Ok(fail(
                                "replacementPath is a protein file (.gpt/.prot) but the project is DNA/RNA — pass a nucleotide sequence file".to_string(),
                            ));
                        }
                        (
                            data.sequence,
                            Some((data.features, data.primers)),
                        )
                    }
                    Err(e) => {
                        return Ok(fail(format!(
                            "Failed to read replacement sequence file (supported: .gbk/.gb/.genbank, .dna/.rna/.prot, .gpt, .fa/.fasta, .ab1): {}",
                            e
                        )));
                    }
                }
            }
        };

        // Normalize the replacement to uppercase on every molecule type
        // (matching update_sequence): lowercase bases would evade the
        // case-sensitive enzyme recompute and leak into GenBank output.
        // Protein projects additionally require the amino-acid alphabet
        // (A-Z, optional single trailing '*' stop); DNA/RNA projects require
        // the IUPAC nucleotide alphabet (ACGTURYSWKMBDHVN).
        let mut replacement = replacement.to_ascii_uppercase();
        if project.molecule_type == "protein" {
            let body = replacement.strip_suffix('*').unwrap_or(&replacement);
            if replacement.matches('*').count() > 1 || !body.chars().all(|c| c.is_ascii_alphabetic()) {
                return Ok(fail(
                    "Invalid protein replacement: only amino-acid letters (A-Z) and an optional trailing '*' (stop codon) are allowed".to_string(),
                ));
            }
        } else if !replacement.is_empty()
            && !replacement
                .chars()
                .all(|c| c.is_ascii() && !libregene_core::primer::iupac::iupac_expand(c as u8).is_empty())
        {
            return Ok(fail(
                "Invalid DNA/RNA replacement: only IUPAC nucleotide bases (ACGTURYSWKMBDHVN) are allowed".to_string(),
            ));
        }

        // U and T both pass the IUPAC check above, but a cross-alphabet
        // insert would silently pollute the project (the enzyme recompute
        // does not recognize U; an RNA project must not gain T). Normalize
        // to the target project's alphabet and tell the caller when any
        // base was actually converted. Protein projects are untouched (the
        // amino-acid alphabet check above already covers them).
        let alphabet_note: Option<String> = if project.molecule_type == "protein" {
            None
        } else {
            let (from, to, project_kind, source_kind) = if project.molecule_type == "rna" {
                ('T', 'U', "RNA", "DNA")
            } else {
                ('U', 'T', "DNA", "RNA")
            };
            let count = replacement.matches(from).count();
            if count == 0 {
                None
            } else {
                replacement = replacement.replace(from, &to.to_string());
                Some(format!(
                    "Converted {} {}→{} to match the {} project (source looked like {})",
                    count, from, to, project_kind, source_kind
                ))
            }
        };

        // Insertion direction: "-" reverse-complements the replacement (DNA
        // only — revcomp is meaningless for RNA/protein sequences here).
        let reverse = match request.strand.as_deref() {
            None | Some("+") | Some(".") => false,
            Some("-") => true,
            Some(other) => {
                return Ok(fail(format!(
                    "Invalid strand '{}': must be \"+\" (default, insert as given) or \"-\" (reverse-complement before inserting)",
                    other
                )));
            }
        };
        if reverse {
            if !project.is_dna() {
                return Ok(fail(
                    "strand \"-\" (reverse complement) is only supported on DNA projects".to_string(),
                ));
            }
            replacement = libregene_core::utils::reverse_complement(&replacement);
        }

        let is_insertion = end + 1 == start;

        // One write-lock critical section: re-read the LIVE sequence, check
        // expected_old against it, adjust/transfer annotations and swap the
        // sequence atomically. Checking the resolve-time snapshot and mutating
        // in separate locks would let a concurrent edit slip between check
        // and apply, desyncing feature coordinates from the sequence. The
        // update_sequence core never touches feature coordinates (the
        // frontend adjusts them client-side), so the MCP path does it here.
        let mut transferred_feature_names: Vec<String> = Vec::new();
        let mut transferred_primer_names: Vec<String> = Vec::new();
        let (new_seq, context_before, context_after, impact) = {
            let mut pm = self.pm.write().await;
            let Some(p) = pm.get_project_mut_by_id(&id) else {
                return Ok(fail("Project not found".to_string()));
            };
            // The resolve-time bounds check ran against a snapshot; a
            // concurrent edit may have shrunk the sequence since — re-check
            // against the live sequence or the slices below would panic.
            let live_len = p.sequence.len() as i64;
            let out_of_bounds = if is_insertion { start > live_len } else { end >= live_len };
            if out_of_bounds {
                return Ok(fail(format!(
                    "range {}..{} out of bounds for sequence of length {} (1-based inclusive)",
                    u_start, u_end, live_len
                )));
            }
            let current: String = if is_insertion {
                String::new()
            } else {
                p.sequence[start as usize..=end as usize].to_string()
            };
            if let Some(expected) = &request.expected_old {
                if !current.eq_ignore_ascii_case(expected) {
                    let mut v = fail_envelope(
                        &id,
                        format!(
                            "expectedOld does not match the current bases at [{}..{}] — resend the edit with currentContent as expectedOld",
                            u_start, u_end
                        ),
                    );
                    v["currentContent"] = serde_json::json!(current);
                    insert_seq_hashes(&mut v, &seq_hashes);
                    return Ok(Json(v));
                }
            }

            // Flanking context, 1-based inclusive spans so the caller never has
            // to derive them from the edit coordinates.
            let cb_lo = (start - 30).max(0);
            let cb = p.sequence[cb_lo as usize..start as usize].to_string();
            let context_before = (!cb.is_empty()).then(|| {
                serde_json::json!({ "start": cb_lo + 1, "end": start, "sequence": cb })
            });
            let ca_hi = (end + 1 + 30).min(p.length);
            let ca = p.sequence[(end + 1) as usize..ca_hi as usize].to_string();
            let context_after = (!ca.is_empty()).then(|| {
                serde_json::json!({ "start": end + 2, "end": ca_hi, "sequence": ca })
            });

            // Side effects on features, derived from the pre-edit list with
            // the same span math as the adjust below.
            let impact = libregene_core::utils::features_edit_impact(
                &p.features,
                start,
                end,
                replacement.len() as i64,
            );

            let new_seq = format!(
                "{}{}{}",
                &p.sequence[..start as usize],
                replacement,
                &p.sequence[(end + 1) as usize..]
            );

            libregene_core::utils::adjust_features_for_edit(
                &mut p.features,
                start,
                end,
                replacement.len() as i64,
            );
            if let Some((feats, primers)) = parsed_annotations {
                // revcomp/uppercase preserve length, so replacement.len()
                // is the local coordinate space of the parsed annotations.
                let repl_len = replacement.len() as i64;
                let mut transferred = libregene_core::utils::transfer_features_for_insert(
                    &feats, repl_len, start, reverse,
                );
                let mut taken: std::collections::HashSet<String> = p
                    .features
                    .iter()
                    .map(|f| f.name.clone())
                    .chain(p.primers.iter().map(|pr| pr.name.clone()))
                    .collect();
                for f in &mut transferred {
                    f.name = libregene_core::utils::unique_name(&f.name, &taken);
                    taken.insert(f.name.clone());
                }
                transferred_feature_names =
                    transferred.iter().map(|f| f.name.clone()).collect();
                p.features.extend(transferred);
                if p.is_dna() {
                    for mut pr in primers {
                        pr.name = libregene_core::utils::unique_name(&pr.name, &taken);
                        taken.insert(pr.name.clone());
                        pr.id = pr.name.clone();
                        pr.binding_sites = Vec::new();
                        transferred_primer_names.push(pr.name.clone());
                        p.primers.push(pr);
                    }
                }
            }
            p.sequence = new_seq.clone();
            p.length = p.sequence.len() as i64;
            pm.mark_dirty(&id);
            (new_seq, context_before, context_after, impact)
        };
        let new_len = new_seq.len() as i64;

        let old_win = (
            (start - 30).max(0),
            (end + 30).min(len - 1),
        );
        let old_opts = DigestOptions {
            compact_enzymes: true,
            ..DigestOptions::default()
        };
        let old_region = project_digest(&project, &old_opts, Some(old_win)).ok();

        crate::recompute_after_sequence_change(
            &self.app_handle,
            &self.pm,
            &self.wp,
            &self.agent_tabs,
            None,
            &id,
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;

        let repl_len = replacement.len() as i64;
        let unit = unit_for(&project.molecule_type);
        let new_win = (
            (start - 30).max(0),
            (start + repl_len + 30 - 1).min(new_len - 1),
        );
        let new_region = self.digest_region(&id, Some(new_win), true, false).await;

        let (removed_json, clipped_json) = edit_impact_json(&impact);
        let action = if is_insertion {
            format!("Inserted {} {} before base {}", repl_len, unit, u_start)
        } else {
            format!(
                "Replaced [{}..{}] ({} {}) with {} {}",
                u_start,
                u_end,
                end - start + 1,
                unit,
                repl_len,
                unit
            )
        };
        let mut v = serde_json::json!({
            "ok": true,
            "message": format!(
                "{}{}; new length {} (was {})",
                action,
                if reverse { " (reverse-complemented)" } else { "" },
                new_len,
                len
            ),
            "projectId": id,
            "unit": unit,
            "oldLength": len,
            "newLength": new_len,
            "removedFeatures": removed_json,
            "clippedFeatures": clipped_json,
        });
        // Flanking context, 1-based inclusive spans; omitted at the sequence
        // ends (nothing to show).
        if let Some(ctx) = context_before {
            v["contextBefore"] = ctx;
        }
        if let Some(ctx) = context_after {
            v["contextAfter"] = ctx;
        }
        if !transferred_feature_names.is_empty() {
            v["transferredFeatures"] = serde_json::json!(transferred_feature_names);
        }
        if !transferred_primer_names.is_empty() {
            v["transferredPrimers"] = serde_json::json!(transferred_primer_names);
        }
        if let Some(note) = alphabet_note {
            push_note(&mut v, note);
        }
        if let Some(rv) = old_region {
            v["textBefore"] = serde_json::json!(rv);
        }
        if let Some(rv) = new_region {
            v["text"] = serde_json::json!(rv);
        }
        insert_seq_hashes(
            &mut v,
            &libregene_core::utils::orientation_hashes(&new_seq, &project.molecule_type),
        );
        Ok(Json(v))
    }

    pub(crate) async fn set_feature_impl(
        &self,
        request: SetFeatureRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.resolve_project_id(request.project_id.clone()).await?;
        self.require_agent_tab(&id).await?;
        let seq_hashes = self.project_seq_hashes(&id).await;
        let fail = |msg: String| -> Json<serde_json::Value> {
            let mut v = fail_envelope(&id, msg);
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            Json(v)
        };

        if let Some(feature_id) = request.feature_id.clone() {
            // ---- update mode ----
            if !self.feature_exists(&id, &feature_id).await {
                return Ok(fail(format!("Feature not found: {}", feature_id)));
            }
            if request.notes.is_some() {
                return Ok(fail(
                    "notes update is not supported by set_feature (notes can only be set when creating a feature)".to_string(),
                ));
            }
            if let Some(n) = &request.name {
                if n.is_empty() {
                    return Ok(fail("name must not be empty".to_string()));
                }
            }
            if request.name.is_none()
                && request.ftype.is_none()
                && request.color.is_none()
                && request.strand.is_none()
                && request.start.is_none()
                && request.end.is_none()
                && request.segments.is_none()
            {
                return Ok(fail(
                    "Nothing to update: give at least one of name/ftype/color/strand/start+end/segments".to_string(),
                ));
            }
            if let Some(s) = &request.strand {
                if !matches!(s.as_str(), "." | "+" | "-") {
                    return Ok(fail("Invalid strand: must be ., +, or -".to_string()));
                }
            }
            let has_span = request.start.is_some()
                || request.end.is_some()
                || request.segments.is_some();
            let new_span = if has_span {
                let span = resolve_feature_span(request.start, request.end, request.segments)
                    .map_err(|e| ErrorData::invalid_params(e, None))?;
                if let Err(e) = self.span_within_bounds(&id, &span.0, span.1, span.2).await {
                    return Ok(fail(e));
                }
                Some(span)
            } else {
                None
            };
            let payload = crate::do_update_feature(
                &self.app_handle,
                &self.pm,
                &self.wp,
                &self.agent_tabs,
                None,
                &id,
                &feature_id,
                move |f| {
                    if let Some((segments, start, end)) = new_span {
                        f.segments = segments;
                        f.start = start;
                        f.end = end;
                    }
                    if let Some(v) = request.name {
                        f.name = v;
                    }
                    if let Some(v) = request.ftype {
                        f.ftype = v;
                    }
                    if let Some(v) = &request.color {
                        f.color = v.clone();
                        for seg in f.segments.iter_mut() {
                            seg.color = Some(v.clone());
                        }
                    }
                    if let Some(v) = request.strand {
                        f.strand = v;
                    }
                    Ok(())
                },
            )
            .await
            .map_err(|e| ErrorData::invalid_params(e, None))?;
            if let Some(err) = Self::payload_error(&payload) {
                return Ok(fail(err));
            }
            let (message, unit) = {
                let pm = self.pm.read().await;
                let unit = pm
                    .get_project_by_id(&id)
                    .map(|p| unit_for(&p.molecule_type))
                    .unwrap_or("bp");
                let message = pm
                    .get_project_by_id(&id)
                    .and_then(|p| p.features.iter().find(|f| f.id == feature_id))
                    .map(|f| {
                        format!(
                            "Updated feature {}: {} {} at {} (1-based inclusive), strand {}",
                            feature_id,
                            f.ftype,
                            f.name,
                            stored_location(f),
                            f.strand
                        )
                    })
                    .unwrap_or_else(|| format!("Updated feature {}", feature_id));
                (message, unit)
            };
            let region = self.digest_feature_region(&id, &feature_id).await;
            let mut v = ok_envelope(&id, message, region);
            v["unit"] = serde_json::json!(unit);
            v["featureId"] = serde_json::json!(feature_id);
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            return Ok(Json(v));
        }

        // ---- create mode ----
        let name = match request.name.clone() {
            Some(n) if !n.is_empty() => n,
            _ => {
                return Ok(fail(
                    "name is required when creating a feature (omit featureId = create; pass featureId to update)".to_string(),
                ))
            }
        };
        let ftype = match request.ftype.clone() {
            Some(t) if !t.is_empty() => t,
            _ => {
                return Ok(fail(
                    "ftype is required when creating a feature (omit featureId = create; pass featureId to update)".to_string(),
                ))
            }
        };
        let (segments, start, end) = resolve_feature_span(
            request.start,
            request.end,
            request.segments,
        )
        .map_err(|e| ErrorData::invalid_params(e, None))?;
        let strand = request.strand.clone().unwrap_or_else(|| "+".to_string());
        if !matches!(strand.as_str(), "." | "+" | "-") {
            return Ok(fail("Invalid strand: must be ., +, or -".to_string()));
        }
        if let Err(e) = self.span_within_bounds(&id, &segments, start, end).await {
            return Ok(fail(e));
        }

        let feature_id = next_id("feature");
        let feature = Feature {
            id: feature_id.clone(),
            name: name.clone(),
            start,
            end,
            color: request.color.unwrap_or_else(|| "#60A5FA".to_string()),
            ftype: ftype.clone(),
            segments,
            strand,
            notes: request.notes.unwrap_or_default(),
            translation: String::new(),
            qualifiers: Vec::new(),
        };

        let stored = stored_location(&feature);
        let payload = crate::do_add_features(
            &self.app_handle,
            &self.pm,
            &self.wp,
            &self.agent_tabs,
            None,
            &id,
            vec![feature],
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(fail(err));
        }
        let region = self.digest_feature_region(&id, &feature_id).await;
        let mut v = ok_envelope(
            &id,
            format!("Added {} {} at {} (1-based inclusive)", ftype, name, stored),
            region,
        );
        v["featureId"] = serde_json::json!(feature_id);
        v["unit"] = serde_json::json!(self
            .pm
            .read()
            .await
            .get_project_by_id(&id)
            .map(|p| unit_for(&p.molecule_type))
            .unwrap_or("bp"));
        if let Some(h) = &seq_hashes {
            insert_seq_hashes(&mut v, h);
        }
        Ok(Json(v))
    }

}
