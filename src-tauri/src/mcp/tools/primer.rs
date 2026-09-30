//! Primer MCP tools: add_primer, design_primers, check_primer_binding.

use rmcp::{ErrorData, handler::server::wrapper::Json};
use tauri::Runtime;

use libregene_core::models::Primer;

use crate::mcp::LibreGeneMcp;
use crate::mcp::next_id;
use crate::mcp::support::{fail_envelope, from1, insert_seq_hashes, ok_envelope, site_json_to_1based, to1};
use crate::mcp::types::{AddPrimerRequest, CheckPrimerBindingRequest, DesignPrimersRequest};

/// analyze_mutagenesis reports internal 0-based coordinates; bump the
/// template span, the diff offsets and the CDS codon index to the 1-based
/// inclusive MCP convention. `codonIndex` becomes 1-based within the CDS
/// (then equal to `aaPosition1Based`); `aaPosition1Based`/
/// `aaPositionExcludingMet` are amino-acid numbering (already 1-based
/// conventions) and stay untouched.
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
    }
    v["orientationHint"] = serde_json::json!(orientation_hint(info));
    v
}

/// Plain-language restatement of the mutagenesis strand semantics with the
/// ACTUAL outcome, so a coding-strand/plus-strand slip is called out instead
/// of silently producing the wrong amino acid.
fn orientation_hint(info: &libregene_core::primer::design::MutagenesisAnalysis) -> String {
    let base = format!(
        "mut_seq was applied as the PLUS-strand (top-strand) content of seg {}..{}.",
        info.seg_start + 1,
        info.seg_end + 1
    );
    match &info.cds {
        Some(cds) if cds.strand == "-" => {
            let aa_pos = cds
                .aa_position_excluding_met
                .map(|p| p.to_string())
                .unwrap_or_else(|| cds.aa_position_1_based.to_string());
            format!(
                "{} CDS '{}' is on the MINUS strand: the coding-strand effect is the reverse complement of the plus-strand edit — codonAfter '{}' = {} at aa {}. If {} is NOT the amino acid you intended, you most likely passed CODING-strand sequence as mut_seq; reverse-complement it and retry.",
                base, cds.name, cds.codon_after, cds.aa_after, aa_pos, cds.aa_after
            )
        }
        Some(cds) => {
            let aa_pos = cds
                .aa_position_excluding_met
                .map(|p| p.to_string())
                .unwrap_or_else(|| cds.aa_position_1_based.to_string());
            format!(
                "{} CDS '{}' is on the PLUS strand: the coding-strand codon after the edit is '{}' = {} at aa {}, read directly from the plus-strand edit.",
                base, cds.name, cds.codon_after, cds.aa_after, aa_pos
            )
        }
        None => format!(
            "{} seg is not inside any CDS feature, so no codon-level self-check was possible; verify strand and location via plusContext/minusContext.",
            base
        ),
    }
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
        let clean_seq = match clean_primer_input(&request.name, &request.r#type, &request.seq) {
            Ok(s) => s,
            Err(e) => {
                let mut v = fail_envelope(&id, e);
                if let Some(h) = &seq_hashes {
                    insert_seq_hashes(&mut v, h);
                }
                return Ok(Json(v));
            }
        };
        let primer = Primer {
            id: primer_id.clone(),
            name: request.name,
            r#type: request.r#type,
            primer_seq: clean_seq,
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
        let (sites, region) = {
            let pm = self.pm.read().await;
            let project = pm.get_project_by_id(&id);
            let primer = project.and_then(|p| p.primers.iter().find(|pr| pr.id == primer_id));
            match (project, primer) {
                (Some(p), Some(pr)) => {
                    let sites: Vec<serde_json::Value> = pr
                        .binding_sites
                        .iter()
                        .map(|s| {
                            let mut site = serde_json::json!({
                                "strand": s.strand,
                                "templateStart": s.template_start,
                                "templateEnd": s.template_end,
                                "tm": (s.tm * 10.0).round() / 10.0,
                                "3PrimeMismatch": s.has_3_prime_mismatch,
                                "annealLen": libregene_core::primer::align::anneal_len(
                                    &p.sequence, &p.topology, &pr.primer_seq, s,
                                ),
                            });
                            site_json_to_1based(&mut site, p.length, p.topology == "circular");
                            site
                        })
                        .collect();
                    let region = pr.binding_sites.first().map(|s| {
                        (
                            (s.template_start - 10).max(0),
                            (s.template_end - 1 + 10).min(p.length - 1),
                        )
                    });
                    (sites, region)
                }
                _ => (Vec::new(), None),
            }
        };
        let region_view = match region {
            Some(r) => self.digest_region(&id, Some(r), true).await,
            None => self.digest_region(&id, None, true).await,
        };
        let mut env = ok_envelope(
            &id,
            format!("Added primer {} ({} binding site(s))", name, sites.len()),
            region_view,
        );
        env["bindingSites"] = serde_json::json!(sites);
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
        let (tlen, circular) = {
            let pm = self.pm.read().await;
            let p = pm
                .get_project_by_id(&id)
                .ok_or_else(|| ErrorData::invalid_params("Project not found", None))?;
            (p.length, p.topology == "circular")
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
                Ok(info) => mutation_info = Some(mutagenesis_json_1based(&info)),
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
        let mut v = serde_json::json!({
            "projectId": id,
            "groups": groups,
            "tmBasis": "3' continuous match; 5' tail bases that accidentally match the adjacent template are included in annealLen/Tm (expected for tailed primers — see check_primer_binding per-site alignedTemplate/matchMask)",
        });
        if let Some(info) = mutation_info {
            v["mutation"] = info;
        }
        if mode_is_amplify {
            v["internalSites"] = serde_json::json!(internal_sites);
            if !internal_sites.is_empty() {
                v["warning"] = serde_json::json!(
                    "The enzyme recognition site occurs inside the amplified segment; digestion will cut the product"
                );
            }
            if let Some((ss, se)) = seg_bounds {
                v["orientation"] = serde_json::json!(format!(
                    "Product top strand = template top strand of seg {}..{}: Fwd primes from its 5' (left) end, Rev from its 3' (right) end — primer names follow the template top strand, not any feature's coding strand",
                    ss + 1,
                    se + 1
                ));
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
                                    let note = if f.strand == "-" {
                                        format!(
                                            "CDS '{}' is on the MINUS strand: its coding direction runs opposite to the product top strand — Fwd sits at the CDS 3' end and Rev at the CDS 5' end",
                                            f.name
                                        )
                                    } else {
                                        format!(
                                            "CDS '{}' is on the plus strand: its coding direction matches the product top strand (Fwd at the CDS 5' side, Rev at the 3' side)",
                                            f.name
                                        )
                                    };
                                    serde_json::json!({
                                        "featureId": f.id,
                                        "name": f.name,
                                        "strand": f.strand,
                                        "note": note,
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

    pub(crate) async fn check_primer_binding_impl(
        &self,
        request: CheckPrimerBindingRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let id = self.require_dna_project(request.project_id).await?;
        let seq_hashes = self.project_seq_hashes(&id).await;
        let mut primers: Vec<Primer> = Vec::with_capacity(request.primers.len());
        for p in request.primers {
            let clean_seq = match clean_primer_input(&p.name, &p.r#type, &p.seq) {
                Ok(s) => s,
                Err(e) => {
                    let mut v = fail_envelope(&id, e);
                    if let Some(h) = &seq_hashes {
                        insert_seq_hashes(&mut v, h);
                    }
                    return Ok(Json(v));
                }
            };
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
        let (tlen, circular) = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id)
                .map(|p| (p.length, p.topology == "circular"))
                .ok_or_else(|| ErrorData::invalid_params("Project not found", None))?
        };
        let mut v = payload;
        // The core reports internal 0-based coordinates; convert every site's
        // templateStart/templateEnd to the 1-based inclusive MCP convention.
        if let Some(results) = v.get_mut("results").and_then(|r| r.as_array_mut()) {
            for result in results.iter_mut() {
                if let Some(site) = result.get_mut("site") {
                    if !site.is_null() {
                        site_json_to_1based(site, tlen, circular);
                    }
                }
                if let Some(sites) = result.get_mut("sites").and_then(|s| s.as_array_mut()) {
                    for site in sites.iter_mut() {
                        site_json_to_1based(site, tlen, circular);
                    }
                }
            }
        }
        v["projectId"] = serde_json::json!(id);
        v["tmBasis"] = serde_json::json!(
            "3' continuous match; 5' tail bases that accidentally match the adjacent template are included in annealLen/Tm (expected for tailed primers — see per-site alignedTemplate/matchMask)"
        );
        if let Some(h) = &seq_hashes {
            insert_seq_hashes(&mut v, h);
        }
        Ok(Json(v))
    }

}
