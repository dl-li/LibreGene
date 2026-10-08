//! Primer MCP tools: add_primer, design_primers, test_primers.
//!
//! Both site-reporting tools (add_primer, test_primers) share one
//! binding-site shape (see `support::primer_site_json`), and every Tm is °C
//! rounded to 0.1.

use rmcp::{ErrorData, handler::server::wrapper::Json};
use tauri::Runtime;

use libregene_core::models::Primer;

use crate::mcp::LibreGeneMcp;
use crate::mcp::next_id;
use crate::mcp::support::{
    fail_envelope, from1, insert_seq_hashes, ok_envelope, primer_site_json, push_note,
    push_warning, rename_key, round1, site_json_to_1based, to1, unit_for,
};
use crate::mcp::types::{AddPrimerRequest, DesignPrimersRequest, PrimerInput, TestPrimersRequest};

/// analyze_mutagenesis reports internal 0-based coordinates; bump the
/// template span, the diff offsets and the CDS codon index to the 1-based
/// inclusive MCP convention. `codonIndex` becomes 1-based within the CDS
/// (then equal to `aaPosition1Based`); the dual `aaPositionExcludingMet`
/// numbering is dropped so the response carries one convention only. The
/// block-level `warning` is hoisted to the response's `warnings` array by the
/// caller.
fn mutagenesis_json_1based(info: &libregene_core::primer::design::MutagenesisAnalysis) -> serde_json::Value {
    let mut v = serde_json::to_value(info).unwrap_or_default();
    v["segStart"] = serde_json::json!(to1(info.seg_start));
    v["segEnd"] = serde_json::json!(to1(info.seg_end));
    if let Some(diffs) = v.get_mut("diffs").and_then(|d| d.as_array_mut()) {
        for d in diffs.iter_mut() {
            if let Some(o) = d.get("offset").and_then(|x| x.as_i64()) {
                d["offset"] = serde_json::json!(o + 1);
            }
        }
    }
    if let Some(cds) = v.get_mut("cds") {
        if let Some(ci) = cds.get("codonIndex").and_then(|x| x.as_i64()) {
            cds["codonIndex"] = serde_json::json!(ci + 1);
        }
        // One amino-acid numbering only: codonIndex counts the initiator Met,
        // so literature numbering that skips it is codonIndex - 1 (see the
        // tool description).
        if let Some(obj) = cds.as_object_mut() {
            obj.remove("aaPositionExcludingMet");
        }
    }
    v
}

/// Clean a caller-supplied primer sequence (letters only, uppercase) and
/// validate `type` against the frontend's fwd/rev convention — an empty
/// cleaned sequence would silently persist as a 0-site primer, and an
/// arbitrary type string breaks the UI's fwd/rev rendering.
fn clean_primer_input(name: &str, r#type: &str, seq: &str) -> Result<String, String> {
    let clean: String = seq
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect::<String>()
        .to_uppercase();
    if clean.is_empty() {
        return Err(format!(
            "primer '{}' seq is empty after removing non-letter characters",
            name
        ));
    }
    if !matches!(r#type, "fwd" | "rev") {
        return Err(format!(
            "primer '{}' has invalid type '{}': must be \"fwd\" or \"rev\"",
            name, r#type
        ));
    }
    Ok(clean)
}

/// Look up an enzyme's recognition site by name (case-insensitive); the error
/// lists near matches so the caller can fix the name.
fn resolve_enzyme_site(name: &str) -> Result<String, String> {
    let db = libregene_core::enzyme::search::get_db();
    if let Some(e) = db.enzymes.iter().find(|e| e.name.eq_ignore_ascii_case(name)) {
        return Ok(e.site.to_ascii_uppercase());
    }
    let q = name.to_lowercase();
    let suggestions: Vec<&str> = db
        .enzymes
        .iter()
        .map(|e| e.name.as_str())
        .filter(|n| n.to_lowercase().contains(&q))
        .take(5)
        .collect();
    if suggestions.is_empty() {
        Err(format!("Unknown enzyme '{}'; no similar names in the enzyme database", name))
    } else {
        Err(format!("Unknown enzyme '{}'; similar: {}", name, suggestions.join(", ")))
    }
}

/// Map one shared-core candidate to the unified MCP candidate shape: `id`
/// (stable within the response, e.g. "fwd-1"), `recommended`, and the
/// `...Length`/`gcPercent` naming.
fn candidate_json(candidate: &serde_json::Value, group_type: &str, index: usize, default_index: usize) -> serde_json::Value {
    let mut c = candidate.clone();
    rename_key(&mut c, "tailLen", "tailLength");
    rename_key(&mut c, "annealLen", "annealLength");
    rename_key(&mut c, "designedAnnealLen", "designedAnnealLength");
    if let Some(gc) = c.get("gc").and_then(|v| v.as_f64()) {
        c["gcPercent"] = serde_json::json!(round1(gc));
        if let Some(obj) = c.as_object_mut() {
            obj.remove("gc");
        }
    }
    if let Some(tm) = c.get("tm").and_then(|v| v.as_f64()) {
        c["tm"] = serde_json::json!(round1(tm));
    }
    if let Some(tm) = c.get("designedTm").and_then(|v| v.as_f64()) {
        c["designedTm"] = serde_json::json!(round1(tm));
    }
    c["id"] = serde_json::json!(format!("{}-{}", group_type, index + 1));
    c["recommended"] = serde_json::json!(index == default_index);
    c
}

/// Map one primer-design group to `{name, type, recommendedIndex, candidates}`
/// with unified candidate fields.
fn group_json(group: &libregene_core::primer::design::PrimerGroup) -> serde_json::Value {
    let default_index = group.default_index;
    let candidates: Vec<serde_json::Value> = group
        .candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let raw = serde_json::to_value(c).unwrap_or_default();
            candidate_json(&raw, &group.r#type, i, default_index)
        })
        .collect();
    serde_json::json!({
        "name": group.name,
        "type": group.r#type,
        "recommendedIndex": default_index,
        "candidates": candidates,
    })
}

/// The best binding site of a result entry on the wanted strand (sites are
/// best-first). `want` is 1 (forward) or -1 (reverse).
fn best_site_on_strand(result: &serde_json::Value, want: i64) -> Option<&serde_json::Value> {
    result
        .get("sites")
        .and_then(|s| s.as_array())
        .and_then(|sites| sites.iter().find(|s| s["strand"].as_i64() == Some(want)))
}

impl<R: Runtime> LibreGeneMcp<R> {
    pub(crate) async fn add_primer_impl(
        &self,
        request: AddPrimerRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
        self.require_agent_tab(&id).await?;
        let seq_hashes = self.project_seq_hashes(&id).await;
        let primer_id = next_id("primer");
        let name = request.name.clone();
        let primer_type = request.r#type.clone();
        let fail = |msg: String| -> Json<serde_json::Value> {
            let mut v = fail_envelope(&id, msg);
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            Json(v)
        };
        let raw_seq = match (request.seq, request.hash) {
            (Some(s), None) => s,
            (None, Some(hash)) => {
                match crate::mcp::workspace::resolve_workspace_hash(&self.pm, &self.workspace, &hash).await {
                    Ok(r) => r.sequence,
                    Err(e) => return Ok(fail(e)),
                }
            }
            _ => {
                return Ok(fail(
                    "Provide exactly one of `seq` (plain text) or `hash` (workspace hash)".to_string(),
                ));
            }
        };
        let clean_seq = match clean_primer_input(&request.name, &request.r#type, &raw_seq) {
            Ok(s) => s,
            Err(e) => return Ok(fail(e)),
        };
        let primer = Primer {
            id: primer_id.clone(),
            name: request.name,
            r#type: request.r#type,
            primer_seq: clean_seq.clone(),
            binding_sites: Vec::new(),
        };
        let payload = crate::do_add_primer(&self.app_handle, &self.pm, &self.wp, &self.agent_tabs, None, &id, primer)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;
        if let Some(err) = Self::payload_error(&payload) {
            let mut v = fail_envelope(&id, err);
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            return Ok(Json(v));
        }
        let (sites, region, unit) = {
            let pm = self.pm.read().await;
            match pm.get_project_by_id(&id) {
                Some(p) => {
                    let sites: Vec<serde_json::Value> = p
                        .primers
                        .iter()
                        .find(|pr| pr.id == primer_id)
                        .map(|pr| {
                            pr.binding_sites
                                .iter()
                                .map(|s| {
                                    primer_site_json(
                                        &p.sequence,
                                        &p.topology,
                                        &pr.primer_seq,
                                        s,
                                        p.length,
                                    )
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let region = p
                        .primers
                        .iter()
                        .find(|pr| pr.id == primer_id)
                        .and_then(|pr| pr.binding_sites.first())
                        .map(|s| {
                            (
                                (s.template_start - 10).max(0),
                                (s.template_end - 1 + 10).min(p.length - 1),
                            )
                        });
                    let unit = unit_for(&p.molecule_type);
                    match region {
                        Some(r) => (sites, Some(r), unit),
                        None => (sites, None, unit),
                    }
                }
                None => (Vec::new(), None, "bp"),
            }
        };
        let region_view = match region {
            Some(r) => self.digest_region(&id, Some(r), true, false).await,
            None => self.digest_region(&id, None, true, false).await,
        };
        let mut env = ok_envelope(
            &id,
            format!("Added primer {} ({} binding site(s))", name, sites.len()),
            region_view,
        );
        env["unit"] = serde_json::json!(unit);
        env["primerId"] = serde_json::json!(primer_id);
        env["name"] = serde_json::json!(name);
        env["type"] = serde_json::json!(primer_type);
        env["seq"] = serde_json::json!(clean_seq);
        env["length"] = serde_json::json!(clean_seq.len());
        env["bindingSiteCount"] = serde_json::json!(sites.len());
        env["sites"] = serde_json::json!(sites);
        if let Some(h) = &seq_hashes {
            insert_seq_hashes(&mut env, h);
        }
        Ok(Json(env))
    }

    pub(crate) async fn design_primers_impl(
        &self,
        request: DesignPrimersRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
        let seq_hashes = self.project_seq_hashes(&id).await;
        let fail = |msg: String| -> Json<serde_json::Value> {
            let mut v = fail_envelope(&id, msg);
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            Json(v)
        };
        let mode = request.mode.clone();
        // Parameters that only apply to another mode are reported instead of
        // being silently dropped (a caller that misread the mode would
        // otherwise wonder why its names/enzymes never showed up).
        let mut ignored: Vec<&str> = Vec::new();
        if mode != "oepcr"
            && (request.name1.is_some() || request.name2.is_some() || request.seg2.is_some())
        {
            ignored.push("name1/name2/seg2 apply to oepcr mode only — ignored");
        }
        if mode != "amplify"
            && (request.fwd_enzyme.is_some()
                || request.rev_enzyme.is_some()
                || request.protect_bases.is_some())
        {
            ignored.push("fwdEnzyme/revEnzyme/protectBases apply to amplify mode only — ignored");
        }
        if mode != "mutagenesis"
            && (request.mut_seq.is_some() || request.site_name.is_some() || request.arm_len.is_some())
        {
            ignored.push("mutSeq/siteName/armLen apply to mutagenesis mode only — ignored");
        }
        let seg = request.seg.map(|s| libregene_core::models::Segment {
            start: from1(s.start),
            end: from1(s.end),
            color: None,
        });
        let seg2 = request.seg2.map(|s| libregene_core::models::Segment {
            start: from1(s.start),
            end: from1(s.end),
            color: None,
        });

        // Validate seg/seg2 against the project: the design engine slices
        // with modulo/clamping instead of erroring, so an out-of-bounds
        // segment would silently yield garbage candidates. seg/seg2 are
        // 0-based here; messages are 1-based inclusive like every other tool.
        let (tlen, circular, unit) = {
            let pm = self.pm.read().await;
            let p = pm
                .get_project_by_id(&id)
                .ok_or_else(|| ErrorData::invalid_params("Project not found", None))?;
            (p.length, p.topology == "circular", unit_for(&p.molecule_type))
        };
        for (label, s) in [("seg", &seg), ("seg2", &seg2)] {
            let Some(s) = s else { continue };
            if s.start < 0 || s.end < 0 || s.start >= tlen || s.end >= tlen {
                return Ok(fail(format!(
                    "{} {}..{} out of bounds for sequence of length {} (1-based inclusive)",
                    label,
                    s.start + 1,
                    s.end + 1,
                    tlen
                )));
            }
            if s.start > s.end && (!circular || request.mode == "mutagenesis") {
                return Ok(fail(format!(
                    "{} {}..{}: start > end wraps the origin — only allowed on circular templates in amplify/oepcr mode",
                    label,
                    s.start + 1,
                    s.end + 1
                )));
            }
        }

        let mut fwd_tail = None;
        let mut rev_tail = None;
        let mut enzyme_sites: Vec<(String, String)> = Vec::new();
        if request.mode == "amplify" && (request.fwd_enzyme.is_some() || request.rev_enzyme.is_some())
        {
            let protect = libregene_core::primer::design::protect_sequence(
                request.protect_bases.unwrap_or(3),
            );
            for (enzyme, slot) in [
                (&request.fwd_enzyme, &mut fwd_tail),
                (&request.rev_enzyme, &mut rev_tail),
            ] {
                if let Some(name) = enzyme {
                    match resolve_enzyme_site(name) {
                        Ok(site) => {
                            *slot = Some(format!("{}{}", protect, site));
                            enzyme_sites.push((name.clone(), site));
                        }
                        Err(e) => return Ok(fail(e)),
                    }
                }
            }
        }

        // amplify + enzyme tails: warn when the recognition site also occurs
        // INSIDE the amplified segment (digestion would cut the product).
        let mut internal_sites: Vec<serde_json::Value> = Vec::new();
        if request.mode == "amplify" && !enzyme_sites.is_empty() {
            if let Some(seg_ref) = seg.as_ref() {
                let (sequence, topology) = {
                    let pm = self.pm.read().await;
                    let p = pm
                        .get_project_by_id(&id)
                        .ok_or_else(|| ErrorData::invalid_params("Project not found", None))?;
                    (p.sequence.clone(), p.topology.clone())
                };
                let len = sequence.len() as i64;
                let (s, e) = (seg_ref.start, seg_ref.end);
                let amplicon: Option<String> = if s >= 0 && e < len && s <= e {
                    Some(sequence[s as usize..=e as usize].to_string())
                } else if topology == "circular" && s >= 0 && e < len {
                    Some(format!("{}{}", &sequence[s as usize..], &sequence[..=e as usize]))
                } else {
                    None
                };
                if let Some(amplicon) = amplicon {
                    for (enzyme_name, site) in &enzyme_sites {
                        for m in libregene_core::search::find_seq_matches(&amplicon, site) {
                            internal_sites.push(serde_json::json!({
                                "enzyme": enzyme_name,
                                "start": (s + m.start) % len + 1,
                                "end": (s + m.end) % len + 1,
                                "strand": m.strand,
                            }));
                        }
                    }
                }
            }
        }

        let mut mutation_info = None;
        let mut mutation_warning = None;
        if request.mode == "mutagenesis" {
            let (sequence, features) = {
                let pm = self.pm.read().await;
                let p = pm
                    .get_project_by_id(&id)
                    .ok_or_else(|| ErrorData::invalid_params("Project not found", None))?;
                (p.sequence.clone(), p.features.clone())
            };
            let seg_ref = seg.as_ref().ok_or_else(|| {
                ErrorData::invalid_params("seg required for mutagenesis", None)
            })?;
            match libregene_core::primer::design::analyze_mutagenesis(
                &sequence,
                seg_ref,
                request.mut_seq.as_deref().unwrap_or(""),
                &features,
            ) {
                Ok(info) => {
                    if let Some(w) = info.warning.clone() {
                        mutation_warning = Some(w);
                    }
                    mutation_info = Some(mutagenesis_json_1based(&info));
                }
                Err(e) => {
                    // analyze_mutagenesis reports internal 0-based seg
                    // coordinates; restate them 1-based inclusive for the
                    // agent (bounds are pre-validated above, so this covers
                    // the length/identity/diff-count errors).
                    let e = e.replace(
                        &format!("seg {}..{}", seg_ref.start, seg_ref.end),
                        &format!("seg {}..{} (1-based inclusive)", seg_ref.start + 1, seg_ref.end + 1),
                    );
                    let mut v = fail_envelope(&id, e);
                    if let Some(h) = &seq_hashes {
                        insert_seq_hashes(&mut v, h);
                    }
                    let lo = seg_ref.start.max(0) as usize;
                    let hi = ((seg_ref.end + 1).min(sequence.len() as i64)) as usize;
                    if lo < hi {
                        v["templateBases"] =
                            serde_json::json!(sequence[lo..hi].to_ascii_uppercase());
                    }
                    return Ok(Json(v));
                }
            }
        }

        let mode_is_amplify = request.mode == "amplify";
        let seg_bounds = seg.as_ref().map(|s| (s.start, s.end));
        let groups = crate::do_design_primer_candidates(
            &self.pm,
            &id,
            request.mode,
            seg,
            seg2,
            request.name,
            request.name1,
            request.name2,
            request.site_name,
            request.target_tm,
            request.overlap_len,
            request.arm_len,
            request.mut_seq,
            fwd_tail,
            rev_tail,
            request.na_conc,
            request.mg_conc,
            request.dntp_conc,
            request.tris_conc,
            request.primer_conc,
        )
        .await
        .map_err(|e| ErrorData::invalid_params(e, None))?;
        let groups_json: Vec<serde_json::Value> = groups.iter().map(group_json).collect();
        let mut v = serde_json::json!({
            "ok": true,
            "message": format!("{} primer group(s) designed ({} mode)", groups_json.len(), mode),
            "projectId": id,
            "unit": unit,
            "mode": mode,
            "groups": groups_json,
        });
        for note in ignored {
            push_note(&mut v, note);
        }
        if let Some(info) = mutation_info.as_ref() {
            v["mutation"] = info.clone();
        }
        if let Some(w) = mutation_warning {
            push_warning(&mut v, w);
        }
        if mode_is_amplify {
            v["internalSites"] = serde_json::json!(internal_sites);
            v["internalSiteCount"] = serde_json::json!(internal_sites.len());
            if !internal_sites.is_empty() {
                push_warning(
                    &mut v,
                    "The enzyme recognition site occurs inside the amplified segment; digestion will cut the product",
                );
            }
            if let Some((ss, se)) = seg_bounds {
                v["product"] = serde_json::json!({
                    "start": to1(ss),
                    "end": to1(se),
                    "length": (se - ss).rem_euclid(tlen) + 1,
                });
                let cds_overlaps: Vec<serde_json::Value> = {
                    let pm = self.pm.read().await;
                    match pm.get_project_by_id(&id) {
                        Some(p) => {
                            let pieces = if p.topology == "circular" && ss > se {
                                vec![(ss, p.length - 1), (0, se)]
                            } else {
                                vec![(ss, se)]
                            };
                            p.features
                                .iter()
                                .filter(|f| f.ftype.eq_ignore_ascii_case("cds"))
                                .filter(|f| {
                                    let spans: Vec<(i64, i64)> = if f.segments.is_empty() {
                                        vec![(f.start, f.end)]
                                    } else {
                                        f.segments.iter().map(|s| (s.start, s.end)).collect()
                                    };
                                    pieces
                                        .iter()
                                        .any(|&(ps, pe)| spans.iter().any(|&(s, e)| s <= pe && ps <= e))
                                })
                                .map(|f| {
                                    serde_json::json!({
                                        "featureId": f.id,
                                        "name": f.name,
                                        "strand": f.strand,
                                    })
                                })
                                .collect()
                        }
                        None => Vec::new(),
                    }
                };
                if !cds_overlaps.is_empty() {
                    v["cdsOverlaps"] = serde_json::json!(cds_overlaps);
                }
            }
        }
        if let Some(h) = &seq_hashes {
            insert_seq_hashes(&mut v, h);
        }
        Ok(Json(v))
    }

    /// Test ad-hoc primers against the project's sequence (read-only, DNA
    /// only, nothing persisted); exactly one binding fwd + one binding rev
    /// additionally report the pair's `amplicon`.
    pub(crate) async fn test_primers_impl(
        &self,
        request: TestPrimersRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        if request.primers.is_empty() {
            let id = self.require_dna_project(request.project_id).await?;
            let mut v = fail_envelope(&id, "primers must name at least one primer to test");
            if let Some(h) = self.project_seq_hashes(&id).await {
                insert_seq_hashes(&mut v, &h);
            }
            return Ok(Json(v));
        }
        self.check_ad_hoc_primers(request.project_id, request.primers).await
    }

    async fn check_ad_hoc_primers(
        &self,
        project_id: String,
        inputs_req: Vec<PrimerInput>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(project_id).await?;
        let seq_hashes = self.project_seq_hashes(&id).await;
        let mut primers: Vec<Primer> = Vec::with_capacity(inputs_req.len());
        let mut inputs: Vec<(String, String, usize)> = Vec::with_capacity(inputs_req.len());
        let fail = |msg: String| -> Json<serde_json::Value> {
            let mut v = fail_envelope(&id, msg);
            if let Some(h) = &seq_hashes {
                insert_seq_hashes(&mut v, h);
            }
            Json(v)
        };
        for p in inputs_req {
            let raw_seq = match (p.seq, p.hash) {
                (Some(s), None) => s,
                (None, Some(hash)) => {
                    match crate::mcp::workspace::resolve_workspace_hash(&self.pm, &self.workspace, &hash).await {
                        Ok(r) => r.sequence,
                        Err(e) => return Ok(fail(e)),
                    }
                }
                _ => {
                    return Ok(fail(format!(
                        "primer '{}': provide exactly one of `seq` (plain text) or `hash` (workspace hash)",
                        p.name
                    )));
                }
            };
            let clean_seq = match clean_primer_input(&p.name, &p.r#type, &raw_seq) {
                Ok(s) => s,
                Err(e) => return Ok(fail(e)),
            };
            inputs.push((p.name.clone(), p.r#type.clone(), clean_seq.len()));
            primers.push(Primer {
                id: p.name.clone(),
                name: p.name,
                r#type: p.r#type,
                primer_seq: clean_seq,
                binding_sites: Vec::new(),
            });
        }
        let payload = crate::do_check_primers_binding(&self.pm, &id, primers)
            .await
            .map_err(|e| ErrorData::internal_error(e, None))?;
        let (tlen, topology, unit) = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id)
                .map(|p| (p.length, p.topology.clone(), unit_for(&p.molecule_type)))
                .ok_or_else(|| ErrorData::invalid_params("Project not found", None))?
        };
        let circular = topology == "circular";
        // The core reports internal 0-based coordinates and its own field
        // names; convert every site to the unified 1-based site shape, and add
        // the caller-facing identity of each primer (the core echoes only the
        // id it received).
        let mut results = payload;
        if let Some(arr) = results.get_mut("results").and_then(|r| r.as_array_mut()) {
            for (i, result) in arr.iter_mut().enumerate() {
                if let Some((name, ty, plen)) = inputs.get(i) {
                    result["name"] = serde_json::json!(name);
                    result["type"] = serde_json::json!(ty);
                    result["length"] = serde_json::json!(plen);
                }
                for key in ["site", "sites"] {
                    match result.get_mut(key) {
                        Some(serde_json::Value::Array(sites)) => {
                            for site in sites.iter_mut() {
                                remap_site(site, tlen, circular);
                            }
                        }
                        Some(site) if !site.is_null() => remap_site(site, tlen, circular),
                        _ => {}
                    }
                }
            }
        }
        let checked = results["results"].clone();
        let mut v = serde_json::json!({
            "ok": true,
            "message": format!("Checked {} primer(s) for binding", inputs.len()),
            "projectId": id,
            "unit": unit,
            "primerCount": inputs.len(),
            "primers": checked,
        });
        // A single fwd + single rev primer define an amplicon: report its size
        // so the caller does not have to export the product to measure it.
        if let Some(amp) = amplicon_json(&v["primers"], &inputs, tlen, circular) {
            v["amplicon"] = amp;
        }
        if let Some(h) = &seq_hashes {
            insert_seq_hashes(&mut v, h);
        }
        Ok(Json(v))
    }
}

/// Shared-core site → unified MCP site shape (1-based inclusive).
fn remap_site(site: &mut serde_json::Value, tlen: i64, circular: bool) {
    rename_key(site, "annealLen", "annealLength");
    rename_key(site, "mismatchedTail", "tailLength");
    if let Some(tm) = site.get("tm").and_then(|v| v.as_f64()) {
        site["tm"] = serde_json::json!(round1(tm));
    }
    site_json_to_1based(site, tlen, circular);
}

/// `{forwardStart, reverseEnd, length, note}` for the fwd/rev primer pair, when
/// the request holds exactly one primer of each type and both bind the strand
/// their role needs. The best (highest-Tm) site per role is used.
fn amplicon_json(
    results: &serde_json::Value,
    inputs: &[(String, String, usize)],
    tlen: i64,
    circular: bool,
) -> Option<serde_json::Value> {
    if inputs.len() != 2 {
        return None;
    }
    let fwd_idx = inputs.iter().position(|(_, t, _)| t == "fwd")?;
    let rev_idx = inputs.iter().position(|(_, t, _)| t == "rev")?;
    let results = results.as_array()?;
    let fwd_start = best_site_on_strand(results.get(fwd_idx)?, 1)?["templateStart"].as_i64()?;
    let rev_end = best_site_on_strand(results.get(rev_idx)?, -1)?["templateEnd"].as_i64()?;
    let length = if circular {
        (rev_end - fwd_start).rem_euclid(tlen) + 1
    } else if rev_end >= fwd_start {
        rev_end - fwd_start + 1
    } else {
        return None;
    };
    Some(serde_json::json!({
        "forwardStart": fwd_start,
        "reverseEnd": rev_end,
        "length": length,
        "note": "Length of the fwd/rev primer pair's PCR product (top strand), derived from the best binding site of each primer; binding sites are best-first (Tm descending)",
    }))
}
