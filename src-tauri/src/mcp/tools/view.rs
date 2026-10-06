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
    primer_site_json, to1, unit_for,
};
use crate::mcp::types::{
    EnzymeListRequest, FindOrfsRequest, FindRestrictionSitesRequest, ListPrimersRequest,
    OverviewRequest, RegionRequest, SequenceRequest,
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
            "codon": h.codon,
            "aminoAcid": h.amino_acid.to_string(),
            "codonBaseIndex": h.codon_base_index,
        })).collect::<Vec<_>>(),
    })
}

/// `"<name>: 5214 bp circular"` (falling back to the file name for a project
/// without a stored name, and to `"5214 bp circular"` when neither exists).
fn project_label(project_id: &str, project: &libregene_core::models::ProjectData) -> String {
    let desc = format!(
        "{} {} {}",
        project.length,
        unit_for(&project.molecule_type),
        project.topology
    );
    let label = if !project.name.is_empty() {
        project.name.clone()
    } else {
        std::path::Path::new(project_id)
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default()
    };
    if label.is_empty() {
        desc
    } else {
        format!("{}: {}", label, desc)
    }
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
                entry["unit"] = serde_json::json!(unit_for(&p.molecule_type));
                insert_seq_hashes(
                    entry,
                    &libregene_core::utils::orientation_hashes(&p.sequence, &p.molecule_type),
                );
            }
        }
        let active_id = pm.active_id().map(|s| s.to_string());
        let count = projects.len();
        Ok(Json(serde_json::json!({
            "ok": true,
            "message": format!("{} project(s) open", count),
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
        let label = project_label(&id, &project);
        let unit = unit_for(&project.molecule_type);
        let opts = DigestOptions {
            max_features: request.max_features,
            feature_filter: request.feature_filter,
            compact_enzymes: false,
            compact_cutters: request.compact_cutters.unwrap_or(true),
            include_auto_annotation: true,
            include_alignment_view: false,
        };
        // Auto-annotation scans the whole feature database — CPU-heavy, so
        // render off the tokio worker.
        let text = tokio::task::spawn_blocking(move || project_digest(&project, &opts, None))
            .await
            .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let mut v = ok_envelope(&id, format!("Overview of {}", label), Some(text));
        v["unit"] = serde_json::json!(unit);
        insert_seq_hashes(&mut v, &hashes);
        Ok(Json(v))
    }

    pub(crate) async fn get_region_view_impl(
        &self,
        request: RegionRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let hashes = libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        let unit = unit_for(&project.molecule_type);
        let tlen = project.length;
        let (start, end) = (request.start, request.end);
        let opts = DigestOptions {
            max_features: request.max_features,
            feature_filter: request.feature_filter,
            compact_enzymes: request.compact.unwrap_or(true),
            compact_cutters: false,
            include_auto_annotation: false,
            include_alignment_view: request.show_alignment_columns.unwrap_or(false),
        };
        let region = (from1(request.start), from1(request.end));
        let text = tokio::task::spawn_blocking(move || project_digest(&project, &opts, Some(region)))
            .await
            .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let mut v = ok_envelope(
            &id,
            format!("Region {}..{} ({} {})", start, end, tlen, unit),
            Some(text),
        );
        v["unit"] = serde_json::json!(unit);
        v["region"] = serde_json::json!({ "start": start, "end": end });
        insert_seq_hashes(&mut v, &hashes);
        Ok(Json(v))
    }

    pub(crate) async fn read_sequence_impl(
        &self,
        request: SequenceRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project_light(request.project_id).await?;
        let unit = unit_for(&project.molecule_type);
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
                "Provide exactly one input form: `start` + `end` (window read); `position`; `featureId` + `featureOffset`; or `featureId` + `aaPosition`".to_string(),
            ));
        }

        if window_active {
            let (s1, e1) = match (request.start, request.end) {
                (Some(s), Some(e)) => (s, e),
                _ => {
                    return Ok(fail(
                        "start and end are both required (1-based inclusive)".to_string(),
                    ))
                }
            };
            let (s, e) = (from1(s1), from1(e1));
            let text = read_sequence(&project, s, e)
                .map_err(|e| ErrorData::invalid_params(e, None))?;
            let bases = libregene_core::digest::read_sequence_bases(&project, s, e)
                .map_err(|e| ErrorData::invalid_params(e, None))?;
            let mut v = serde_json::json!({
                "ok": true,
                "message": format!("Read {} {} at {}..{}", bases.len(), unit, s1, e1),
                "projectId": id,
                "unit": unit,
                "start": s1,
                "end": e1,
                "length": bases.len(),
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
                "Provide exactly one of: `position`; `featureId` + `featureOffset`; or `featureId` + `aaPosition`".to_string(),
            ));
        }

        if let Some(pos1) = request.position {
            if pos1 < 1 || pos1 > len {
                return Ok(fail(
                    format!("position {} out of bounds (1..={})", pos1, len),
                ));
            }
            position = pos1 - 1;
            input_json = serde_json::json!({ "mode": "position", "position": pos1 });
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
                            "mode": "featureOffset",
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
                            "mode": "aminoAcid",
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
                    "Provide exactly one of: `position`; `featureId` + `featureOffset`; or `featureId` + `aaPosition`".to_string(),
                ));
            }
        } else {
            return Ok(fail(
                "Provide exactly one of: `position`; `featureId` + `featureOffset`; or `featureId` + `aaPosition`".to_string(),
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
        let base = sequence[position as usize..position as usize + 1].to_ascii_uppercase();
        let mut v = serde_json::json!({
            "ok": true,
            "message": format!("Position {} = {}", position + 1, base),
            "projectId": id,
            "unit": unit,
            "mode": input_json["mode"],
            "input": input_json,
            "position": position + 1,
            "base": base,
            "features": ctx["features"],
            "translations": ctx["translations"],
            "start": ws + 1,
            "end": we + 1,
            "length": bases.len(),
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

    /// Enzyme-name discovery: the built-in database, filtered and capped.
    /// Name discovery used to be an implicit protocol (probe
    /// find_restriction_sites with a name and read the suggestions out of the
    /// error); this is the explicit, general replacement.
    pub(crate) async fn list_enzymes_impl(
        &self,
        request: EnzymeListRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let query = request
            .query
            .as_deref()
            .map(str::trim)
            .filter(|q| !q.is_empty())
            .map(|q| q.to_lowercase());
        let limit = request.limit.unwrap_or(50).clamp(1, 200);
        let db = libregene_core::enzyme::search::get_db();
        let mut matched: Vec<&libregene_core::enzyme::data::EnzymeRecord> = db
            .enzymes
            .iter()
            .filter(|e| match &query {
                None => true,
                Some(q) => {
                    e.name.to_lowercase().contains(q) || e.site.to_lowercase().contains(q)
                }
            })
            .collect();
        matched.sort_by(|a, b| a.name.cmp(&b.name));
        let total = matched.len();
        let enzymes: Vec<serde_json::Value> = matched
            .into_iter()
            .take(limit)
            .map(|e| serde_json::json!({ "name": e.name, "site": e.site }))
            .collect();
        let count = enzymes.len();
        Ok(Json(serde_json::json!({
            "ok": true,
            "message": match &query {
                Some(q) => format!("{} of {} enzyme(s) match '{}'", count, total, q),
                None => format!("{} of {} enzyme(s)", count, total),
            },
            "query": request.query,
            "total": total,
            "count": count,
            "enzymes": enzymes,
        })))
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
        // unknown to the database (unknown). Unknown names are data, not a
        // call failure: they are reported under `unknownEnzymes` (with
        // near-match suggestions) and the rest of the query still answers.
        // Name discovery belongs to list_enzymes.
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
                let site_count = sites.len();
                serde_json::json!({
                    "name": n,
                    "siteCount": site_count,
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
                            "hasCutsOutsideRecognitionSite": outside,
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
                "siteCount": 0,
                "sites": [],
                "note": "enzyme exists in the enzyme database but has no recognition site on this sequence",
            }));
        }
        enzymes_json.sort_by(|a, b| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")));
        let enzyme_count = enzymes_json.len();
        let mut resp = serde_json::json!({
            "ok": true,
            "message": format!("{} enzyme(s) reported on {} {}", enzyme_count, project.length, unit_for(&project.molecule_type)),
            "projectId": id,
            "unit": unit_for(&project.molecule_type),
            "enzymeCount": enzyme_count,
            "enzymes": enzymes_json,
        });
        insert_seq_hashes(&mut resp, &hashes);
        if !unknown.is_empty() {
            resp["unknownEnzymes"] = unknown
                .iter()
                .map(|(n, sugg)| {
                    serde_json::json!({
                        "name": n,
                        "message": format!("Unknown enzyme '{}': not in the enzyme database", n),
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
        let topology = project.topology.as_str();
        let primers: Vec<serde_json::Value> = project
            .primers
            .iter()
            .map(|p| {
                let sites: Vec<serde_json::Value> = p
                    .binding_sites
                    .iter()
                    .map(|s| primer_site_json(&project.sequence, topology, &p.primer_seq, s, tlen))
                    .collect();
                serde_json::json!({
                    "id": p.id,
                    "name": p.name,
                    "type": p.r#type,
                    "seq": p.primer_seq,
                    "length": p.primer_seq.len(),
                    "bindingSiteCount": p.binding_sites.len(),
                    "sites": sites,
                })
            })
            .collect();
        let count = primers.len();
        let mut v = serde_json::json!({
            "ok": true,
            "message": format!("{} primer(s)", count),
            "projectId": id,
            "unit": unit_for(&project.molecule_type),
            "primerCount": count,
            "primers": primers,
        });
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
        let min_aa = request.min_aa;
        let orfs = crate::do_find_orfs(&self.pm, &id, request.min_aa)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;

        if !request.add_as_features.unwrap_or(false) {
            let orfs_json: Vec<serde_json::Value> = orfs.iter().map(feature_json_1based).collect();
            let count = orfs_json.len();
            let mut v = serde_json::json!({
                "ok": true,
                "message": format!("{} ORF(s) found", count),
                "projectId": id,
                "unit": "bp",
                "minAa": min_aa,
                "orfCount": count,
                "orfs": orfs_json,
            });
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            return Ok(Json(v));
        }
        self.require_agent_tab(&id).await?;
        if orfs.is_empty() {
            let mut v = ok_envelope(&id, "No ORFs found", None);
            v["unit"] = serde_json::json!("bp");
            v["featureCount"] = serde_json::json!(0);
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            return Ok(Json(v));
        }
        let min_s = orfs.iter().map(|f| f.start).min().unwrap_or(0);
        let max_e = orfs.iter().map(|f| f.end).max().unwrap_or(0);
        let feature_count = orfs.len();
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
        let region = self.digest_region(&id, Some((min_s, max_e)), true, false).await;
        let mut v = ok_envelope(&id, format!("Added {} ORF(s) as CDS features", feature_count), region);
        v["unit"] = serde_json::json!("bp");
        v["featureCount"] = serde_json::json!(feature_count);
        if let Some(h) = &seq_hashes {
            insert_seq_hashes(&mut v, h);
        }
        Ok(Json(v))
    }
}
