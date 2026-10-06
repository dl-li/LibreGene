//! convert_sequence MCP tool + the conversion engine (input resolution,
//! codon optimization, output writing).

use rmcp::{ErrorData, handler::server::wrapper::Json};
use tauri::Runtime;

use libregene_core::models::{Feature, ProjectData};

use crate::mcp::LibreGeneMcp;
use crate::mcp::support::{insert_seq_hashes, round1};
use crate::mcp::types::{ConvertItem, ConvertSequenceRequest, OptimizeInput};

/// Validate the input-mode combination and resolve it to exactly one
/// [`OptimizeInput`]. Error messages name the offending combination.
pub(crate) fn resolve_optimize_input(
    project_id: Option<&str>,
    feature_id: Option<&str>,
    sequence: Option<&str>,
    input_path: Option<&str>,
) -> Result<OptimizeInput, String> {
    match (sequence, input_path) {
        (Some(_), Some(_)) => Err(
            "provide exactly one input: `projectId` (+`featureId`), `sequence`, or `inputPath` — not both `sequence` and `inputPath`"
                .to_string(),
        ),
        (Some(seq), None) => {
            if project_id.is_some() {
                return Err(
                    "`projectId` cannot be combined with `sequence`; use exactly one input mode"
                        .to_string(),
                );
            }
            if feature_id.is_some() {
                return Err(
                    "`featureId` is only valid with `projectId` (project mode) or an `inputPath` file that has features"
                        .to_string(),
                );
            }
            Ok(OptimizeInput::Sequence(seq.to_string()))
        }
        (None, Some(path)) => {
            if project_id.is_some() {
                return Err(
                    "`projectId` cannot be combined with `inputPath`; use exactly one input mode"
                        .to_string(),
                );
            }
            Ok(OptimizeInput::File {
                path: path.to_string(),
                feature_id: feature_id.map(str::to_string),
            })
        }
        (None, None) => {
            let project_id = project_id.ok_or_else(|| {
                "`projectId` is required in project mode (or pass `sequence` or `inputPath` for standalone input)"
                    .to_string()
            })?;
            let feature_id = feature_id.ok_or_else(|| {
                "`featureId` is required in project mode (or pass `sequence` or `inputPath` for standalone input)"
                    .to_string()
            })?;
            Ok(OptimizeInput::Project {
                project_id: project_id.to_string(),
                feature_id: feature_id.to_string(),
            })
        }
    }
}

/// Strip whitespace/digits, uppercase, and require a valid DNA coding
/// sequence: only A/C/G/T and a length divisible by 3 (a trailing stop codon
/// is fine — it is just another codon).
pub(crate) fn clean_coding_sequence(seq: &str) -> Result<String, String> {
    let cleaned: String = seq
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_ascii_digit())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if cleaned.is_empty() {
        return Err("sequence is empty".to_string());
    }
    for (i, c) in cleaned.char_indices() {
        if c != 'A' && c != 'C' && c != 'G' && c != 'T' {
            return Err(format!(
                "invalid base '{}' at position {} in sequence (expected A/C/G/T)",
                c, i
            ));
        }
    }
    if !cleaned.len().is_multiple_of(3) {
        return Err(format!(
            "sequence length {} not divisible by 3 (expected a complete coding sequence)",
            cleaned.len()
        ));
    }
    Ok(cleaned)
}

/// Strip whitespace/digits and uppercase, requiring every letter in
/// `alphabet` ("ACGT" for DNA, "ACGU" for RNA). No length constraint.
fn clean_na_sequence(seq: &str, alphabet: &str) -> Result<String, String> {
    let cleaned: String = seq
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_ascii_digit())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if cleaned.is_empty() {
        return Err("sequence is empty".to_string());
    }
    for (i, c) in cleaned.char_indices() {
        if !alphabet.contains(c) {
            return Err(format!(
                "invalid base '{}' at position {} in sequence (expected one of {})",
                c, i, alphabet
            ));
        }
    }
    Ok(cleaned)
}

/// The shared convert_sequence preview fields for codon-optimizing items
/// (identical across all input modes). The backend reports repairs/unresolved
/// as raw 0-based values; MCP bumps them to the 1-based inclusive convention
/// here (codonIndex 1 = first codon; unresolved ranges are 1-based inclusive
/// base offsets within the optimized sequence).
pub(crate) fn codon_preview_json(
    result: &libregene_core::codon::OptimizeResult,
    aa: &str,
    codon_count: usize,
    method: &str,
    species: &str,
) -> serde_json::Value {
    let repairs: Vec<serde_json::Value> = result
        .repairs
        .iter()
        .map(|r| {
            serde_json::json!({
                "codonIndex": r.codon_index + 1,
                "old": r.old,
                "new": r.new,
                "reason": r.reason,
            })
        })
        .collect();
    let unresolved: Vec<serde_json::Value> = result
        .unresolved
        .iter()
        .map(|u| {
            // Backend format: "<reason> <start>..<end>" (0-based inclusive).
            match u.rsplit_once(' ') {
                Some((reason, range)) => match range.split_once("..") {
                    Some((a, b)) => match (a.parse::<i64>(), b.parse::<i64>()) {
                        (Ok(s), Ok(e)) => {
                            serde_json::json!(format!("{} {}..{}", reason, s + 1, e + 1))
                        }
                        _ => serde_json::json!(u),
                    },
                    None => serde_json::json!(u),
                },
                None => serde_json::json!(u),
            }
        })
        .collect();
    serde_json::json!({
        "aa": aa,
        "codonCount": codon_count,
        "newCodons": result.new_codons,
        "caiBefore": result.cai_before,
        "caiAfter": result.cai_after,
        "gcPercentBefore": round1(result.gc_before * 100.0),
        "gcPercentAfter": round1(result.gc_after * 100.0),
        "repairs": repairs,
        "repairCount": repairs.len(),
        "unresolved": unresolved,
        "method": method,
        "species": species,
    })
}

/// Write a conversion result to `output_path`. The extension decides the
/// format: .gbk/.gb/.genbank → GenBank of `molecule` ("dna" | "rna" |
/// "protein"; `source` replaces the default minimal project when the input
/// file already carried features), .gpt → protein GenBank, .fa/.fasta/.txt →
/// bare sequence text. `cds_name` labels the whole-length CDS feature of a
/// minimal project (typically the source file stem); it falls back to the
/// output file stem. Returns the written path.
pub(crate) fn write_convert_output(
    output_path: &str,
    sequence: &str,
    molecule: &str,
    source: Option<&ProjectData>,
    cds_name: Option<&str>,
) -> Result<String, String> {
    let ext = crate::validate_user_path(output_path, crate::CONVERT_OUTPUT_EXTS)?;
    let path = std::path::Path::new(output_path);
    match ext.as_str() {
        "gbk" | "gb" | "genbank" => {
            let project = match source {
                Some(p) => p.clone(),
                None => minimal_na_project(output_path, sequence, molecule, cds_name),
            };
            libregene_core::file_io::gbk::write_gbk(&project, path)
                .map_err(|e| format!("failed to write {}: {}", output_path, e))?;
        }
        "gpt" => {
            if molecule != "protein" {
                return Err(format!(
                    ".gpt is a protein GenBank format; the {} output cannot be written as .gpt (use .gbk/.fa/.fasta/.txt)",
                    molecule
                ));
            }
            let project = minimal_protein_project(output_path, sequence, cds_name);
            libregene_core::file_io::gpt::write_gpt(&project, path)
                .map_err(|e| format!("failed to write {}: {}", output_path, e))?;
        }
        "fa" | "fasta" | "txt" => {
            std::fs::write(path, format!("{}\n", sequence))
                .map_err(|e| format!("failed to write {}: {}", output_path, e))?;
        }
        other => {
            return Err(format!(
                "unsupported output extension '.{}' (allowed: gbk, gb, genbank, gpt, fa, fasta, txt)",
                other
            ))
        }
    }
    Ok(output_path.to_string())
}

fn minimal_na_project(
    output_path: &str,
    seq: &str,
    molecule: &str,
    cds_name: Option<&str>,
) -> ProjectData {
    let name = output_project_name(output_path);
    let len = seq.len() as i64;
    ProjectData {
        features: vec![whole_cds_feature(len, cds_name.unwrap_or(&name))],
        name,
        sequence: seq.to_string(),
        length: len,
        topology: "linear".to_string(),
        molecule_type: molecule.to_string(),
        ..Default::default()
    }
}

fn minimal_protein_project(output_path: &str, aa: &str, cds_name: Option<&str>) -> ProjectData {
    minimal_na_project(output_path, aa, "protein", cds_name)
}

pub(crate) fn output_project_name(output_path: &str) -> String {
    std::path::Path::new(output_path)
        .file_stem()
        .map(|s| s.to_string_lossy().replace(' ', "_"))
        .unwrap_or_else(|| "optimized".to_string())
}

fn whole_cds_feature(len: i64, name: &str) -> Feature {
    Feature {
        id: "cds".to_string(),
        name: name.to_string(),
        start: 0,
        end: len - 1,
        color: "#60A5FA".to_string(),
        ftype: "CDS".to_string(),
        segments: Vec::new(),
        strand: "+".to_string(),
        notes: String::new(),
        translation: String::new(),
        qualifiers: Vec::new(),
    }
}

impl<R: Runtime> LibreGeneMcp<R> {
    /// Convert one batch item of `convert_sequence`: resolve the input mode,
    /// infer/validate the from→to pair, run the conversion and build the
    /// per-item result JSON (the caller adds `index` / `ok`).
    async fn convert_one(&self, item: &ConvertItem) -> Result<serde_json::Value, String> {
        let mode = match &item.hash {
            Some(hash) => {
                if item.project_id.is_some()
                    || item.sequence.is_some()
                    || item.input_path.is_some()
                    || item.feature_id.is_some()
                {
                    return Err(
                        "`hash` cannot be combined with projectId/featureId/sequence/inputPath — use exactly one input mode"
                            .to_string(),
                    );
                }
                let resolved =
                    crate::mcp::workspace::resolve_workspace_hash(&self.pm, &self.workspace, hash)
                        .await?;
                OptimizeInput::Workspace {
                    sequence: resolved.sequence,
                    molecule_type: resolved.molecule_type,
                }
            }
            None => resolve_optimize_input(
                item.project_id.as_deref(),
                item.feature_id.as_deref(),
                item.sequence.as_deref(),
                item.input_path.as_deref(),
            )?,
        };

        let apply = item.apply.unwrap_or(false);
        if !matches!(mode, OptimizeInput::Project { .. }) && apply && item.output_path.is_none() {
            return Err(
                "apply=true is only meaningful in project mode; in sequence/inputPath mode pass `outputPath` to write the result to a file (or set apply=false)"
                    .to_string(),
            );
        }
        if matches!(mode, OptimizeInput::Project { .. }) && item.output_path.is_some() {
            return Err(
                "outputPath is only supported in sequence/inputPath modes; in project mode use apply=true to write the optimized CDS back into the project, then save_file to export a file"
                    .to_string(),
            );
        }
        if let Some(op) = &item.output_path {
            crate::validate_user_path(op, crate::CONVERT_OUTPUT_EXTS)
                .map_err(|e| format!("invalid outputPath: {}", e))?;
            if !item.overwrite.unwrap_or(false) && std::path::Path::new(op).exists() {
                return Err(format!(
                    "{} already exists — pass overwrite: true to replace it, or choose a different outputPath",
                    op
                ));
            }
        }

        match mode {
            OptimizeInput::Project { project_id, feature_id } => {
                if let Some(f) = &item.from {
                    if f != "dna" {
                        return Err(format!(
                            "project mode is dna→dna codon optimization; from=\"{}\" is not supported (projects hold the molecule they hold — export a region with save_file and use inputPath/sequence for {} input)",
                            f, f
                        ));
                    }
                }
                let to = item.to.clone().unwrap_or_else(|| "dna".to_string());
                if to != "dna" {
                    return Err(format!(
                        "project mode only supports dna→dna codon optimization (to=\"{}\" requested); for conversions export the region with save_file first, then use inputPath",
                        to
                    ));
                }
                let species = item.species.clone().ok_or_else(|| {
                    "species is required in project mode (codon optimization needs a built-in codon-usage key, e.g. \"e_coli\" — see this tool's description for the full key list)"
                        .to_string()
                })?;
                self.convert_project_item(item, project_id, feature_id, &species, apply)
                    .await
            }
            OptimizeInput::Sequence(seq) => {
                let from = item.from.clone().unwrap_or_else(|| "dna".to_string());
                let to = default_to(item.to.as_deref(), &from);
                check_conversion(&from, &to, item)?;
                require_species_for(&from, &to, item)?;
                self.convert_sequence_input(item, seq, &from, &to).await
            }
            OptimizeInput::Workspace { sequence, molecule_type } => {
                let from = item.from.clone().unwrap_or(molecule_type);
                let to = default_to(item.to.as_deref(), &from);
                check_conversion(&from, &to, item)?;
                require_species_for(&from, &to, item)?;
                self.convert_sequence_input(item, sequence, &from, &to).await
            }
            OptimizeInput::File { path, feature_id } => {
                self.convert_file_input(item, path, feature_id).await
            }
        }
    }

    /// Project mode: codon-optimize a CDS/mRNA feature inside an open DNA
    /// project (preview by default, write-back on apply=true).
    async fn convert_project_item(
        &self,
        item: &ConvertItem,
        project_id: String,
        feature_id: String,
        species: &str,
        apply: bool,
    ) -> Result<serde_json::Value, String> {
        let id = self.resolve_project_id(project_id).await.map_err(|e| e.message.to_string())?;
        if apply {
            self.require_agent_tab(&id).await.map_err(|e| e.message.to_string())?;
        }
        let project = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(&id)
                .cloned()
                .ok_or_else(|| format!("Project not found: {}", id))?
        };
        if !project.is_dna() {
            return Err(format!(
                "convert_sequence project mode re-encodes a CDS feature inside a DNA project; a {} project has no coding DNA to re-encode — pass `sequence` or `inputPath` instead (a protein .gpt/.prot file or sequence with from=\"protein\" is reverse-translated to optimized DNA)",
                project.molecule_type
            ));
        }
        let method = item.method.clone().unwrap_or_else(|| "use_best_codon".to_string());
        let f_id = feature_id.clone();
        let sp = species.to_string();
        let m = method.clone();
        let os = item.original_species.clone();
        let aes = item.avoid_enzyme_sites.clone();
        let (new_sequence, result, coding) = tokio::task::spawn_blocking(move || {
            crate::codon_optimize(&project, &f_id, &sp, &m, None, os.as_deref(), aes, None)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))??;

        let mut v = codon_preview_json(&result, &coding.aa, coding.codons.len(), &method, species);
        v["ok"] = serde_json::json!(true);
        v["from"] = serde_json::json!("dna");
        v["to"] = serde_json::json!("dna");
        v["length"] = serde_json::json!(coding.codons.len() * 3);
        v["projectId"] = serde_json::json!(id);
        v["message"] = serde_json::json!(format!(
            "Codon optimization preview for {} ({}): CAI {:.3} → {:.3}, GC {:.1}% → {:.1}%, {} repairs, {} unresolved",
            feature_id,
            species,
            result.cai_before,
            result.cai_after,
            result.gc_before * 100.0,
            result.gc_after * 100.0,
            result.repairs.len(),
            result.unresolved.len(),
        ));
        if apply {
            let payload = crate::do_update_sequence(
                &self.app_handle,
                &self.pm,
                &self.wp,
                &self.agent_tabs,
                None,
                id.clone(),
                new_sequence,
                None,
            )
            .await
            .map_err(|e| e.to_string())?;
            if let Some(err) = Self::payload_error(&payload) {
                return Err(err);
            }
            if let Some(rv) = self.digest_feature_region(&id, &feature_id).await {
                v["text"] = serde_json::json!(rv);
            }
            v["message"] = serde_json::json!(format!(
                "Optimized CDS {} ({}): CAI {:.3} → {:.3}, {} repairs, {} unresolved",
                feature_id,
                species,
                result.cai_before,
                result.cai_after,
                result.repairs.len(),
                result.unresolved.len(),
            ));
        }
        // After apply this is the post-mutation hash; a preview leaves the
        // sequence unchanged, so the same read serves both.
        if let Some(h) = self.project_seq_hashes(&id).await {
            insert_seq_hashes(&mut v, &h);
        }
        Ok(v)
    }

    /// Standalone `sequence` input: clean per `from`, run the from→to
    /// conversion, optionally write the result to a file.
    async fn convert_sequence_input(
        &self,
        item: &ConvertItem,
        sequence: String,
        from: &str,
        to: &str,
    ) -> Result<serde_json::Value, String> {
        let optimize = from == "dna" && to == "dna" && wants_optimization(item);
        let cleaned = match from {
            "dna" if optimize => clean_coding_sequence(&sequence)?,
            "dna" => clean_na_sequence(&sequence, "ACGT")?,
            "rna" => clean_na_sequence(&sequence, "ACGU")?,
            _ => sequence.clone(),
        };
        let species = item.species.clone();
        let method = item.method.clone().unwrap_or_else(|| "use_best_codon".to_string());
        let os = item.original_species.clone();
        let aes = item.avoid_enzyme_sites.clone();
        let rev = item.rev_comp.unwrap_or(false);
        let f = from.to_string();
        let t = to.to_string();
        let conv = tokio::task::spawn_blocking(move || {
            convert_sequence_text(&cleaned, &f, &t, optimize, rev, species, &method, os, aes)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))??;

        let mut v = standalone_result_json(&conv, from, to);
        // Hashes describe the INPUT sequence (whitespace/case-insensitive, so
        // hashing the raw text equals hashing the cleaned form).
        insert_seq_hashes(&mut v, &libregene_core::utils::orientation_hashes(&sequence, from));
        if let Some(op) = &item.output_path {
            let written = write_convert_output(op, &conv.sequence, to, None, None)?;
            v["path"] = serde_json::json!(written);
        }
        Ok(v)
    }

    /// Standalone `input_path` input: parse the file (its molecule type
    /// defaults `from`), run the conversion, optionally write the result.
    async fn convert_file_input(
        &self,
        item: &ConvertItem,
        path: String,
        feature_id: Option<String>,
    ) -> Result<serde_json::Value, String> {
        crate::validate_user_path(&path, crate::SEQ_EXTS)
            .map_err(|e| format!("invalid inputPath: {}", e))?;
        let p = path.clone();
        let hint = item.from.clone();
        let project = tokio::task::spawn_blocking(move || {
            libregene_core::file_io::parse_file_with_molecule_type(
                std::path::Path::new(&p),
                hint.as_deref(),
            )
            .map_err(|e| {
                format!(
                    "failed to read {} (supported: .gbk/.gb/.genbank, .dna/.rna/.prot, .gpt, .fa/.fasta, .ab1): {}",
                    p, e
                )
            })
        })
        .await
        .map_err(|e| format!("task join error: {}", e))??;

        let file_mol = if project.molecule_type.is_empty() {
            "dna"
        } else {
            project.molecule_type.as_str()
        };
        let from = match &item.from {
            Some(f) => {
                if f != file_mol {
                    return Err(format!(
                        "input file {} is a {} sequence but from=\"{}\" was given — drop `from` to use the file's molecule type",
                        path, file_mol, f
                    ));
                }
                f.clone()
            }
            None => file_mol.to_string(),
        };
        let to = default_to(item.to.as_deref(), &from);
        check_conversion(&from, &to, item)?;
        require_species_for(&from, &to, item)?;

        let species = item.species.clone();
        let method = item.method.clone().unwrap_or_else(|| "use_best_codon".to_string());
        let os = item.original_species.clone();
        let aes = item.avoid_enzyme_sites.clone();
        let rev = item.rev_comp.unwrap_or(false);

        // Feature-CDS codon optimization inside a DNA file: write-back
        // through the template so the full sequence (CDS replaced) survives.
        if from == "dna" && to == "dna" && feature_id.is_some() && wants_optimization(item) {
            let fid = feature_id.unwrap();
            let fid_label = fid.clone();
            let sp = species.clone().expect("require_species_for gates optimization");
            let m = method.clone();
            let proj = project.clone();
            let (new_sequence, result, coding) = tokio::task::spawn_blocking(move || {
                crate::codon_optimize(&proj, &fid, &sp, &m, None, os.as_deref(), aes, None)
            })
            .await
            .map_err(|e| format!("task join error: {}", e))??;
            let message = format!(
                "Codon optimization for {} (file input, {}): CAI {:.3} → {:.3}, {} repairs, {} unresolved",
                fid_label,
                species.clone().unwrap_or_default(),
                result.cai_before,
                result.cai_after,
                result.repairs.len(),
                result.unresolved.len(),
            );
            let mut v = codon_preview_json(
                &result,
                &coding.aa,
                coding.codons.len(),
                &method,
                species.as_deref().unwrap_or(""),
            );
            v["from"] = serde_json::json!("dna");
            v["to"] = serde_json::json!("dna");
            v["sequence"] = serde_json::json!(new_sequence);
            v["length"] = serde_json::json!(new_sequence.len());
            v["message"] = serde_json::json!(message);
            insert_seq_hashes(
                &mut v,
                &libregene_core::utils::orientation_hashes(&project.sequence, "dna"),
            );
            if let Some(op) = &item.output_path {
                let mut source_project = project.clone();
                source_project.sequence = new_sequence.clone();
                source_project.length = new_sequence.len() as i64;
                let cds_name = std::path::Path::new(&path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().replace(' ', "_"));
                let written =
                    write_convert_output(op, &new_sequence, "dna", Some(&source_project), cds_name.as_deref())?;
                v["path"] = serde_json::json!(written);
            }
            return Ok(v);
        }
        if feature_id.is_some() {
            return Err(
                "featureId is only meaningful for dna→dna codon optimization (pass `species` to optimize, or drop `featureId` to convert the whole file sequence)"
                    .to_string(),
            );
        }

        let raw = project.sequence.to_ascii_uppercase();
        let optimize = from == "dna" && to == "dna" && wants_optimization(item);
        let cleaned = match from.as_str() {
            "dna" if optimize => clean_coding_sequence(&raw)
                .map_err(|e| format!("invalid file sequence: {}", e))?,
            "dna" => clean_na_sequence(&raw, "ACGT")
                .map_err(|e| format!("invalid file sequence: {}", e))?,
            "rna" => clean_na_sequence(&raw, "ACGU")
                .map_err(|e| format!("invalid file sequence: {}", e))?,
            _ => raw,
        };
        let f = from.clone();
        let t = to.clone();
        let conv = tokio::task::spawn_blocking(move || {
            convert_sequence_text(&cleaned, &f, &t, optimize, rev, species, &method, os, aes)
        })
        .await
        .map_err(|e| format!("task join error: {}", e))??;

        let mut v = standalone_result_json(&conv, &from, &to);
        // Hashes describe the input file's parsed sequence.
        insert_seq_hashes(
            &mut v,
            &libregene_core::utils::orientation_hashes(&project.sequence, &from),
        );
        if let Some(op) = &item.output_path {
            // Label the minimal project's CDS after the source file (e.g.
            // CAR.gpt → a "CAR" CDS), saving a rename step downstream.
            let cds_name = std::path::Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().replace(' ', "_"));
            let written =
                write_convert_output(op, &conv.sequence, &to, None, cds_name.as_deref())?;
            v["path"] = serde_json::json!(written);
        }
        Ok(v)
    }
}

/// The outcome of a standalone (sequence/file) conversion: the converted
/// sequence plus, for codon-optimizing paths, the optimizer preview fields.
struct Conversion {
    sequence: String,
    preview: Option<serde_json::Value>,
    message: String,
    /// Informational strings surfaced as the item's `notes` array.
    notes: Vec<String>,
}

/// `to` defaults: reverse translation for a protein input, otherwise a
/// molecule-preserving conversion.
fn default_to(to: Option<&str>, from: &str) -> String {
    match to {
        Some(t) => t.to_string(),
        None if from == "protein" => "dna".to_string(),
        None => from.to_string(),
    }
}

/// The item asks for codon optimization (dna→dna) when any optimizer
/// parameter is present.
fn wants_optimization(item: &ConvertItem) -> bool {
    item.species.is_some()
        || item.method.is_some()
        || item.original_species.is_some()
        || item.avoid_enzyme_sites.is_some()
}

/// Validate a from→to pair and its revComp combination (per item).
fn check_conversion(from: &str, to: &str, item: &ConvertItem) -> Result<(), String> {
    for m in [from, to] {
        if !matches!(m, "dna" | "rna" | "protein") {
            return Err(format!(
                "invalid molecule type \"{}\" (expected \"dna\" | \"rna\" | \"protein\")",
                m
            ));
        }
    }
    if from == "protein" && to == "protein" {
        return Err("protein→protein conversion is not supported".to_string());
    }
    if item.rev_comp.unwrap_or(false) && (from == "protein" || to == "protein") {
        return Err(
            "revComp is only meaningful for nucleotide→nucleotide conversions (a protein has no complement)"
                .to_string(),
        );
    }
    if item.rev_comp.unwrap_or(false) && from == "dna" && to == "dna" && wants_optimization(item) {
        return Err("revComp cannot be combined with codon optimization".to_string());
    }
    Ok(())
}

/// Codon-optimizing paths (dna→dna with optimizer parameters, protein→dna/rna
/// reverse translation) need a codon usage table.
fn require_species_for(from: &str, to: &str, item: &ConvertItem) -> Result<(), String> {
    let optimizing =
        from == "protein" || (from == "dna" && to == "dna" && wants_optimization(item));
    if optimizing && item.species.is_none() {
        return Err(
            "species is required for codon optimization / reverse translation (a built-in key, e.g. \"e_coli\" — see this tool's description for the full key list)"
                .to_string(),
        );
    }
    Ok(())
}

/// Run a standalone sequence conversion (shared by `sequence` and
/// `input_path` modes; `input` is already cleaned, in the `from` alphabet).
fn convert_sequence_text(
    input: &str,
    from: &str,
    to: &str,
    optimize: bool,
    rev_comp: bool,
    species: Option<String>,
    method: &str,
    original_species: Option<String>,
    avoid_enzyme_sites: Option<Vec<String>>,
) -> Result<Conversion, String> {
    if from == "protein" {
        let sp = species.expect("require_species_for gates protein input");
        let aa: String = input
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        let table = crate::codon_usage_table(&sp, None)?;
        let opts =
            crate::codon_optimize_options(method, original_species.as_deref(), avoid_enzyme_sites, None)?;
        let result = libregene_core::codon::optimize_from_aa(&aa, &table, &opts)?;
        let dna: String = result.new_codons.concat();
        let out = if to == "rna" {
            libregene_core::utils::to_rna(&dna)
        } else {
            dna
        };
        let unit = if to == "rna" { "nt RNA" } else { "bp DNA" };
        let message = format!(
            "Reverse translation ({} → {}, {}): {} aa → {} {}, {} repairs, {} unresolved",
            from,
            to,
            sp,
            aa.chars().count(),
            out.len(),
            unit,
            result.repairs.len(),
            result.unresolved.len(),
        );
        let notes = if aa.ends_with('*') {
            Vec::new()
        } else {
            vec![
                "Input protein has no trailing '*' stop codon, so the output carries no stop codon either — append one explicitly if the construct needs it".to_string(),
            ]
        };
        return Ok(Conversion {
            sequence: out,
            preview: Some(codon_preview_json(&result, &aa, aa.chars().count(), method, &sp)),
            message,
            notes,
        });
    }
    if optimize {
        let sp = species.expect("require_species_for gates codon optimization");
        let codons: Vec<String> = input
            .as_bytes()
            .chunks(3)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect();
        let table = crate::codon_usage_table(&sp, None)?;
        let opts =
            crate::codon_optimize_options(method, original_species.as_deref(), avoid_enzyme_sites, None)?;
        let result = libregene_core::codon::optimize_codons(&codons, &table, &opts);
        let aa: String = codons
            .iter()
            .map(|c| table.aa_of.get(c).copied().unwrap_or('?'))
            .collect();
        let optimized: String = result.new_codons.concat();
        let message = format!(
            "Codon optimization ({}): CAI {:.3} → {:.3}, GC {:.1}% → {:.1}%, {} repairs, {} unresolved",
            sp,
            result.cai_before,
            result.cai_after,
            result.gc_before * 100.0,
            result.gc_after * 100.0,
            result.repairs.len(),
            result.unresolved.len(),
        );
        return Ok(Conversion {
            sequence: optimized,
            preview: Some(codon_preview_json(&result, &aa, codons.len(), method, &sp)),
            message,
            notes: Vec::new(),
        });
    }
    let dna = if from == "rna" {
        libregene_core::utils::to_dna(input)
    } else {
        input.to_string()
    };
    let dna = if rev_comp {
        libregene_core::utils::reverse_complement(&dna)
    } else {
        dna
    };
    let out = match to {
        "rna" => libregene_core::utils::to_rna(&dna),
        "protein" => String::from_utf8(libregene_core::translate::translate_nt(dna.as_bytes()))
            .map_err(|e| e.to_string())?,
        _ => dna,
    };
    let unit = if to == "protein" { "aa" } else { "nt" };
    let message = format!(
        "Converted {} → {}{}: {} {}",
        from,
        to,
        if rev_comp { " (reverse-complemented)" } else { "" },
        out.len(),
        unit
    );
    Ok(Conversion { sequence: out, preview: None, message, notes: Vec::new() })
}

/// The per-item result JSON shared by the sequence/input_path modes.
fn standalone_result_json(conv: &Conversion, from: &str, to: &str) -> serde_json::Value {
    let mut v = conv.preview.clone().unwrap_or_else(|| serde_json::json!({}));
    v["ok"] = serde_json::json!(true);
    v["from"] = serde_json::json!(from);
    v["to"] = serde_json::json!(to);
    v["sequence"] = serde_json::json!(conv.sequence);
    v["length"] = serde_json::json!(conv.sequence.len());
    v["message"] = serde_json::json!(conv.message);
    if !conv.notes.is_empty() {
        v["notes"] = serde_json::json!(conv.notes);
    }
    v
}

impl<R: Runtime> LibreGeneMcp<R> {
    pub(crate) async fn convert_sequence_impl(
        &self,
        request: ConvertSequenceRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let ConvertSequenceRequest { items, single } = request;
        let items = match items {
            Some(items) if items.is_empty() => {
                return Err(ErrorData::invalid_params(
                    "items must not be empty — give 1-64 conversion items",
                    None,
                ));
            }
            Some(items) => items,
            None => {
                if single.project_id.is_some() || single.sequence.is_some() || single.input_path.is_some() || single.hash.is_some() {
                    vec![single]
                } else {
                    return Err(ErrorData::invalid_params(
                        "items is required: a batch of 1-64 conversion items (a single conversion may put the item fields at the top level instead — one of projectId / sequence / inputPath / hash)",
                        None,
                    ));
                }
            }
        };
        if items.len() > 64 {
            return Err(ErrorData::invalid_params(
                format!("items is capped at 64 entries per call (got {})", items.len()),
                None,
            ));
        }

        let mut results = Vec::with_capacity(items.len());
        let mut ok_count = 0usize;
        for (i, item) in items.iter().enumerate() {
            match self.convert_one(item).await {
                Ok(mut v) => {
                    v["index"] = serde_json::json!(i);
                    v["ok"] = serde_json::json!(true);
                    ok_count += 1;
                    results.push(v);
                }
                Err(e) => {
                    results.push(serde_json::json!({
                        "index": i,
                        "ok": false,
                        "message": e,
                    }));
                }
            }
        }
        let total = results.len();
        let failed = total - ok_count;
        let message = if failed == 0 {
            format!("Converted {} item(s)", ok_count)
        } else if ok_count == 0 {
            let detail = results
                .iter()
                .map(|r| format!("[{}] {}", r["index"], r["message"].as_str().unwrap_or_default()))
                .collect::<Vec<_>>()
                .join("; ");
            format!("All {} item(s) failed: {}", total, detail)
        } else {
            format!("Converted {} of {} item(s); {} failed", ok_count, total, failed)
        };
        Ok(Json(serde_json::json!({
            "ok": ok_count > 0,
            "message": message,
            "resultCount": total,
            "okCount": ok_count,
            "failedCount": failed,
            "results": results,
        })))
    }
}
