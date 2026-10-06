//! Embedded MCP (Model Context Protocol) server for LibreGene.
//!
//! Exposes tools over Streamable HTTP on `127.0.0.1:8766` so an external LLM
//! agent can operate the app like a real user. Mutations go through the same
//! shared cores as the Tauri commands (`crate::do_*`), so recompute, dirty
//! marking and `broadcast_project()` behave identically and the UI updates
//! live.
//!
//! ## Response envelope
//!
//! Every tool answers with one shape — see `support.rs` for the serializers:
//! `{ok, message, projectId?, unit?, text?, sequenceHash?, revCompHash?, ...}`
//! - `ok` — true on success, false when the domain rejected the request
//!   (bad range, unknown enzyme, failed guard, ...). A domain failure keeps
//!   the same keys plus diagnostics, so callers parse one shape either way.
//! - `message` — one-line human summary.
//! - `projectId` — the addressed project (omitted by `list_projects`;
//!   per item in `convert_sequence`).
//! - `unit` — `bp` | `nt` | `aa`, present whenever lengths of a project
//!   molecule are reported.
//! - `text` — human-readable rendering: a compact multi-section digest for
//!   overview/region/mutation responses, a coordinate-ruled window for
//!   `read_sequence`. `textBefore` is the pre-edit digest.
//! - `warnings` / `notes` — arrays of strings flagging something possibly
//!   wrong / informing, present only when non-empty.
//! - Only "the request cannot be executed at all" failures (unknown project,
//!   project not bound as an agent tab, DNA-only tool on another molecule
//!   type, internal errors) are MCP protocol errors instead.
//!
//! Naming: camelCase everywhere (inputs and outputs), 1-based inclusive
//! coordinates; spans are `start`/`end`, single coordinates `position`,
//! feature-relative offsets `offset`, lengths `...Length`, counts `...Count`,
//! detail lists `...Details`. Fractions (`identity`, `cai*`) are 0–1;
//! percentages (`gcPercent`) 0–100 with one decimal; `tm` is °C rounded to
//! 0.1.
//!
//! ## Sequence-change detection
//!
//! Every project-targeting tool response carries `sequenceHash`/`revCompHash`
//! — 7-hex hashes of the current biological sequence and its reverse
//! complement (case-insensitive, annotations/primers/whitespace ignored;
//! `revCompHash` is null for proteins). Comparing them across calls detects
//! any sequence edit; a project and its reverse-complemented file share one
//! (sequenceHash, revCompHash) pair. Text digests carry the same values as a
//! `SEQHASH:` header line.
//!
//! ## Coordinates
//!
//! This MCP layer is agent-facing, so all coordinates in tool inputs and
//! outputs are **1-based inclusive** (the GenBank convention); the internal
//! model and the shared `crate::do_*` cores stay **0-based inclusive**, and
//! this module converts at the boundary (`to1`/`from1`):
//! - internal inclusive [s, e] ↔ interface [s+1, e+1]
//! - a primer site's internal 0-based-EXCLUSIVE `template_end` equals the
//!   1-based inclusive end of the site, so its value crosses the boundary
//!   unchanged (only `template_start` shifts by one)
//! - an enzyme cut at internal 0-based index C (severing between bases C-1
//!   and C) is described as "between the 1-based bases C and C+1" and rendered
//!   `C^C+1` (`cut_notation`; a cut at the origin of a circular molecule is
//!   `len^1`)
//! - a pure insertion into `edit_sequence` before base N is `start=N,
//!   end=N-1`; ranges must not wrap
//! - circular sequences allow `start > end` to wrap the origin for reads
//!   (values are 1-based)

mod auth;
mod server;
mod support;
mod tools;
mod types;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rmcp::{
    ErrorData, ServerHandler,
    handler::server::wrapper::{Json, Parameters},
    tool, tool_handler, tool_router,
};
use tokio::sync::RwLock;
use tauri::{AppHandle, Emitter, Runtime};

use libregene_core::digest::{DigestOptions, project_digest};
use libregene_core::models::{ProjectData, Segment};
use libregene_core::project::ProjectManager;

// Names mcp/tests.rs reaches through `use super::*;` (the test module globs
// this module's scope); some are not used by mod.rs itself.
#[allow(unused_imports)]
use rmcp::schemars;
#[allow(unused_imports)]
use libregene_core::digest::read_sequence;
#[allow(unused_imports)]
use libregene_core::models::{Enzyme, Feature, Primer, PrimerBindingSite};

#[allow(unused_imports)]
pub(crate) use tools::*;
pub(crate) use support::*;
pub(crate) use types::*;

// Public surface parity with the pre-split module: lib.rs names
// `mcp::McpServer`; `McpConfig` stays reachable at `crate::mcp::McpConfig`.
#[allow(unused_imports)]
pub use server::{McpConfig, McpServer};

/// Loopback port for the embedded MCP server (settings toggle comes later).
pub const MCP_PORT: u16 = 8766;

// ---------------------------------------------------------------------------
// Server handler
// ---------------------------------------------------------------------------

pub struct LibreGeneMcp<R: Runtime> {
    app_handle: AppHandle<R>,
    pm: Arc<RwLock<ProjectManager>>,
    wp: Arc<RwLock<HashMap<String, String>>>,
    agent_tabs: crate::AgentTabs,
}

static ID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn next_id(prefix: &str) -> String {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let n = ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}_{}_{}", prefix, millis, n)
}

/// Window-label sanitizer: keep only `[A-Za-z0-9-_]`; every other character
/// (path separators, '.', spaces, parentheses, ...) becomes '_' so a file
/// path never produces an invalid Tauri window label.
pub(crate) fn sanitize_window_label(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl<R: Runtime> LibreGeneMcp<R> {
    pub fn new(
        app_handle: AppHandle<R>,
        pm: Arc<RwLock<ProjectManager>>,
        wp: Arc<RwLock<HashMap<String, String>>>,
        agent_tabs: crate::AgentTabs,
    ) -> Self {
        Self { app_handle, pm, wp, agent_tabs }
    }

    /// Resolving a project also re-locks any agent tab bound to it — the user
    /// may unlock the tab, but the next tool call on the project locks it
    /// again.
    async fn resolve_project_id(&self, project_id: String) -> Result<String, ErrorData> {
        crate::lock_agent_tab_for_project(&self.app_handle, &self.agent_tabs, &project_id).await;
        Ok(project_id)
    }

    /// Clone the project's data out of the lock (and re-lock its agent tab).
    async fn resolve_project(&self, project_id: String) -> Result<(String, ProjectData), ErrorData> {
        self.resolve_project_impl(project_id, false).await
    }

    /// Same as resolve_project but strips the enzyme list — the single
    /// heaviest field on large plasmids — from the clone. Only for tools
    /// that never render a digest or consult restriction sites.
    async fn resolve_project_light(
        &self,
        project_id: String,
    ) -> Result<(String, ProjectData), ErrorData> {
        self.resolve_project_impl(project_id, true).await
    }

    async fn resolve_project_impl(
        &self,
        project_id: String,
        strip_enzymes: bool,
    ) -> Result<(String, ProjectData), ErrorData> {
        let project = {
            let pm = self.pm.read().await;
            let p = pm.get_project_by_id(&project_id).cloned().ok_or_else(|| {
                ErrorData::invalid_params(format!("Project not found: {}", project_id), None)
            })?;
            if strip_enzymes {
                ProjectData { enzymes: Vec::new(), ..p }
            } else {
                p
            }
        };
        crate::lock_agent_tab_for_project(&self.app_handle, &self.agent_tabs, &project_id).await;
        Ok((project_id, project))
    }

    /// Mutating tools may only operate on projects bound as MCP agent tabs,
    /// so the user's own projects stay untouched. Read-only tools are
    /// unrestricted; `open_project` performs the binding.
    async fn require_agent_tab(&self, project_id: &str) -> Result<(), ErrorData> {
        let at = self.agent_tabs.read().await;
        if at.contains_key(project_id) {
            return Ok(());
        }
        Err(ErrorData::invalid_params(
            format!(
                "Project '{}' is not bound as an MCP agent tab (it was opened by the user, not via MCP open_project). Mutating tools refuse to operate on projects the user opened — copy the file (e.g. bash `cp`) to a new path and open_project the copy.",
                project_id
            ),
            None,
        ))
    }

    /// Reuse the agent tab binding of an already-loaded project (re-locking
    /// it), or reject when the project is loaded but NOT bound — that means
    /// the user opened it, and their projects stay under user control.
    /// Shared by open_project's fast path and the lost-race path after its
    /// atomic check-and-load.
    async fn reuse_agent_tab_or_reject(&self, id: &str) -> Result<Json<serde_json::Value>, ErrorData> {
        // Emit agent-tab-lock only on an unlocked → locked transition
        // (same semantics as lock_agent_tab_for_project).
        enum Reuse {
            Relocked,
            AlreadyLocked,
            NotBound,
        }
        let reuse = {
            let mut at = self.agent_tabs.write().await;
            match at.get_mut(id) {
                Some(meta) if meta.locked => Reuse::AlreadyLocked,
                Some(meta) => {
                    meta.locked = true;
                    Reuse::Relocked
                }
                None => Reuse::NotBound,
            }
        };
        match reuse {
            Reuse::Relocked | Reuse::AlreadyLocked => {
                if matches!(reuse, Reuse::Relocked) {
                    let _ = self.app_handle.emit(
                        "agent-tab-lock",
                        serde_json::json!({ "projectId": id, "locked": true }),
                    );
                }
                let mut v = serde_json::json!({
                    "ok": true,
                    "message": format!("Project '{}' is already open and bound as your agent tab (re-locked)", id),
                    "projectId": id,
                    "locked": true,
                    "reused": true,
                });
                let unit = {
                    let pm = self.pm.read().await;
                    pm.get_project_by_id(id)
                        .map(|p| support::unit_for(&p.molecule_type))
                };
                if let Some(u) = unit {
                    v["unit"] = serde_json::json!(u);
                }
                if let Some(h) = self.project_seq_hashes(id).await {
                    insert_seq_hashes(&mut v, &h);
                }
                Ok(Json(v))
            }
            Reuse::NotBound => Err(ErrorData::invalid_params(
                format!(
                    "Project '{}' is already open and was NOT opened via MCP open_project (it was opened by the user, or in a separate window). To work on a copy, copy the file with bash `cp` to a new path and open_project the copy.",
                    id
                ),
                None,
            )),
        }
    }

    async fn project_summary(&self, project_id: &str) -> Option<String> {
        let pm = self.pm.read().await;
        let p = pm.get_project_by_id(project_id)?;
        let unit = support::unit_for(&p.molecule_type);
        let desc = format!("{} {} {}", p.length, unit, p.topology);
        let label = if !p.name.is_empty() {
            p.name.clone()
        } else {
            std::path::Path::new(project_id)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default()
        };
        Some(if label.is_empty() {
            desc
        } else {
            format!("{}: {}", label, desc)
        })
    }

    /// (sequenceHash, revCompHash) of the project's CURRENT in-memory
    /// sequence, re-read from pm — call after a mutation so the hash reflects
    /// the edit. Never call while holding a pm guard (lock recursion).
    async fn project_seq_hashes(&self, project_id: &str) -> Option<(String, Option<String>)> {
        let pm = self.pm.read().await;
        pm.get_project_by_id(project_id)
            .map(|p| libregene_core::utils::orientation_hashes(&p.sequence, &p.molecule_type))
    }

    /// Resolve the project and reject non-DNA projects for DNA-only tools.
    async fn require_dna_project(&self, project_id: String) -> Result<String, ErrorData> {
        let (id, project) = self.resolve_project_light(project_id).await?;
        if !project.is_dna() {
            return Err(ErrorData::invalid_params(
                format!(
                    "This tool only supports DNA projects; project '{}' is a {} project",
                    id, project.molecule_type
                ),
                None,
            ));
        }
        Ok(id)
    }

    /// Text digest of `region` (internal 0-based inclusive, may wrap on
    /// circular) or the whole project when `None`. `compact` collapses the
    /// enzyme cut list into a count line (mutation tools use it to keep
    /// the response `text` small). `alignment_columns` additionally emits the
    /// per-read ALIGNMENT VIEW column block (add_alignment's deliverable);
    /// other callers get the structured ALIGNMENT DIFFS lines only.
    /// Rendered coordinates are 1-based inclusive.
    /// The project is cloned out of the lock and rendered on a blocking
    /// thread — rendering the enzyme list needs the full project, and
    /// holding the pm read lock across it would starve UI edits (writers).
    async fn digest_region(
        &self,
        project_id: &str,
        region: Option<(i64, i64)>,
        compact: bool,
        alignment_columns: bool,
    ) -> Option<String> {
        let project = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(project_id).cloned()?
        };
        let opts = DigestOptions {
            compact_enzymes: compact,
            include_alignment_view: alignment_columns,
            ..DigestOptions::default()
        };
        tokio::task::spawn_blocking(move || project_digest(&project, &opts, region).ok())
            .await
            .ok()
            .flatten()
    }

    /// Text digest of the region around a feature (looked up by id); compact
    /// enzyme rendering (only mutation tools call this).
    async fn digest_feature_region(&self, project_id: &str, feature_id: &str) -> Option<String> {
        let (project, s, e) = {
            let pm = self.pm.read().await;
            let project = pm.get_project_by_id(project_id)?;
            let f = project.features.iter().find(|f| f.id == feature_id)?;
            // Clamp the +/-5 context window with saturating arithmetic so a
            // feature near an end (or a maliciously huge coordinate that slipped
            // past validation) can't underflow/overflow and panic the process.
            // A wrapping feature (start > end) yields s > e, which
            // project_digest interprets as an origin-wrapping region window
            // on circular projects — the intended span.
            let last = project.length.saturating_sub(1);
            let s = f.start.saturating_sub(5).min(last);
            let e = (f.end.saturating_add(5)).min(last);
            (project.clone(), s, e)
        };
        let opts = DigestOptions {
            compact_enzymes: true,
            ..DigestOptions::default()
        };
        tokio::task::spawn_blocking(move || project_digest(&project, &opts, Some((s, e))).ok())
            .await
            .ok()
            .flatten()
    }

    async fn feature_exists(&self, project_id: &str, feature_id: &str) -> bool {
        let pm = self.pm.read().await;
        pm.get_project_by_id(project_id)
            .map(|p| p.features.iter().any(|f| f.id == feature_id))
            .unwrap_or(false)
    }

    /// Reject feature spans outside [1, project.length] (1-based).
    /// resolve_feature_span only checks start<=end (no upper bound), so
    /// without this a caller could write a feature with end = i64::MAX and
    /// later panic downstream code that slices the sequence by these
    /// coordinates. `start`/`end` are internal 0-based inclusive; for a
    /// wrapping feature start > end, so every segment end is checked too.
    async fn span_within_bounds(
        &self,
        project_id: &str,
        segments: &[Segment],
        start: i64,
        end: i64,
    ) -> Result<(), String> {
        let pm = self.pm.read().await;
        let plen = pm.get_project_by_id(project_id).map(|p| p.length).unwrap_or(0);
        let max_coord = segments
            .iter()
            .map(|s| s.end.max(s.start))
            .max()
            .unwrap_or(0)
            .max(start)
            .max(end);
        if max_coord >= plen {
            return Err(format!(
                "feature span {}..{} is out of range for project length {} (1-based inclusive)",
                start + 1,
                end + 1,
                plen
            ));
        }
        Ok(())
    }

    /// A `{"error": ...}` payload from a shared core means a tool-level failure.
    fn payload_error(payload: &serde_json::Value) -> Option<String> {
        payload.get("error").and_then(|v| v.as_str()).map(String::from)
    }
}

#[tool_router]
impl<R: Runtime> LibreGeneMcp<R> {
    /// List the open projects — the sequence files currently loaded in memory.
    ///
    /// Returns {ok, message, projects: [{id, name, length, topology, moleculeType,
    /// unit, dirty, sequenceHash, revCompHash}], activeId}. The id of a file-backed
    /// project is its path; every other tool addresses a project by that id.
    /// `activeId` is the project the USER is viewing — informational only; avoid
    /// mutating it while other agents work in parallel.
    #[tool]
    async fn list_projects(&self) -> Result<Json<serde_json::Value>, ErrorData> {
        self.list_projects_impl().await
    }

    /// Compact text digest of a whole project (`text`): features, primers, enzyme
    /// cutters, methylation, auto-annotated common features — RNA/protein projects
    /// omit the DNA-only sections. Enzyme cutters collapse to one count line unless
    /// `compactCutters: false`. CDS/mRNA features whose stored /translation
    /// disagrees with the DNA get a WARNING line, and positions where >=2 stored
    /// reads carry the same mismatch are listed as SHARED MISMATCHES (a fact, not a
    /// verdict — shared differences can be biological, clonal or template-derived).
    /// `featureFilter` keeps features by name (case-insensitive substring) or
    /// exact ftype; `maxFeatures` caps the list.
    /// Returns {ok, message, projectId, unit, text, sequenceHash, revCompHash}.
    #[tool]
    async fn get_project_overview(
        &self,
        Parameters(request): Parameters<OverviewRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.get_project_overview_impl(request).await
    }

    /// Compact text digest of one region of a project. Coordinates are 1-based
    /// inclusive; on circular sequences `start > end` wraps the origin. Covers the
    /// window's features, primer sites and enzyme cuts (one count line unless
    /// `compact: false`), plus ALIGNMENT DIFFS (per-read mismatches, deletions,
    /// insertions with 1-based coordinates) when stored reads overlap. Pass
    /// `showAlignmentColumns: true` to add the ALIGNMENT VIEW column block (three
    /// rows per read: template, match mask, read bases); it covers at most 500 bp of
    /// window — wider windows get an omission note instead of rows.
    /// Returns {ok, message, projectId, unit, region, text, sequenceHash,
    /// revCompHash}. This is the fast way to check whether a site is mutated
    /// without digesting a full read.
    #[tool]
    async fn get_region_view(
        &self,
        Parameters(request): Parameters<RegionRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.get_region_view_impl(request).await
    }

    /// Read bases or resolve a coordinate — INSPECTION only: to move a sequence
    /// between tools, write it to a file with save_file's `region` instead.
    ///
    /// WINDOW MODE — `start` + `end` (1-based inclusive; start > end wraps the
    /// origin on circular sequences; at most 10000 bp). Returns {ok, message,
    /// projectId, unit, start, end, length, sequence, text, startContext,
    /// endContext}: `sequence` is the uppercase bases, `text` the same window with
    /// a coordinate ruler, and each context describes the window's first/last base
    /// (containing features with their feature-relative offset, plus codon and
    /// amino-acid hits inside CDS/mRNA features).
    ///
    /// COORDINATE MODE — exactly one of `position` (absolute template position),
    /// `featureId` + `featureOffset` (1-based along the feature's own 5'->3'
    /// direction), or `featureId` + `aaPosition` (1-based inside a CDS/mRNA,
    /// counting the initiator Met as 1 — literature numbering that skips the Met is
    /// this value minus 1, and the response echoes it as `codonIndex`). Returns
    /// {ok, message, projectId, unit, mode, input,
    /// position, base, codonPositions?, features, translations, start, end,
    /// sequence, text}; `sequence`/`text` cover `flank` bases on each side
    /// (default 30, clamped).
    #[tool]
    async fn read_sequence(
        &self,
        Parameters(request): Parameters<SequenceRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.read_sequence_impl(request).await
    }

    /// Look up enzyme names in the built-in restriction-enzyme database — the
    /// discovery tool to use before find_restriction_sites. `query` matches
    /// case-insensitively against enzyme NAMES or recognition SITES (e.g.
    /// "eco", "Bam", "GAATTC"); omit it for the whole catalog (paged by
    /// `limit`, default 50, max 200 — `total` reports the full match count).
    /// Returns {ok, message, query, total, count, enzymes: [{name, site}]}.
    #[tool]
    async fn list_enzymes(
        &self,
        Parameters(request): Parameters<EnzymeListRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.list_enzymes_impl(request).await
    }

    /// List restriction-enzyme sites on a DNA project. `enzymes` = names to report
    /// (case-insensitive); omit it for every enzyme with a site. Requested names may
    /// be cutting (normal entry), known but site-less (empty `sites` + a `note`), or
    /// unknown — unknown names never fail the call: they appear under
    /// `unknownEnzymes` with `similar` suggestions while the known names still
    /// answer. Use list_enzymes to discover valid names. For a full panorama of cuts
    /// in a window use get_region_view with `compact: false`.
    ///
    /// Returns {ok, message, projectId, unit, enzymeCount, enzymes: [{name,
    /// siteCount, sites: [site], note?}], unknownEnzymes?, hashes}, with site =
    /// {recStart, recEnd, recSeq, strand, cuts: [{topCutIndex, botCutIndex}],
    /// unique, methylationBlocked, hasCutsOutsideRecognitionSite, note?}.
    /// recStart/recEnd are 1-based inclusive (recStart > recEnd when the
    /// recognition sequence spans the circular origin). A cut at topCutIndex N
    /// severs the DNA between the 1-based bases N and N+1, so the upstream fragment
    /// ends at N and the downstream fragment starts at N+1 — use this to model
    /// ligation junctions. For type IIS enzymes the cut lies outside the
    /// recognition sequence (`hasCutsOutsideRecognitionSite`), so the site survives
    /// on the upstream fragment.
    #[tool]
    async fn find_restriction_sites(
        &self,
        Parameters(request): Parameters<FindRestrictionSitesRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.find_restriction_sites_impl(request).await
    }

    /// List the primers stored in a project (read-only; no recompute). Returns
    /// {ok, message, projectId, unit, primerCount, primers: [{id, name, type, seq,
    /// length, bindingSiteCount, sites: [site]}]} — sites best-first (Tm
    /// descending), empty when the primer does not bind. The site shape is shared
    /// with add_primer and check_primer_binding: {strand, templateStart,
    /// templateEnd, tm, annealLength, tailLength, has3PrimeMismatch,
    /// alignedTemplate, matchMask}, templateStart/templateEnd 1-based inclusive.
    #[tool]
    async fn list_primers(
        &self,
        Parameters(request): Parameters<ListPrimersRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.list_primers_impl(request).await
    }

    // -----------------------------------------------------------------------
    // Mutations
    // -----------------------------------------------------------------------

    /// Open a sequence file, load it as a project AND bind it as your agent tab in
    /// one step (the project id IS the file path). This is the entry point for every
    /// sequence that already exists as a file — do not paste file contents into
    /// other tools. The bound project stays in the sidebar marked as an agent tab
    /// and LOCKED against user input; every later tool call on it re-locks it.
    ///
    /// A fresh open returns {ok, message, projectId, unit, locked, text,
    /// sequenceHash, revCompHash} with `text` = the compact overview digest. A path
    /// already bound to you is re-locked and returns {..., locked: true, reused:
    /// true} without `text`. A path the USER opened is refused: copy the file with
    /// bash `cp` and open the copy. Mutating tools refuse every project that is not
    /// bound as an agent tab.
    #[tool]
    async fn open_project(
        &self,
        Parameters(request): Parameters<OpenProjectRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.open_project_impl(request).await
    }

    /// Write a project to disk — the reverse of open_project.
    ///
    /// Whole project (no `region`): sequence + features go through the normal
    /// serializer (.gbk/.gb/.genbank for DNA/RNA, .gpt for protein) and the project
    /// is marked clean. Returns {ok, message, projectId, unit, path, length,
    /// bytesWritten, text, hashes}.
    ///
    /// `region`: export one subsequence — the recommended way to create a new file
    /// from a known region of an open project (never retype a sequence into another
    /// tool). Exactly one selector:
    /// - `start` + `end`: 1-based inclusive; wraps the origin on circular sequences.
    /// - `featureId`: the feature's sequence, segments joined 5'->3' (reverse-
    /// complemented for a minus-strand DNA feature), overlapping annotations
    /// carried along.
    /// - `cut1` + `cut2`: the fragment between two cuts; a cut at N severs the DNA
    /// between the 1-based bases N and N+1. Take the positions from
    /// find_restriction_sites (`topCutIndex`) or check_primer_binding's `amplicon`
    /// instead of deriving them by hand.
    /// Exports are always linear, include every overlapping feature (clipped) and
    /// primer, do NOT mark the project clean, and return {ok, message, projectId,
    /// unit, path, length, bytesWritten, primerCount, primers?, text, hashes}; the
    /// exported sequence itself is not echoed.
    ///
    /// `overwrite: true` is required when `path` already exists, unless it is the
    /// project's own source path for a whole-project save (a region export over the
    /// source path needs it too).
    #[tool]
    async fn save_file(
        &self,
        Parameters(request): Parameters<SaveFileRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.save_file_impl(request).await
    }

    /// Replace sequence [start..end] (1-based inclusive) with a replacement; an
    /// empty replacement deletes. A pure insertion before base N is `start: N,
    /// end: N-1`; ranges must not wrap.
    ///
    /// Give the replacement as `replacementPath` (PREFERRED — a file cannot be
    /// mistyped, and its features/primers travel with the sequence: clipped to the
    /// inserted span, rebased, strand-flipped when `strand` is "-") or as short
    /// plain text in `replacement`. `strand: "-"` reverse-complements the
    /// replacement (DNA projects only). `expectedOld` guards the edit: on mismatch
    /// the failure carries the authoritative `currentContent` (copy it as
    /// `expectedOld` and retry; there are no other diagnostics).
    ///
    /// Enzymes, primer sites, translations AND stored read alignments are
    /// recomputed, so alignment data read earlier may be superseded.
    /// Returns {ok, message, projectId, unit, oldLength, newLength, contextBefore?,
    /// contextAfter?, removedFeatures, clippedFeatures, transferredFeatures?,
    /// transferredPrimers?, notes?, textBefore?,
    /// text, hashes}: contexts are {start, end, sequence} 1-based spans; equal-length
    /// replacements keep every feature at its coordinates (safe for point mutations
    /// and case normalization); U<->T normalization is reported in `notes`.
    #[tool]
    async fn edit_sequence(
        &self,
        Parameters(request): Parameters<EditSequenceRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.edit_sequence_impl(request).await
    }

    /// Create or update one feature (coordinates 1-based inclusive).
    ///
    /// OMIT `featureId` to create: `name`, `ftype` and a span — `start` + `end`, or
    /// `segments` ([{start, end}] in 5'->3' order, ascending starts; a cross-origin
    /// feature leads with its tail) — are required. `strand` (".", "+", "-";
    /// default "+"), `color` (hex) and `notes` are optional.
    ///
    /// GIVE `featureId` to update: pass at least one of name/ftype/color/strand/
    /// start+end/segments (notes cannot be updated). `segments` is mutually
    /// exclusive with `start`/`end` in both modes; neither form touches `strand`.
    ///
    /// Returns {ok, message, projectId, unit, featureId, text, hashes}.
    #[tool]
    async fn set_feature(
        &self,
        Parameters(request): Parameters<SetFeatureRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.set_feature_impl(request).await
    }

    /// Add a primer to a DNA project and recompute its binding sites. Sequences are
    /// short (~20-60 nt), so `seq` is plain text. `name` must be unique across the
    /// project's primers AND features (rename with a "-F"/"-R" suffix if taken).
    ///
    /// Returns {ok, message, projectId, unit, primerId, name, type, seq, length,
    /// bindingSiteCount, sites: [site], text, hashes}; the site shape is the shared
    /// one (see list_primers) — sites best-first, empty when the primer does not
    /// bind.
    #[tool]
    async fn add_primer(
        &self,
        Parameters(request): Parameters<AddPrimerRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.add_primer_impl(request).await
    }

    /// Align a read against the project template and APPEND it as a new alignment
    /// (ids aln-1, aln-2, ...; existing alignments are never touched).
    ///
    /// Give exactly one of `path` (PREFERRED: .gbk/.gb/.genbank, .dna/.rna/.prot,
    /// .gpt, .fa/.fasta, .ab1 — for .ab1 the basecalled sequence is used) or `bases`
    /// (short hand-authored reads only). `algorithm`: "blast" (default; chains any
    /// number of colinear segments, so split/multi-hit reads align in full) or
    /// "smith-waterman" (single local block plus at most one flank).
    ///
    /// `region` or `featureId` (+`flank`) focus the response on a window: the detail
    /// lists are filtered to it, `window` reports the in-window counts, and
    /// `orientedSequence` is omitted. `compact: true` drops `orientedSequence` and
    /// `text` entirely.
    ///
    /// Returns {ok, message, projectId, unit, significant, alignmentId, name,
    /// identity, strand, segmentCount, alignedLength, readLength, mismatches,
    /// insertions, deletions, mismatchDetails, deletionDetails, insertionDetails,
    /// coverage, orientedSequence?, window?,
    /// notes?, alignments, text, hashes}.
    /// `identity` is a 0-1 fraction; `alignedLength` is the covered template span
    /// while `readLength` is the read's own length; the mismatch/insertion/deletion
    /// counts describe the WHOLE read (an insertion sits between bases `position`
    /// and `position+1`); `orientedSequence` is the full read oriented to the
    /// template (rev-comp'd when strand is "-"); every `alignments` entry has the
    /// same shape, older reads stats-only. No significant hit returns {ok: false,
    /// significant: false} with the reason (identity < 0.60 or span < 50 bp).
    #[tool]
    async fn add_alignment(
        &self,
        Parameters(request): Parameters<AddAlignmentRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.add_alignment_impl(request).await
    }

    // -----------------------------------------------------------------------
    // Analysis
    // -----------------------------------------------------------------------

    /// Find open reading frames (ATG->stop, both strands, all frames) on a DNA
    /// project. `minAa` defaults to 75; `addAsFeatures: true` appends the ORFs as
    /// real CDS features (mutation envelope with `featureCount` and `text`).
    /// Otherwise returns {ok, message, projectId, unit, minAa, orfCount, orfs:
    /// [feature], hashes} with 1-based inclusive coordinates.
    #[tool]
    async fn find_orfs(
        &self,
        Parameters(request): Parameters<FindOrfsRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.find_orfs_impl(request).await
    }

    /// Design primer candidates against a DNA project. `mode`:
    /// - "amplify": primes across `seg`; the product's top strand IS seg's template
    /// top strand, so Fwd/Rev follow the TEMPLATE, not any feature's coding
    /// strand (`cdsOverlaps` maps them onto overlapping CDS features — for a
    /// minus-strand CDS, Fwd sits at its 3' end). `fwdEnzyme`/`revEnzyme` append a
    /// 5' tail of `protectBases` (default 3) GC bases + the recognition site;
    /// `internalSites` warns when that site also occurs inside the product.
    /// - "oepcr": two fragments (`seg`, `seg2`) joined through `overlapLen`
    /// (default 20).
    /// - "mutagenesis": `mutSeq` is the desired PLUS-strand content of `seg` (same
    /// length, at most 3 differing bases) — for a minus-strand CDS, reverse-
    /// complement the intended coding-strand edit yourself, then confirm
    /// `mutation.cds.aaAfter` is the residue you intended. Primer length is
    /// roughly seg length + 2 × `armLen` (default 20), so keep `seg` tight around
    /// the edited codon(s). Amino-acid positions count the initiator Met as 1
    /// (literature numbering that skips it = minus 1).
    ///
    /// Returns {ok, message, projectId, mode, groups: [{name, type,
    /// recommendedIndex, candidates: [{id (unique within its group), recommended,
    /// seq, tail, tailLength, annealLength, tm, gcPercent, designedAnnealLength?,
    /// designedTm?}]}],
    /// mutation?, internalSites?, internalSiteCount?, orientation?, cdsOverlaps?,
    /// warnings?, notes?, hashes} (parameters that only apply to another mode are
    /// reported in `notes` instead of being silently dropped). Use the candidate
    /// with `recommended: true` (or `recommendedIndex`) — `annealLength`/`tm`
    /// describe the ACTUAL contiguous 3' match, `designedAnnealLength`/`designedTm`
    /// the designed core before 3'-end unification. `orientation` is the product's
    /// top strand, `cdsOverlaps[].strand` maps overlapping CDS features onto it.
    #[tool]
    async fn design_primers(
        &self,
        Parameters(request): Parameters<DesignPrimersRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.design_primers_impl(request).await
    }

    /// Test primers against a DNA project's sequence without persisting them
    /// (sequences are short, so plain text). `binds: true` means the 3' anneal core
    /// matched; a 5' tail may still mismatch. Tm/annealLength describe the ACTUAL
    /// contiguous 3' match, so a tailed primer can report a higher value than
    /// design_primers did.
    ///
    /// Returns {ok, message, projectId, results: [{id, name, type,
    /// primerLength, binds, bindingSiteCount, site, sites}], amplicon?, hashes}.
    /// Sites are best-first with the shared site shape (see list_primers) — read
    /// `alignedTemplate`/`matchMask` to see exactly which primer bases pair. When
    /// the request holds exactly one fwd and one rev primer that both bind, the
    /// result carries `amplicon` {forwardStart, reverseEnd, length, note} with the
    /// PCR product's size (no export needed).
    #[tool]
    async fn check_primer_binding(
        &self,
        Parameters(request): Parameters<CheckPrimerBindingRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.check_primer_binding_impl(request).await
    }

    /// Convert sequences between molecule types, with codon optimization.
    /// BATCH: `items` holds 1-64 independent conversions (a failing item is
    /// reported in its slot and does not abort the others); a single conversion may
    /// pass the item fields at the top level instead.
    ///
    /// Matrix (`from`/`to` = "dna" | "rna" | "protein"):
    /// - dna<->rna: T<->U conversion (optional `revComp`).
    /// - dna/rna -> protein: translation, frame 0 (a trailing partial codon is
    /// dropped).
    /// - protein -> dna/rna: reverse translation with codon optimization
    ///   (`species` required — one of the built-in keys {species}; `method` =
    ///   use_best_codon | match_codon_usage | harmonize_rca).
    /// A protein without a trailing '*' yields DNA without a stop codon — see the
    /// item's `notes`.
    /// - dna -> dna: codon optimization when `species`/optimizer parameters are
    /// given, else passthrough; `revComp` reverse-complements.
    /// - protein -> protein and `revComp` with a protein side: rejected.
    /// Defaults: `from` = dna (project mode), the file's type (`inputPath`), or dna
    /// (`sequence`); `to` = dna for a protein input, else `from`.
    ///
    /// Exactly one input per item:
    /// - `projectId` + `featureId` (DNA projects, dna->dna optimization ONLY):
    /// optimize a CDS/mRNA feature; `apply: true` writes it back through the same
    /// recompute path as edit_sequence, `false` (default) previews.
    /// - `inputPath` (PREFERRED for real sequences): .gbk/.gb/.genbank/.dna/.rna/
    /// .fasta/.fa/.ab1 nucleotide, .gpt/.prot protein (with `featureId` +
    /// `species`, that file's CDS is optimized and the rest of the file kept).
    /// - `sequence`: short hand-authored text only.
    ///
    /// `outputPath` (sequence/inputPath modes) writes the result — .gbk/.gb/.genbank
    /// GenBank, .gpt protein GenBank, .fa/.fasta/.txt bare text; `overwrite: true`
    /// when it exists. Prefer the written file over the echoed `sequence`.
    ///
    /// Returns {ok, message, resultCount, okCount, failedCount, results: [{index,
    /// ok, from, to, message, sequence?, length?, path?, projectId?, text?, notes?,
    /// aa?, codonCount?, newCodons?, caiBefore?, caiAfter?, gcPercentBefore?,
    /// gcPercentAfter?, repairs?, repairCount?, unresolved?, method?, species?,
    /// sequenceHash?, revCompHash?}]}. Failed items carry only {index, ok: false,
    /// message}; `repairs[].codonIndex` is 1-based and `unresolved` entries are
    /// "<reason> <start>..<end>" with 1-based inclusive base offsets.
    #[tool]
    async fn convert_sequence(
        &self,
        Parameters(request): Parameters<ConvertSequenceRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.convert_sequence_impl(request).await
    }

}

/// Splice the core's built-in codon-usage species keys into every tool
/// description carrying the `{species}` placeholder, so the authoritative list
/// (and only it) reaches the client — no dedicated list_species tool, and no
/// hand-maintained copy that can go stale.
pub(crate) fn splice_species_keys(tools: &mut [rmcp::model::Tool]) {
    let species = libregene_core::codon::list_species().join(", ");
    for tool in tools.iter_mut() {
        if let Some(desc) = &mut tool.description {
            if desc.contains("{species}") {
                *desc = std::borrow::Cow::Owned(desc.replace("{species}", &species));
            }
        }
    }
}

#[tool_handler(name = "LibreGene", instructions = "LibreGene is a plasmid editor; you drive the open project like a user. AGENT TABS: open_project loads a sequence file AND binds it as your agent tab in one step (locked against user input; every call on it re-locks it). Mutating tools refuse any project you did not open — if a path is already open but not bound, it belongs to the user: copy the file with bash `cp` to a new path and open the copy. A path already bound to you is reused (locked, reused: true). RESPONSES: every tool returns {ok, message, projectId?, unit?, text?, sequenceHash?, revCompHash?, ...}; ok:false is a domain rejection with the same keys plus diagnostics, while unknown-project / not-an-agent-tab / wrong-molecule-type / internal failures come back as MCP errors. Coordinates are 1-based inclusive everywhere; compare sequenceHash/revCompHash across calls to detect sequence changes. FILE-FIRST I/O: whenever a sequence exists as a file (or can be written to one), pass the path — open_project, edit_sequence's replacementPath, add_alignment's path, convert_sequence's inputPath/outputPath, save_file's `region` export — instead of pasting sequence text; plain-text sequence parameters are only for short hand-authored input (primers ~20-60 nt, point mutations, short inserts). read_sequence is for inspecting bases, not for moving sequences between tools. list_projects' activeId is the project the user is viewing (informational) — avoid it when several agents work in parallel, and prefer one working copy per agent.")]
impl<R: Runtime> ServerHandler for LibreGeneMcp<R> {
    // Tools return Json<serde_json::Value>, so the generated outputSchema has
    // no top-level "type". The MCP spec requires outputSchema.type == "object";
    // strict clients (e.g. kimi-code) reject the list otherwise.
    async fn list_tools(
        &self,
        _request: Option<rmcp::model::PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::RoleServer>,
    ) -> Result<rmcp::model::ListToolsResult, rmcp::ErrorData> {
        let mut tools = Self::tool_router().list_all();
        splice_species_keys(&mut tools);
        for tool in &mut tools {
            if let Some(schema) = &mut tool.output_schema {
                let patched = Arc::make_mut(schema);
                patched
                    .entry("type")
                    .or_insert_with(|| serde_json::Value::String("object".into()));
            }
        }
        Ok(rmcp::model::ListToolsResult {
            result_type: Some(rmcp::model::ResultType::COMPLETE),
            tools,
            meta: None,
            next_cursor: None,
            ttl_ms: None,
            cache_scope: None,
        })
    }
}

#[cfg(test)]
mod tests;
