//! Mutating MCP tool: edit_sequence.

use rmcp::{handler::server::wrapper::Json, ErrorData};
use tauri::Runtime;

use crate::mcp::support::{fail_envelope, from1, insert_seq_hashes, push_note, to1, unit_for};
use crate::mcp::types::EditSequenceRequest;
use crate::mcp::LibreGeneMcp;
use libregene_core::digest::{project_digest, DigestOptions};

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

impl<R: Runtime> LibreGeneMcp<R> {
    pub(crate) async fn edit_sequence_impl(
        &self,
        request: EditSequenceRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project_light(request.project_id).await?;
        self.require_agent_tab(&id).await?;
        let seq_hashes =
            libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
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

        // (flipped, label, match_count) of a replacementHash resolution, for
        // the response notes.
        let mut hash_notes: Option<(bool, String, usize)> = None;

        let (replacement, parsed_annotations) = match (
            request.replacement,
            request.replacement_path,
            request.replacement_hash,
        ) {
            (Some(_), Some(_), _) | (Some(_), _, Some(_)) | (None, Some(_), Some(_)) => {
                return Ok(fail(
                    "Provide exactly one of `replacement`, `replacementPath` or `replacementHash`, not several"
                        .to_string(),
                ));
            }
            (None, None, None) => {
                return Ok(fail(
                    "Provide exactly one of `replacement` (sequence string, empty = delete), `replacementPath` (sequence file) or `replacementHash` (workspace hash)"
                        .to_string(),
                ));
            }
            (Some(s), None, None) => (s, None),
            (None, None, Some(hash)) => {
                // A hash that matches nothing is a domain rejection.
                let resolved = match crate::mcp::workspace::resolve_workspace_hash(
                    &self.pm,
                    &self.workspace,
                    &hash,
                )
                .await
                {
                    Ok(r) => r,
                    Err(e) => return Ok(fail(e)),
                };
                if project.molecule_type == "protein" && resolved.molecule_type != "protein" {
                    return Ok(fail(format!(
                        "replacementHash resolved to a {} entry but the project is protein",
                        resolved.molecule_type
                    )));
                }
                if project.molecule_type != "protein" && resolved.molecule_type == "protein" {
                    return Ok(fail(
                        "replacementHash resolved to a protein entry but the project is DNA/RNA"
                            .to_string(),
                    ));
                }
                hash_notes = Some((
                    resolved.flipped,
                    resolved.label.clone(),
                    resolved.match_count,
                ));
                (
                    resolved.sequence,
                    Some((resolved.features, resolved.primers)),
                )
            }
            (None, Some(path), None) => {
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
                        (data.sequence, Some((data.features, data.primers)))
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
            if replacement.matches('*').count() > 1
                || !body.chars().all(|c| c.is_ascii_alphabetic())
            {
                return Ok(fail(
                    "Invalid protein replacement: only amino-acid letters (A-Z) and an optional trailing '*' (stop codon) are allowed".to_string(),
                ));
            }
        } else if !replacement.is_empty()
            && !replacement.chars().all(|c| {
                c.is_ascii() && !libregene_core::primer::iupac::iupac_expand(c as u8).is_empty()
            })
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
                    "strand \"-\" (reverse complement) is only supported on DNA projects"
                        .to_string(),
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
            let out_of_bounds = if is_insertion {
                start > live_len
            } else {
                end >= live_len
            };
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
            let context_before = (!cb.is_empty())
                .then(|| serde_json::json!({ "start": cb_lo + 1, "end": start, "sequence": cb }));
            let ca_hi = (end + 1 + 30).min(p.length);
            let ca = p.sequence[(end + 1) as usize..ca_hi as usize].to_string();
            let context_after = (!ca.is_empty())
                .then(|| serde_json::json!({ "start": end + 2, "end": ca_hi, "sequence": ca }));

            // Side effects on features, derived from the pre-edit list with
            // the same span math as the adjust below.
            let mut impact = libregene_core::utils::features_edit_impact(
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
            // Text color runs ride the same span math.
            let new_seq_len = p.length - (end - start + 1) + replacement.len() as i64;
            p.text_colors_top = libregene_core::utils::adjust_text_colors_for_edit(
                &p.text_colors_top,
                start,
                end,
                replacement.len() as i64,
                new_seq_len,
            );
            p.text_colors_bottom = libregene_core::utils::adjust_text_colors_for_edit(
                &p.text_colors_bottom,
                start,
                end,
                replacement.len() as i64,
                new_seq_len,
            );
            if let Some((feats, primers)) = parsed_annotations {
                // The replacement carries its own annotations: on an
                // equal-length edit the covered features survive
                // adjust_features_for_edit (point mutations must not drop
                // them), so they are removed here or the transferred copy
                // would duplicate the original. Length-changing edits already
                // dropped (and reported) them via adjust_features_for_edit;
                // re-checking their shifted coordinates against the old span
                // would misfire on clipped boundary features.
                if !feats.is_empty() && replacement.len() as i64 == end - start + 1 {
                    let mut i = 0;
                    while i < p.features.len() {
                        if libregene_core::utils::feature_fully_within(&p.features[i], start, end) {
                            let f = p.features.remove(i);
                            impact.removed_features.push(
                                libregene_core::models::RemovedFeatureImpact {
                                    name: f.name,
                                    ftype: f.ftype,
                                    location: format!("{}..{}", f.start, f.end),
                                },
                            );
                        } else {
                            i += 1;
                        }
                    }
                }
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
                transferred_feature_names = transferred.iter().map(|f| f.name.clone()).collect();
                p.features.extend(transferred);
                if p.is_dna() {
                    let mut taken_ids: std::collections::HashSet<String> =
                        p.primers.iter().map(|pr| pr.id.clone()).collect();
                    for mut pr in primers {
                        pr.name = libregene_core::utils::unique_name(&pr.name, &taken);
                        taken.insert(pr.name.clone());
                        pr.id = libregene_core::utils::unique_name(&pr.id, &taken_ids);
                        taken_ids.insert(pr.id.clone());
                        pr.binding_sites = Vec::new();
                        transferred_primer_names.push(pr.name.clone());
                        p.primers.push(pr);
                    }
                }
            }
            p.sequence = new_seq.clone();
            p.length = p.sequence.len() as i64;
            pm.bump_template_revision(&id);
            pm.mark_dirty(&id);
            (new_seq, context_before, context_after, impact)
        };
        let new_len = new_seq.len() as i64;

        let old_win = ((start - 30).max(0), (end + 30).min(len - 1));
        let old_opts = DigestOptions {
            compact_enzymes: true,
            ..DigestOptions::default()
        };
        let old_region = project_digest(&project, &old_opts, Some(old_win)).ok();

        crate::recompute_after_sequence_change(
            &self.app_handle,
            &self.pm,
            &self.agent_tabs,
            None,
            &id,
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;

        let repl_len = replacement.len() as i64;
        {
            let start0 = start;
            let end0 = if repl_len > 0 {
                (start0 + repl_len - 1).min(new_len - 1)
            } else {
                start0.min(new_len - 1)
            };
            crate::payload::emit_mutation_focus(
                &self.app_handle,
                &id,
                start0.max(0),
                end0.max(0),
                "sequence",
            );
        }
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
        if let Some((flipped, label, match_count)) = hash_notes {
            if flipped {
                push_note(&mut v, format!(
                    "replacementHash matched in swapped orientation — inserted the reverse complement of '{}' (annotations flipped)",
                    label
                ));
            }
            let _ = match_count;
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
}
