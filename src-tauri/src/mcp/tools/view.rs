//! Read-only MCP tools: project list/digests, sequence reads,
//! restriction sites, ORFs.

use std::collections::HashMap;

use rmcp::{handler::server::wrapper::Json, ErrorData};
use tauri::Runtime;

use libregene_core::digest::{
    alignment_in_region, cut_flanks, enzyme_in_region, feature_in_region, feature_matches_filter,
    project_digest, read_sequence, DigestOptions,
};
use libregene_core::models::{Enzyme, Feature, ProjectData};

use super::align::{
    alignment_json_1based, alignment_stats_json_1based, filter_alignment_json_focus,
};
use crate::mcp::support::{
    fail_envelope, feature_json_1based, from1, insert_seq_hashes, ok_envelope, primer_json_1based,
    to1, unit_for, MAX_FLANK,
};
use crate::mcp::types::{
    FindOrfsRequest, FindRestrictionSitesRequest, OverviewRequest, RegionRequest, SequenceRequest,
};
use crate::mcp::LibreGeneMcp;

/// Feature/translation hits containing internal 0-based `position`,
/// serialized for read_sequence: every containing feature with its 1-based
/// feature-relative offset, plus CDS/mRNA codon/amino-acid details. Used for
/// the window mode's startContext/endContext and the coordinate modes' hit
/// details.
fn position_context_json(position: i64, sequence: &str, features: &[Feature]) -> serde_json::Value {
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

fn feature_list_json(
    project: &ProjectData,
    filter: Option<&str>,
    max: Option<usize>,
    region: Option<(i64, i64)>,
) -> (Vec<serde_json::Value>, usize) {
    let circular = project.topology == "circular";
    let features: Vec<&Feature> = project
        .features
        .iter()
        .filter(|f| feature_matches_filter(f, filter))
        .filter(|f| region.is_none_or(|(s, e)| feature_in_region(f, s, e, circular)))
        .collect();
    let count = features.len();
    let returned = features
        .into_iter()
        .take(max.unwrap_or(usize::MAX))
        .map(feature_json_1based)
        .collect();
    (returned, count)
}

fn enzyme_site_json(e: &Enzyme, tlen: i64, circular: bool) -> serde_json::Value {
    let wrap = |x: i64| -> i64 {
        if circular {
            x.rem_euclid(tlen)
        } else {
            x
        }
    };
    let rec_start = to1(wrap(e.rec_start));
    let rec_end = to1(wrap(e.rec_end));
    let cuts: Vec<serde_json::Value> = e
        .cut_pairs
        .iter()
        .map(|p| {
            serde_json::json!({
                "topCutIndex": cut_flanks(p.top_cut_index, tlen, circular).0,
                "botCutIndex": cut_flanks(p.bot_cut_index, tlen, circular).0,
            })
        })
        .collect();
    let outside = if circular {
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
}

fn region_restriction_sites(
    project: &ProjectData,
    region: (i64, i64),
    wanted: Option<&[String]>,
) -> Vec<serde_json::Value> {
    let circular = project.topology == "circular";
    let mut groups: Vec<serde_json::Value> = Vec::new();
    let mut by_site: HashMap<String, usize> = HashMap::new();
    for enzyme in project
        .derived
        .enzymes
        .iter()
        .filter(|e| {
            wanted.is_none_or(|names| {
                names.is_empty() || names.iter().any(|name| name.eq_ignore_ascii_case(&e.name))
            })
        })
        .filter(|e| enzyme_in_region(e, region.0, region.1, circular))
    {
        let site = enzyme_site_json(enzyme, project.length, circular);
        let key = serde_json::to_string(&site).unwrap_or_default();
        if let Some(index) = by_site.get(&key).copied() {
            let group = &mut groups[index];
            group["enzymes"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!(enzyme.name));
            group["enzymeCount"] = serde_json::json!(group["enzymes"].as_array().unwrap().len());
        } else {
            let mut group = site;
            group["enzymes"] = serde_json::json!([enzyme.name]);
            group["enzymeCount"] = serde_json::json!(1);
            by_site.insert(key, groups.len());
            groups.push(group);
        }
    }
    for group in &mut groups {
        group["enzymes"]
            .as_array_mut()
            .unwrap()
            .sort_by(|a, b| a.as_str().unwrap_or("").cmp(b.as_str().unwrap_or("")));
    }
    groups.sort_by(|a, b| {
        (a["recStart"].as_i64(), a["recEnd"].as_i64())
            .cmp(&(b["recStart"].as_i64(), b["recEnd"].as_i64()))
    });
    groups
}

impl<R: Runtime> LibreGeneMcp<R> {
    pub(crate) async fn list_workspace_impl(&self) -> Result<Json<serde_json::Value>, ErrorData> {
        let (projects, active_id, count) = {
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
            (projects, active_id, count)
        };
        // Lock order: the pm guard above is already dropped; never hold the
        // workspace lock together with it.
        let workspace: Vec<serde_json::Value> = {
            let ws = self.workspace.read().await;
            ws.iter()
                .map(crate::mcp::workspace::workspace_item_json)
                .collect()
        };
        let wcount = workspace.len();
        Ok(Json(serde_json::json!({
            "ok": true,
            "message": format!("{} project(s) open, {} workspace fragment(s)", count, wcount),
            "projects": projects,
            "activeId": active_id,
            "workspace": workspace,
        })))
    }

    pub(crate) async fn get_project_overview_impl(
        &self,
        request: OverviewRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let hashes =
            libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        let label = project_label(&id, &project);
        let unit = unit_for(&project.molecule_type);
        let include_text = request.include_text.unwrap_or(false);
        let include_enzymes = request.include_enzymes.unwrap_or(false);
        let (features, feature_count) = feature_list_json(
            &project,
            request.feature_filter.as_deref(),
            request.max_features,
            None,
        );
        let primers: Vec<serde_json::Value> = project
            .primers
            .iter()
            .map(|p| primer_json_1based(&project, p, None))
            .collect();
        let alignments: Vec<serde_json::Value> = project
            .alignments
            .iter()
            .map(|a| {
                alignment_stats_json_1based(
                    a,
                    &project.sequence,
                    project.length,
                    project.topology == "circular",
                )
            })
            .collect();
        let mut enzyme_counts: HashMap<&str, usize> = HashMap::new();
        for enzyme in &project.derived.enzymes {
            *enzyme_counts.entry(enzyme.name.as_str()).or_default() += 1;
        }
        let enzyme_count = enzyme_counts.len();
        let mut enzymes: Vec<serde_json::Value> = if include_enzymes {
            enzyme_counts
                .into_iter()
                .map(|(name, count)| serde_json::json!({"name": name, "siteCount": count}))
                .collect()
        } else {
            Vec::new()
        };
        enzymes.sort_by(|a, b| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["name"].as_str().unwrap_or(""))
        });
        let opts = DigestOptions {
            max_features: request.max_features,
            feature_filter: request.feature_filter,
            compact_enzymes: !include_enzymes,
            omit_enzymes: !include_enzymes,
            include_auto_annotation: include_text,
            include_alignment_view: false,
            ..DigestOptions::default()
        };
        let text = if include_text {
            let project = project.clone();
            Some(
                tokio::task::spawn_blocking(move || project_digest(&project, &opts, None))
                    .await
                    .map_err(|e| {
                        ErrorData::internal_error(format!("task join error: {}", e), None)
                    })?
                    .map_err(|e| ErrorData::invalid_params(e, None))?,
            )
        } else {
            None
        };
        let mut v = ok_envelope(&id, format!("Overview of {}", label), text);
        v["unit"] = serde_json::json!(unit);
        v["project"] = serde_json::json!({
            "name": project.name,
            "length": project.length,
            "topology": project.topology,
            "moleculeType": project.molecule_type,
        });
        if let Some((start, end)) = project.roi {
            v["project"]["roi"] = serde_json::json!({"start": start + 1, "end": end + 1});
        }
        v["featureCount"] = serde_json::json!(feature_count);
        v["features"] = serde_json::json!(features);
        v["primerCount"] = serde_json::json!(primers.len());
        v["primers"] = serde_json::json!(primers);
        v["enzymeCount"] = serde_json::json!(enzyme_count);
        if include_enzymes {
            v["enzymes"] = serde_json::json!(enzymes);
        }
        v["alignmentCount"] = serde_json::json!(alignments.len());
        v["alignments"] = serde_json::json!(alignments);
        if project.is_dna() {
            v["methylation"] = serde_json::json!({
                "systems": project.methylation_systems,
                "overlap": project.methylation_overlap,
            });
        }
        insert_seq_hashes(&mut v, &hashes);
        Ok(Json(v))
    }

    pub(crate) async fn get_region_view_impl(
        &self,
        request: RegionRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project(request.project_id).await?;
        let hashes =
            libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        let unit = unit_for(&project.molecule_type);
        let tlen = project.length;
        let circular = project.topology == "circular";
        let (start, end) = (request.start, request.end);
        let region = (from1(start), from1(end));
        let show_alignment_columns = request.show_alignment_columns.unwrap_or(false);
        let include_text = request.include_text.unwrap_or(false) || show_alignment_columns;
        let include_enzymes = request.include_enzymes.unwrap_or(false);
        let compact = request.compact.unwrap_or(true);
        let opts = DigestOptions {
            max_features: request.max_features,
            feature_filter: request.feature_filter.clone(),
            enzyme_filter: request.enzymes.clone(),
            compact_enzymes: compact,
            omit_enzymes: !include_enzymes,
            include_auto_annotation: false,
            include_alignment_view: show_alignment_columns,
        };
        let digest_project = project.clone();
        let text = tokio::task::spawn_blocking(move || {
            project_digest(&digest_project, &opts, Some(region))
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?
        .map_err(|e| ErrorData::invalid_params(e, None))?;
        let (features, feature_count) = feature_list_json(
            &project,
            request.feature_filter.as_deref(),
            request.max_features,
            Some(region),
        );
        let primers: Vec<serde_json::Value> = project
            .primers
            .iter()
            .map(|p| primer_json_1based(&project, p, Some(region)))
            .collect();
        let restriction_sites = if project.is_dna() {
            region_restriction_sites(&project, region, request.enzymes.as_deref())
        } else {
            Vec::new()
        };
        let alignments: Vec<serde_json::Value> = project
            .alignments
            .iter()
            .filter(|a| alignment_in_region(a, region.0, region.1, circular))
            .map(|a| {
                let mut value = alignment_json_1based(a, &project.sequence, tlen, circular, true);
                let (mismatches, insertions, deletions) =
                    filter_alignment_json_focus(&mut value, start, end, tlen, circular);
                value["window"] = serde_json::json!({
                    "mismatchCount": mismatches,
                    "insertionCount": insertions,
                    "deletionCount": deletions,
                });
                value
            })
            .collect();
        let mut v = ok_envelope(
            &id,
            format!("Region {}..{} ({} {})", start, end, tlen, unit),
            include_text.then_some(text),
        );
        v["unit"] = serde_json::json!(unit);
        v["region"] = serde_json::json!({ "start": start, "end": end });
        v["featureCount"] = serde_json::json!(feature_count);
        v["features"] = serde_json::json!(features);
        v["primerCount"] = serde_json::json!(primers.len());
        v["primers"] = serde_json::json!(primers);
        v["restrictionSiteCount"] = serde_json::json!(restriction_sites.len());
        if include_enzymes {
            if compact {
                let summaries: Vec<serde_json::Value> = restriction_sites
                    .iter()
                    .map(|site| {
                        serde_json::json!({
                            "enzymes": site["enzymes"],
                            "enzymeCount": site["enzymeCount"],
                            "recStart": site["recStart"],
                            "recEnd": site["recEnd"],
                        })
                    })
                    .collect();
                v["restrictionSites"] = serde_json::json!(summaries);
            } else {
                v["restrictionSites"] = serde_json::json!(restriction_sites);
            }
        }
        v["alignmentCount"] = serde_json::json!(alignments.len());
        v["alignments"] = serde_json::json!(alignments);
        insert_seq_hashes(&mut v, &hashes);
        Ok(Json(v))
    }

    pub(crate) async fn read_sequence_impl(
        &self,
        request: SequenceRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project_light(request.project_id).await?;
        let unit = unit_for(&project.molecule_type);
        let seq_hashes =
            libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
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
            let text =
                read_sequence(&project, s, e).map_err(|e| ErrorData::invalid_params(e, None))?;
            let bases = libregene_core::digest::read_sequence_bases(&project, s, e)
                .map_err(|e| ErrorData::invalid_params(e, None))?;
            let mut v = serde_json::json!({
                "ok": true,
                "message": format!("Read {} {} at {}..{}", bases.len(), unit, s1, e1),
                "projectId": id,
                "unit": unit,
                "mode": "window",
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
                return Ok(fail(format!(
                    "position {} out of bounds (1..={})",
                    pos1, len
                )));
            }
            position = pos1 - 1;
            input_json = serde_json::json!({ "mode": "position", "position": pos1 });
            codon_positions_opt = None;
        } else if let Some(feature_id) = &request.feature_id {
            let f = features
                .iter()
                .find(|f| &f.id == feature_id)
                .ok_or_else(|| {
                    ErrorData::invalid_params(format!("feature '{}' not found", feature_id), None)
                })?;
            if let Some(offset1) = request.feature_offset {
                match libregene_core::coords::position_from_feature_offset(f, offset1) {
                    Ok(pos0) => {
                        // Feature coordinates are file-derived and not
                        // range-checked at parse time; reject before the
                        // sequence indexing below panics.
                        if pos0 < 0 || pos0 >= len {
                            return Ok(fail(format!(
                                "feature '{}' coordinates fall outside the sequence (length {})",
                                f.name, len
                            )));
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
        let text =
            read_sequence(&project, ws, we).map_err(|e| ErrorData::invalid_params(e, None))?;
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
            v["codonPositions"] =
                serde_json::json!(positions.iter().map(|p| p + 1).collect::<Vec<_>>());
        }
        insert_seq_hashes(&mut v, &seq_hashes);
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
        let hashes =
            libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
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
        // near-match suggestions — this is also the enzyme-name discovery
        // path) and the rest of the query still answers.
        let mut requested: Vec<String> = Vec::new();
        let mut no_site: Vec<String> = Vec::new();
        let mut unknown: Vec<(String, Vec<String>)> = Vec::new();
        if let Some(list) = wanted.as_ref().filter(|l| !l.is_empty()) {
            let names: Vec<&str> = project
                .derived
                .enzymes
                .iter()
                .map(|e| e.name.as_str())
                .collect();
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
        for e in &project.derived.enzymes {
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
                    "sites": sites.iter().map(|e| enzyme_site_json(e, tlen, circular)).collect::<Vec<_>>(),
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
        enzymes_json.sort_by(|a, b| {
            a["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["name"].as_str().unwrap_or(""))
        });
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
        let orfs_json: Vec<serde_json::Value> = orfs.iter().map(feature_json_1based).collect();
        let count = orfs_json.len();
        let unit = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id)
                .map(|p| unit_for(&p.molecule_type))
                .unwrap_or("bp")
        };

        let mut v = serde_json::json!({
            "ok": true,
            "message": format!("{} ORF(s) found; pass selected entries to update_features to annotate them", count),
            "projectId": id,
            "unit": unit,
            "minAa": min_aa,
            "orfCount": count,
            "orfs": orfs_json,
        });
        if let Some(h) = &seq_hashes {
            insert_seq_hashes(&mut v, h);
        }
        Ok(Json(v))
    }
}
