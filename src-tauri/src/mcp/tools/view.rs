//! Read-only MCP tools: project list/digests, sequence reads, search,
//! restriction sites, primers, ORFs.

use std::collections::HashMap;

use rmcp::{ErrorData, handler::server::wrapper::Json};
use tauri::Runtime;

use libregene_core::digest::{DigestOptions, cut_flanks, project_digest, read_sequence};
use libregene_core::models::{Enzyme, Feature};

use crate::mcp::LibreGeneMcp;
use crate::mcp::support::{
    MAX_FLANK, fail_envelope, feature_json_1based, from1, insert_seq_hashes, ok_envelope,
    site_json_to_1based, to1,
};
use crate::mcp::types::{
    FindOrfsRequest, FindRestrictionSitesRequest, ListPrimersRequest, OverviewRequest,
    RegionRequest, SearchRequest, SequenceRequest,
};

/// Feature/translation hits containing internal 0-based `position`,
/// serialized for read_sequence: every containing feature with its 1-based
/// feature-relative offset, plus CDS/mRNA codon/amino-acid details. Used for
/// the window mode's startContext/endContext and the coordinate modes' hit
/// details.
fn position_context_json(
    position: i64,
    sequence: &str,
    features: &[Feature],
) -> serde_json::Value {
    let feature_hits = libregene_core::coords::position_to_features(position, features);
    let translation_hits =
        libregene_core::coords::position_to_translations(position, sequence, features);
    serde_json::json!({
        "position": position + 1,
        "features": feature_hits.iter().map(|h| serde_json::json!({
            "featureId": h.feature_id,
            "name": h.name,
            "ftype": h.ftype,
            "strand": h.strand,
            "featureOffset": h.offset,
            "featureLength": h.length,
        })).collect::<Vec<_>>(),
        "translations": translation_hits.iter().map(|h| serde_json::json!({
            "featureId": h.feature_id,
            "name": h.name,
            "strand": h.strand,
            "codonIndex": h.codon_index,
            "aaPosition1Based": h.aa_position_1_based,
            "aaPositionExcludingMet": h.aa_position_excluding_met,
            "codon": h.codon,
            "aminoAcid": h.amino_acid.to_string(),
            "codonBaseIndex": h.codon_base_index,
        })).collect::<Vec<_>>(),
    })
}

impl<R: Runtime> LibreGeneMcp<R> {
    pub(crate) async fn list_projects_impl(&self) -> Result<Json<serde_json::Value>, ErrorData> {
        let pm = self.pm.read().await;
        let mut projects = pm.list_projects();
        for entry in projects.iter_mut() {
            if let Some(p) = entry
                .get("id")
                .and_then(|v| v.as_str())
                .and_then(|id| pm.get_project_by_id(id))
            {
                insert_seq_hashes(
                    entry,
                    &libregene_core::utils::orientation_hashes(&p.sequence, &p.molecule_type),
                );
            }
        }
        let active_id = pm.active_id().map(|s| s.to_string());
        Ok(Json(serde_json::json!({
            "projects": projects,
            "activeId": active_id,
        })))
    }

    pub(crate) async fn get_project_overview_impl(
        &self,
        request: OverviewRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let hashes = libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        let opts = DigestOptions {
            max_features: request.max_features,
            feature_filter: request.feature_filter,
            compact_enzymes: false,
            compact_cutters: request.compact_cutters.unwrap_or(true),
            include_auto_annotation: true,
        };
        // Auto-annotation scans the whole feature database — CPU-heavy, so
        // render off the tokio worker.
        let text = tokio::task::spawn_blocking(move || project_digest(&project, &opts, None))
            .await
            .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let mut v = serde_json::json!({ "projectId": id, "text": text });
        insert_seq_hashes(&mut v, &hashes);
        Ok(Json(v))
    }

    pub(crate) async fn get_region_view_impl(
        &self,
        request: RegionRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let hashes = libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        let opts = DigestOptions {
            max_features: request.max_features,
            feature_filter: request.feature_filter,
            compact_enzymes: request.compact.unwrap_or(true),
            compact_cutters: false,
            include_auto_annotation: false,
        };
        let region = (from1(request.start), from1(request.end));
        let text = tokio::task::spawn_blocking(move || project_digest(&project, &opts, Some(region)))
            .await
            .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let mut v = serde_json::json!({ "projectId": id, "text": text });
        insert_seq_hashes(&mut v, &hashes);
        Ok(Json(v))
    }

    pub(crate) async fn read_sequence_impl(
        &self,
        request: SequenceRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project_light(request.project_id).await?;
        let seq_hashes = libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        // Reads never mutate, so the resolve-time hash stays valid; the
        // closure also serves fail paths that run inside a lock below (no
        // lock acquisition allowed there).
        let fail = |msg: String| -> Json<serde_json::Value> {
            let mut v = fail_envelope(&id, msg);
            insert_seq_hashes(&mut v, &seq_hashes);
            Json(v)
        };
        let window_active = request.start.is_some() || request.end.is_some();
        let coord_active = request.position.is_some()
            || request.feature_id.is_some()
            || request.feature_offset.is_some()
            || request.aa_position.is_some();
        if window_active == coord_active {
            return Ok(fail(
                "Provide exactly one input form: `start` + `end` (window read); `position`; `feature_id` + `feature_offset`; or `feature_id` + `aa_position`".to_string(),
            ));
        }

        if window_active {
            let (s, e) = match (request.start, request.end) {
                (Some(s), Some(e)) => (from1(s), from1(e)),
                _ => {
                    return Ok(fail(
                        "start and end are both required (1-based inclusive)".to_string(),
                    ))
                }
            };
            let text = read_sequence(&project, s, e)
                .map_err(|e| ErrorData::invalid_params(e, None))?;
            let bases = libregene_core::digest::read_sequence_bases(&project, s, e)
                .map_err(|e| ErrorData::invalid_params(e, None))?;
            let mut v = serde_json::json!({
                "projectId": id,
                "sequence": bases,
                "text": text,
                "startContext": position_context_json(s, &project.sequence, &project.features),
                "endContext": position_context_json(e, &project.sequence, &project.features),
            });
            insert_seq_hashes(&mut v, &seq_hashes);
            return Ok(Json(v));
        }

        // Coordinate mode: resolve the input form to an absolute 0-based
        // template position, then report hit details + a flank window.
        let sequence = &project.sequence;
        let features = &project.features;
        let len = sequence.len() as i64;

        let position: i64;
        let input_json: serde_json::Value;
        let codon_positions_opt: Option<[i64; 3]>;

        let has_position = request.position.is_some() as u8;
        let has_feature_offset = request.feature_id.is_some() && request.feature_offset.is_some();
        let has_aa_position = request.feature_id.is_some() && request.aa_position.is_some();
        if has_position + has_feature_offset as u8 + has_aa_position as u8 != 1 {
            return Ok(fail(
                "Provide exactly one of: `position`; `feature_id` + `feature_offset`; or `feature_id` + `aa_position`".to_string(),
            ));
        }

        if let Some(pos1) = request.position {
            if pos1 < 1 || pos1 > len {
                return Ok(fail(
                    format!("position {} out of bounds (1..={})", pos1, len),
                ));
            }
            position = pos1 - 1;
            input_json = serde_json::json!({ "kind": "template", "position": pos1 });
            codon_positions_opt = None;
        } else if let Some(feature_id) = &request.feature_id {
            let f = features
                .iter()
                .find(|f| &f.id == feature_id)
                .ok_or_else(|| ErrorData::invalid_params(format!("feature '{}' not found", feature_id), None))?;
            if let Some(offset1) = request.feature_offset {
                match libregene_core::coords::position_from_feature_offset(f, offset1) {
                    Ok(pos0) => {
                        // Feature coordinates are file-derived and not
                        // range-checked at parse time; reject before the
                        // sequence indexing below panics.
                        if pos0 < 0 || pos0 >= len {
                            return Ok(fail(
                                format!(
                                    "feature '{}' coordinates fall outside the sequence (length {})",
                                    f.name, len
                                ),
                            ));
                        }
                        position = pos0;
                        input_json = serde_json::json!({
                            "kind": "featureOffset",
                            "featureId": feature_id,
                            "featureOffset": offset1,
                        });
                        codon_positions_opt = None;
                    }
                    Err(e) => return Ok(fail(e)),
                }
            } else if let Some(aa1) = request.aa_position {
                match libregene_core::coords::codon_from_aa(f, sequence, aa1) {
                    Ok((positions, codon, aa)) => {
                        position = positions[0];
                        input_json = serde_json::json!({
                            "kind": "aminoAcid",
                            "featureId": feature_id,
                            "aaPosition": aa1,
                            "codon": codon,
                            "aminoAcid": aa.to_string(),
                        });
                        codon_positions_opt = Some(positions);
                    }
                    Err(e) => return Ok(fail(e)),
                }
            } else {
                // Unreachable because of the mutual-exclusion check above.
                return Ok(fail(
                    "Provide exactly one of: `position`; `feature_id` + `feature_offset`; or `feature_id` + `aa_position`".to_string(),
                ));
            }
        } else {
            return Ok(fail(
                "Provide exactly one of: `position`; `feature_id` + `feature_offset`; or `feature_id` + `aa_position`".to_string(),
            ));
        }

        let flank = request.flank.unwrap_or(30).clamp(0, MAX_FLANK);
        let ws = position.saturating_sub(flank).max(0);
        let we = position.saturating_add(flank).min(len - 1);
        let ctx = position_context_json(position, sequence, features);
        let text = read_sequence(&project, ws, we)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let bases = libregene_core::digest::read_sequence_bases(&project, ws, we)
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let mut v = serde_json::json!({
            "projectId": id,
            "input": input_json,
            "position": position + 1,
            "base": sequence[position as usize..position as usize + 1].to_ascii_uppercase(),
            "features": ctx["features"],
            "translations": ctx["translations"],
            "sequence": bases,
            "text": text,
        });
        if let Some(positions) = codon_positions_opt {
            v["codonPositions"] = serde_json::json!(
                positions.iter().map(|p| p + 1).collect::<Vec<_>>()
            );
        }
        insert_seq_hashes(&mut v, &seq_hashes);
        Ok(Json(v))
    }

    pub(crate) async fn search_sequence_impl(
        &self,
        request: SearchRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
        let hashes = self.project_seq_hashes(&id).await;
        let matches = crate::do_search_sequence(&self.pm, &id, request.query)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;
        let matches: Vec<serde_json::Value> = matches
            .iter()
            .map(|m| {
                serde_json::json!({
                    "start": to1(m.start),
                    "end": to1(m.end),
                    "strand": m.strand,
                })
            })
            .collect();
        let mut v = serde_json::json!({ "projectId": id, "matches": matches });
        if let Some(h) = &hashes {
            insert_seq_hashes(&mut v, h);
        }
        Ok(Json(v))
    }

    pub(crate) async fn find_restriction_sites_impl(
        &self,
        request: FindRestrictionSitesRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        if !project.is_dna() {
            return Err(ErrorData::invalid_params(
                format!(
                    "This tool only supports DNA projects; project '{}' is a {} project",
                    id, project.molecule_type
                ),
                None,
            ));
        }
        let hashes = libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        let wanted: Option<Vec<String>> = request.enzymes.map(|v| {
            v.into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        });
        // The engine only stores entries with at least one recognition site,
        // so requested names fall into three classes: cutting this sequence
        // (resolved), in the enzyme database but no site here (no_site), and
        // unknown to the database (unknown). Batch queries degrade
        // gracefully: known names return normally and only truly unknown
        // names are listed under `unknownEnzymes`; a query where EVERY name
        // is unknown still fails with near-match suggestions (the enzyme-name
        // probe).
        let mut requested: Vec<String> = Vec::new();
        let mut no_site: Vec<String> = Vec::new();
        let mut unknown: Vec<(String, Vec<String>)> = Vec::new();
        if let Some(list) = wanted.as_ref().filter(|l| !l.is_empty()) {
            let names: Vec<&str> = project.enzymes.iter().map(|e| e.name.as_str()).collect();
            let db = libregene_core::enzyme::search::get_db();
            for n in list {
                if let Some(found) = names.iter().find(|a| a.eq_ignore_ascii_case(n)) {
                    if !requested.iter().any(|r| r.eq_ignore_ascii_case(found)) {
                        requested.push(found.to_string());
                    }
                    continue;
                }
                if let Some(e) = db.enzymes.iter().find(|e| e.name.eq_ignore_ascii_case(n)) {
                    if !no_site.iter().any(|r| r.eq_ignore_ascii_case(&e.name)) {
                        no_site.push(e.name.clone());
                    }
                    continue;
                }
                let q = n.to_lowercase();
                let sugg: Vec<String> = names
                    .iter()
                    .copied()
                    .filter(|a| a.to_lowercase().contains(&q))
                    .take(5)
                    .map(|s| s.to_string())
                    .collect();
                unknown.push((n.clone(), sugg));
            }
            if !unknown.is_empty() && requested.is_empty() && no_site.is_empty() {
                let msg = unknown
                    .iter()
                    .map(|(n, sugg)| {
                        if sugg.is_empty() {
                            format!(
                                "Unknown enzyme '{}': no enzyme with a recognition site in this project has a similar name",
                                n
                            )
                        } else {
                            format!(
                                "Unknown enzyme '{}'; enzymes cutting this sequence with similar names: {}",
                                n,
                                sugg.join(", ")
                            )
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                let mut v = fail_envelope(&id, msg);
                insert_seq_hashes(&mut v, &hashes);
                return Ok(Json(v));
            }
        }
        let filter_active = !requested.is_empty() || !no_site.is_empty() || !unknown.is_empty();
        let mut by_name: HashMap<&str, Vec<&Enzyme>> = HashMap::new();
        for e in &project.enzymes {
            if !filter_active || requested.iter().any(|w| w.eq_ignore_ascii_case(&e.name)) {
                by_name.entry(e.name.as_str()).or_default().push(e);
            }
        }
        let mut enzyme_names: Vec<&str> = by_name.keys().copied().collect();
        enzyme_names.sort();
        let circular = project.topology == "circular";
        let tlen = project.length;
        let mut enzymes_json: Vec<serde_json::Value> = enzyme_names
            .into_iter()
            .map(|n| {
                let mut sites = by_name[n].clone();
                // Circular display frames can store rec_start/rec_end shifted
                // by a whole sequence length (the engine's comment notes the
                // frontend wraps indices mod seq_len); wrap at this boundary so
                // MCP coordinates always land in 1..=len. A recognition
                // spanning the origin then reads recStart > recEnd.
                let wrap = |x: i64| -> i64 {
                    if circular { x.rem_euclid(tlen) } else { x }
                };
                sites.sort_by_key(|e| wrap(e.rec_start));
                serde_json::json!({
                    "name": n,
                    "sites": sites.iter().map(|e| {
                        let rec_start = to1(wrap(e.rec_start));
                        let rec_end = to1(wrap(e.rec_end));
                        let cuts: Vec<serde_json::Value> = e.cut_pairs.iter().map(|p| serde_json::json!({
                            "topCutIndex": cut_flanks(p.top_cut_index, tlen, circular).0,
                            "botCutIndex": cut_flanks(p.bot_cut_index, tlen, circular).0,
                        })).collect();
                        // Type IIS and similar enzymes cut outside their
                        // recognition sequence; flag those sites so the
                        // cut-vs-recognition offset does not have to be
                        // inferred from the coordinates alone.
                        let outside = if circular {
                            // Compare in the engine's unwrapped frame: a cut is
                            // inside the recognition when its modular distance
                            // from rec_start falls in 1..=span+1 (the same
                            // boundary semantics as the linear check below).
                            let span = e.rec_end - e.rec_start;
                            e.cut_pairs.iter().any(|p| {
                                let dt = (p.top_cut_index - e.rec_start).rem_euclid(tlen);
                                let db = (p.bot_cut_index - e.rec_start).rem_euclid(tlen);
                                dt == 0 || db == 0 || dt > span + 1 || db > span + 1
                            })
                        } else {
                            cuts.iter().any(|c| {
                                let t = c["topCutIndex"].as_i64().unwrap_or(0);
                                let b = c["botCutIndex"].as_i64().unwrap_or(0);
                                t < rec_start || t > rec_end || b < rec_start || b > rec_end
                            })
                        };
                        let mut site = serde_json::json!({
                            "recStart": rec_start,
                            "recEnd": rec_end,
                            "recSeq": e.rec_seq,
                            "strand": e.recognition_strand,
                            "cuts": cuts,
                            "cutsOutsideRecognitionSite": outside,
                            "methylationBlocked": e.methylation_blocked,
                            "unique": e.is_unique,
                        });
                        if outside {
                            site["note"] = serde_json::json!(
                                "cut positions lie outside the recognition sequence (type IIS-style); topCutIndex/botCutIndex give the actual break points"
                            );
                        }
                        site
                    }).collect::<Vec<_>>(),
                })
            })
            .collect();
        // Known-in-database enzymes without a site on this sequence are
        // reported with empty sites and an explicit note, so an agent can
        // tell "no site here" apart from "unknown enzyme".
        for n in &no_site {
            enzymes_json.push(serde_json::json!({
                "name": n,
                "sites": [],
                "note": "enzyme exists in the enzyme database but has no recognition site on this sequence",
            }));
        }
        enzymes_json.sort_by(|a, b| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")));
        let mut resp = serde_json::json!({ "projectId": id, "enzymes": enzymes_json });
        insert_seq_hashes(&mut resp, &hashes);
        if !unknown.is_empty() {
            resp["unknownEnzymes"] = unknown
                .iter()
                .map(|(n, sugg)| {
                    serde_json::json!({
                        "name": n,
                        "error": format!("Unknown enzyme '{}': not in the enzyme database", n),
                        "similar": sugg,
                    })
                })
                .collect();
        }
        Ok(Json(resp))
    }

    pub(crate) async fn list_primers_impl(
        &self,
        request: ListPrimersRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project_light(request.project_id).await?;
        let tlen = project.length;
        let circular = project.topology == "circular";
        let primers: Vec<serde_json::Value> = project
            .primers
            .iter()
            .map(|p| {
                let sites: Vec<serde_json::Value> = p
                    .binding_sites
                    .iter()
                    .map(|s| {
                        let mut site = serde_json::json!({
                            "strand": s.strand,
                            "templateStart": s.template_start,
                            "templateEnd": s.template_end,
                        });
                        site_json_to_1based(&mut site, tlen, circular);
                        site
                    })
                    .collect();
                serde_json::json!({
                    "id": p.id,
                    "name": p.name,
                    "type": p.r#type,
                    "seq": p.primer_seq,
                    "bindingSiteCount": p.binding_sites.len(),
                    "sites": sites,
                })
            })
            .collect();
        let mut v = serde_json::json!({ "projectId": id, "primers": primers });
        insert_seq_hashes(
            &mut v,
            &libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type),
        );
        Ok(Json(v))
    }

    pub(crate) async fn find_orfs_impl(
        &self,
        request: FindOrfsRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
        let seq_hashes = self.project_seq_hashes(&id).await;
        let orfs = crate::do_find_orfs(&self.pm, &id, request.min_aa)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;

        if !request.add_as_features.unwrap_or(false) {
            let orfs_json: Vec<serde_json::Value> = orfs.iter().map(feature_json_1based).collect();
            let mut v = serde_json::json!({ "projectId": id, "orfs": orfs_json });
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            return Ok(Json(v));
        }
        self.require_agent_tab(&id).await?;
        if orfs.is_empty() {
            let mut v = serde_json::json!({
                "ok": true,
                "message": "No ORFs found",
                "projectId": id,
            });
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            return Ok(Json(v));
        }
        let min_s = orfs.iter().map(|f| f.start).min().unwrap_or(0);
        let max_e = orfs.iter().map(|f| f.end).max().unwrap_or(0);
        let payload = crate::do_add_features(
            &self.app_handle,
            &self.pm,
            &self.wp,
            &self.agent_tabs,
            None,
            &id,
            orfs,
        )
        .await
        .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            let mut v = fail_envelope(&id, err);
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            return Ok(Json(v));
        }
        let region = self.digest_region(&id, Some((min_s, max_e)), true).await;
        let mut v = ok_envelope(&id, "Added ORFs as CDS features".to_string(), region);
        if let Some(h) = &seq_hashes {
            insert_seq_hashes(&mut v, h);
        }
        Ok(Json(v))
    }

}
