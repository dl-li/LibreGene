//! Embedded MCP (Model Context Protocol) server for LibreGene.
//!
//! Exposes tools over Streamable HTTP on `127.0.0.1:8766` so an external LLM
//! agent can operate the app like a real user. Mutations go through the same
//! shared cores as the Tauri commands (`crate::do_*`), so recompute, dirty
//! marking and `broadcast_project()` behave identically and the UI updates
//! live. Every mutation tool returns a uniform `{ok, message, projectId,
//! regionView?}` envelope (plus tool-specific fields).
//!
//! Sequence-change detection: every project-targeting tool response carries
//! `sequenceHash`/`revCompHash` — 7-hex hashes of the current biological
//! sequence and its reverse complement (case-insensitive, annotations/
//! primers/whitespace ignored; `revCompHash` is null for proteins). Comparing
//! them across calls detects any sequence edit; a project and its
//! reverse-complemented file share one (sequenceHash, revCompHash) pair.
//! Text digests carry the same values as a `SEQHASH:` header line.
//!
//! Coordinate conventions (stated again in every tool description). This MCP
//! layer is agent-facing, so all coordinates in tool inputs and outputs are
//! **1-based inclusive** (the GenBank convention); the internal model and the
//! shared `crate::do_*` cores stay **0-based inclusive**, and this module
//! converts at the boundary (`to1`/`from1`):
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
                    "projectId": id,
                    "locked": true,
                    "reused": true,
                    "message": format!("Project '{}' is already open and bound as your agent tab (re-locked)", id),
                });
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
        let unit = match p.molecule_type.as_str() {
            "rna" => "nt",
            "protein" => "aa",
            _ => "bp",
        };
        let desc = format!("{} {} {}", p.length, unit, p.topology);
        Some(if p.name.is_empty() {
            desc
        } else {
            format!("{}: {}", p.name, desc)
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
    /// regionView small). Rendered coordinates are 1-based inclusive.
    /// The project is cloned out of the lock and rendered on a blocking
    /// thread — rendering the enzyme list needs the full project, and
    /// holding the pm read lock across it would starve UI edits (writers).
    async fn digest_region(
        &self,
        project_id: &str,
        region: Option<(i64, i64)>,
        compact: bool,
    ) -> Option<String> {
        let project = {
            let pm = self.pm.read().await;
            pm.get_project_by_id(project_id).cloned()?
        };
        let opts = DigestOptions {
            compact_enzymes: compact,
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
    /// List all open projects — the files currently loaded into memory.
    /// A "project" is an open file: `open_project` loads a file as a project
    /// (bound as your agent tab) and returns its `projectId`; every other
    /// tool addresses a project by its required `project_id`. Returns
    /// {"projects": [{id, name, length, topology, dirty}], "activeId":
    /// id-or-null}. `activeId` is the project the USER is currently viewing —
    /// informational only; it does not influence tool routing, and when
    /// several agents work in parallel you should avoid touching it.
    /// Each project entry also carries `sequenceHash`/`revCompHash` (see
    /// get_project_overview).
    #[tool]
    async fn list_projects(&self) -> Result<Json<serde_json::Value>, ErrorData> {
        self.list_projects_impl().await
    }

    /// Compact text digest of a whole project. Coordinates are 1-based
    /// inclusive (features, primers, read ranges); enzyme cuts render as
    /// N^N+1 (between the 1-based bases N and N+1). feature_filter matches
    /// feature name (case-insensitive substring) or exact ftype. Primers
    /// render as a PRIMERS
    /// section (or "PRIMERS (none)" when the project has none). The UNIQUE
    /// CUTTERS list (90+ lines on real plasmids) is collapsed to a single count
    /// line by default; pass `compactCutters: false` for the full per-enzyme
    /// list. The digest ends with a `DETECTED COMMON FEATURES (auto)` section
    /// listing non-fragment features auto-annotated against the embedded
    /// SnapGene database, one line each (name | type | strand | start..end |
    /// identity%) with an `(already annotated)` marker. Fragment hits are
    /// omitted to avoid misleading partial matches. CDS/mRNA features whose
    /// stored `/translation` qualifier disagrees with the current DNA
    /// sequence get a WARNING line (first disagreeing amino-acid position).
    /// When ≥2 stored reads share the same mismatch at the same template
    /// position, a MISMATCH CONSENSUS line flags the positions as possibly
    /// outdated template. Length units and sections
    /// follow the molecule type: DNA projects get bp + PRIMERS/ENZYMES/
    /// methylation/auto-annotation; RNA/protein projects use nt/aa and omit all
    /// DNA-only sections (features still render). Returns {projectId, text}.
    /// The response carries `sequenceHash`/`revCompHash` (7-hex hashes of the
    /// current biological sequence and its reverse complement,
    /// case-insensitive, ignoring features/primers/alignments; revCompHash is
    /// null for proteins) — compare across calls to detect any sequence
    /// change; a project and its reverse-complemented file share one
    /// (sequenceHash, revCompHash) pair. The `text` digest carries the same
    /// values as a `SEQHASH:` header line.
    #[tool]
    async fn get_project_overview(
        &self,
        Parameters(request): Parameters<OverviewRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.get_project_overview_impl(request).await
    }

    /// Compact text digest of a region of a project. start/end are 1-based
    /// inclusive; on circular sequences start > end wraps the origin. Only
    /// features, primer binding sites and enzyme cut positions overlapping
    /// [start, end] are included. The enzyme cut list is collapsed into a
    /// single count line by default; pass `compact: false` for every cut in
    /// the window. When stored read alignments overlap the window, an
    /// ALIGNMENT DIFFS IN REGION section lists each read's mismatches,
    /// deletions and insertions inside the window (1-based coordinates and
    /// bases; reads with no differences in the window are marked
    /// "no differences in window") — use it to check whether a site is
    /// mutated without aligning reads by eye. An ALIGNMENT VIEW IN REGION
    /// section then shows each overlapping read as aligned columns: three rows
    /// per read — template bases, a match mask (`|` match, `.` mismatch, `-`
    /// read gap, the same convention as check_primer_binding's matchMask) and
    /// the read bases (a `-` marks a deleted template column) — with the
    /// 1-based start coordinate on each row, so you can read a window's read
    /// bases directly instead of unwinding circular wraps and gap offsets from
    /// orientedSequence. Rows wrap at 60 bp; insertions and template positions
    /// the read does not cover are listed as `+N bp` / `uncovered template`
    /// notes below the block. Reads whose covered window exceeds 500 bp get an
    /// omission note instead of the rows (use ALIGNMENT DIFFS or a narrower
    /// window). Returns {projectId, text}.
    /// The response also carries `sequenceHash`/`revCompHash` (see
    /// get_project_overview; the digest `text` carries the same values as a
    /// `SEQHASH:` header line).
    #[tool]
    async fn get_region_view(
        &self,
        Parameters(request): Parameters<RegionRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.get_region_view_impl(request).await
    }

    /// Read bases of a project's sequence, or resolve a coordinate. Exactly
    /// one input form:
    ///
    /// WINDOW MODE — `start` + `end` (both required, 1-based inclusive; on
    /// circular sequences start > end wraps the origin; windows larger than
    /// 10000 bp are rejected). Returns {projectId, sequence, text,
    /// startContext, endContext} — `sequence` is the plain uppercase base
    /// string (machine-readable); `text` is the same window with a coordinate
    /// ruler (10 bp groups, 60 bp per line; the ruler line is omitted for
    /// windows of 60 bp or less, where the per-line position prefix is
    /// enough). `startContext`/`endContext` annotate the window's first/last
    /// base: each is {position, features, translations} listing every feature
    /// containing that position (with its 1-based feature-relative offset)
    /// and, inside a CDS/mRNA, the codon index, amino-acid position (two
    /// conventions) and amino acid.
    ///
    /// COORDINATE MODE — exactly one of:
    /// 1. `position`: a full-file 1-based inclusive template coordinate.
    /// 2. `feature_id` + `feature_offset`: 1-based offset along the feature's
    ///    own 5'→3' direction (reverse-complemented features count from their
    ///    3' end on the template).
    /// 3. `feature_id` + `aa_position`: 1-based amino-acid position within a
    ///    CDS/mRNA feature, INCLUDING the initiator Met (Met = 1). Literature
    ///    numbering that skips the Met (e.g. mEGFP A206K) corresponds to the
    ///    response's `aaPositionExcludingMet`, so convert before calling:
    ///    literature position + 1 (when the Met is present) is the
    ///    `aa_position` to send.
    /// Returns {projectId, input, position, base, codonPositions?, features,
    /// translations, sequence, text}: `position` echoes the resolved absolute
    /// coordinate (1-based) and `base` the template base there (plus-strand,
    /// uppercase; the residue letter on protein projects); `features`/
    /// `translations` are the full hit details for that position (same shape
    /// as the window mode's contexts); for amino-acid input `codonPositions`
    /// holds the three template positions of the requested codon in 5'→3'
    /// biological order (and `position` is the first of them); `sequence`/
    /// `text` give the `flank`-bp window around the position (default 30,
    /// clamped at the sequence ends). Translation hits only appear when the
    /// position falls inside a CDS/mRNA feature.
    ///
    /// This tool is for INSPECTING bases only: if you need to hand this
    /// sequence (or part of it) to another tool or file, write it to a file
    /// with save_file's `region` instead of copying the text.
    /// The response also carries `sequenceHash`/`revCompHash` (see
    /// get_project_overview).
    #[tool]
    async fn read_sequence(
        &self,
        Parameters(request): Parameters<SequenceRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.read_sequence_impl(request).await
    }

    /// IUPAC-aware search of a project's sequence on both strands (reverse
    /// strand skipped for palindromic queries). Hits are 1-based inclusive.
    /// Returns {projectId, matches: [{start, end, strand}]}.
    /// Self-complementary TARGETS (e.g. an shRNA stem: arm X followed later by
    /// its reverse complement X') legitimately produce one '+' hit at X and
    /// one '-' hit at X' — the two arms of the stem, not a duplicated
    /// sequence. Only exactly palindromic QUERIES (reverse complement == the
    /// query itself, e.g. "AT" or a restriction site) skip the reverse scan.
    /// DNA-only: rejects RNA/protein projects (single-strand, no reverse
    /// strand to search).
    /// The response also carries `sequenceHash`/`revCompHash` (see
    /// get_project_overview).
    #[tool]
    async fn search_sequence(
        &self,
        Parameters(request): Parameters<SearchRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.search_sequence_impl(request).await
    }

    /// List restriction-enzyme recognition sites on a project's sequence.
    /// `enzymes` is an optional list of enzyme names (case-insensitive); omit
    /// it (or pass []) to report every enzyme that has a site. Requested names
    /// fall into three classes: cutting this sequence (normal entry), in the
    /// enzyme database but WITHOUT a site on this sequence (entry with empty
    /// `sites` and a `note` saying so — this is the answer for e.g. a site
    /// destroyed by cloning), and unknown to the database. A batch query
    /// degrades gracefully: known names return normally and unknown names are
    /// listed under `unknownEnzymes` ([{name, error, similar}]) without
    /// failing the whole call; only when EVERY requested name is unknown does
    /// the call fail with near-match suggestions — use that error to probe
    /// which enzyme names exist on this sequence (this is the replacement for
    /// the removed full-database dump: query per name instead of pulling the
    /// whole ~196 KB catalog). When you need a full panorama of EVERY enzyme
    /// cut inside a region rather than per-enzyme probing, call
    /// get_region_view with `compact: false` on that window instead — it lists
    /// all cuts without naming enzymes one by one. Sites are the
    /// already-computed engine results
    /// the UI shows (circular-normalized, methylation-aware), so no recompute
    /// runs.
    /// Returns {projectId, enzymes: [{name, sites: [{recStart, recEnd,
    /// recSeq, strand, cuts: [{topCutIndex, botCutIndex}], methylationBlocked,
    /// unique}]}]}. recStart/recEnd are 1-based inclusive; topCutIndex/
    /// botCutIndex give the 1-based base BEFORE the break: the strand is
    /// severed between topCutIndex and topCutIndex+1 (topCutIndex = len on a
    /// circular sequence means between the last and the first base); strand is
    /// "top" or "bottom" (recognition orientation); unique = exactly one site
    /// for that enzyme. On circular molecules every coordinate stays within
    /// 1..=len; a recognition sequence spanning the origin reads
    /// recStart > recEnd. Sites whose cuts fall OUTSIDE the recognition
    /// sequence (type IIS enzymes like BbsI) carry
    /// `cutsOutsideRecognitionSite: true` plus a `note`; for those,
    /// topCutIndex/botCutIndex — not recStart/recEnd — give the actual break
    /// points.
    ///
    /// Half-site accounting for assembly: a cut at topCutIndex N severs the
    /// DNA between the 1-based bases N and N+1, so the UPSTREAM fragment ends
    /// with base N and the DOWNSTREAM fragment starts with base N+1 — each
    /// fragment keeps the half-site that lies on its side of the break. When
    /// you compute a ligation junction between two digests, the product is
    /// [fragment A .. its topCutIndex] + [fragment B .. its topCutIndex+1 ..].
    /// Example — NheI recognizes GCTAGC and cuts G^CTAGC on the top strand:
    /// topCutIndex = recStart (the 1-based G), so the upstream fragment keeps
    /// the "G" and the downstream fragment begins with "CTAGC"; the bottom
    /// strand is severed between the site's 5th and 6th bases (GCTAG^C),
    /// botCutIndex = recStart + 4. A cutter OUTSIDE the recognition site (e.g.
    /// BbsI, GAAGAC, topCutIndex = recStart + 7) leaves the intact GAAGAC on
    /// the upstream fragment while the 4-base 5' overhang belongs entirely to
    /// the downstream fragment — the sticky ends never overlap the recognition
    /// sequence, so the site survives digestion on the upstream side.
    /// DNA-only: rejects RNA/protein projects (no restriction sites).
    /// The response also carries `sequenceHash`/`revCompHash` (see
    /// get_project_overview).
    #[tool]
    async fn find_restriction_sites(
        &self,
        Parameters(request): Parameters<FindRestrictionSitesRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.find_restriction_sites_impl(request).await
    }

    /// List the primers stored in a project (read-only; never recomputes or
    /// checks binding). Returns {projectId, primers: [{id, name, type, seq,
    /// bindingSiteCount, sites: [{strand, templateStart, templateEnd}]}]}.
    /// templateStart/templateEnd are 1-based inclusive (the bound range spans
    /// templateStart..templateEnd, GenBank-style). bindingSiteCount is the
    /// number of recomputed binding sites (0 when the primer does not bind);
    /// sites are best-first (Tm descending, as the UI orders them).
    /// The response also carries `sequenceHash`/`revCompHash` (see
    /// get_project_overview).
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

    /// Open a sequence file, load it as a new project AND bind it as your
    /// agent tab in one step (project id = file path; the returned
    /// `projectId` is how every other tool refers to it — see list_projects).
    /// Binding means: the project stays in the main window's sidebar (marked
    /// with a bot badge) and is LOCKED against user keyboard/pointer input
    /// while you work (the user can temporarily unlock it via an on-screen
    /// button, but any further MCP tool call on the project re-locks it).
    /// This is the entry point for handing a file to the app: whenever a
    /// sequence already exists as a file on disk, bring it in through this
    /// tool rather than pasting its text into other tools. Files are also the
    /// recommended way to move a sequence between projects (write with
    /// save_file, read back with open_project). Enzyme and primer recompute
    /// run on a background thread; the UI is refreshed via broadcast.
    /// If the path is already loaded: a project already bound to you is
    /// re-locked and returns {ok, projectId, locked, reused: true}; a project
    /// the USER opened is REFUSED — copy the file with bash `cp` to a new
    /// path and open_project the copy. Mutating tools (edit_sequence,
    /// set_feature, add_primer, add_alignment, save_file, convert_sequence/apply,
    /// find_orfs/add_as_features) REFUSE to run on projects not bound as an
    /// agent tab. Multiple agents each open their own copy and work in
    /// parallel without interfering.
    /// A fresh open returns {ok, message, projectId, regionView} where
    /// regionView is the compact overview digest of the opened project
    /// (enzyme cutters collapsed to a count line); a reused binding returns
    /// {ok, message, projectId, locked, reused: true} WITHOUT regionView —
    /// call get_project_overview if you need the digest. Both forms carry
    /// `sequenceHash`/`revCompHash` (see get_project_overview).
    #[tool]
    async fn open_project(
        &self,
        Parameters(request): Parameters<OpenProjectRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.open_project_impl(request).await
    }

    /// Save a project (addressed by the required `project_id`) to a file on
    /// disk — the reverse of open_project.
    ///
    /// WITHOUT `region`: the whole project (current sequence + features) is
    /// written through the same serializer and mark-clean logic as the
    /// save_file command (.gbk/.gb/.genbank for DNA/RNA projects, .gpt for
    /// protein projects; topology preserved). Returns the uniform envelope
    /// with the
    /// overview digest plus `bytesWritten` (file size in bytes, for write
    /// verification).
    ///
    /// WITH `region`: exports only that subsequence — THE recommended way to
    /// create a sequence file from a known region of an open project (then
    /// open_project the result to work with it as a project). NEVER retype or
    /// paste the sequence into edit_sequence/other tools to build a new
    /// construct — pasted sequences are error-prone. The file holds the
    /// region's sequence (uppercase; template strand except as noted) plus
    /// every feature overlapping it (partially covered features are clipped
    /// to the region) with coordinates translated to the new linear
    /// coordinate system, and every primer whose primary binding site
    /// overlaps the region at all; circular projects always export linear
    /// fragments and the project is NOT marked clean. Exactly ONE selector
    /// inside `region` (mixing selectors is rejected):
    /// - `start` + `end`: 1-based inclusive template coordinates; on circular
    ///   sequences `start > end` wraps the origin.
    /// - `feature_id`: the feature's sequence with its segments joined in
    ///   biological order (5'→3', reverse-complemented for minus-strand
    ///   features). The exported feature spans the whole exported sequence;
    ///   other features overlapping its segments are carried along
    ///   (coordinates translated, strand flipped to match the rev-comp'd
    ///   orientation).
    /// - `enzyme1` + `enzyme2`: the fragment between the two enzymes' cut
    ///   sites (names — unknown names are rejected with near-match
    ///   suggestions, the same probe find_restriction_sites uses). Each
    ///   enzyme contributes the top-strand cut of its first recognition site
    ///   on the sequence; passing the same name twice uses that enzyme's
    ///   first two sites. On circular sequences the fragment is the forward
    ///   arc from enzyme1's cut to enzyme2's cut (wrapping the origin when
    ///   needed); on linear sequences the two cuts may be given in either
    ///   order.
    /// - `cut1` + `cut2` (alternative to the enzyme names): explicit cut
    ///   positions, 1-based — a cut at N severs the DNA between the 1-based
    ///   bases N and N+1 (valid range 1..=len; N = len is after the last base
    ///   on linear sequences, between the last and the first base on circular
    ///   ones; the fragment is [min, max-1] internal-0-based on linear
    ///   sequences, the forward arc on circular ones).
    /// - `fwd_primer` + `rev_primer`: the amplicon between the two primers'
    ///   binding sites. Each is a project primer name (stored binding sites
    ///   are used; name lookup wins) or a raw sequence (binding sites
    ///   recomputed with the primer engine, like check_primer_binding). The
    ///   fwd primer's best forward-strand site and the rev primer's best
    ///   reverse-strand site define the amplicon [fwdStart, revEnd]
    ///   (1-based inclusive) — the PCR product's top strand. A primer that
    ///   does not bind the strand its role needs is an error.
    ///
    /// Overwrite rule: when `path` already exists and is NOT the project's
    /// own source path, `overwrite: true` is required — otherwise the call
    /// fails with a hint to choose a different path or overwrite explicitly.
    /// A WHOLE-PROJECT save over the project's own file (scratch-copy
    /// iteration) needs no flag; a REGION export over the project's own file
    /// also requires `overwrite: true` (it would replace the full source
    /// file with just the fragment).
    ///
    /// Region mode returns {ok, message, projectId, outputPath, length,
    /// primers?, regionView?}: `length` is the exported sequence length
    /// (bp/nt/aa); `primers` lists the names of primers written with the file
    /// (omitted when none); `regionView` is a compact digest of the source
    /// project over the exported region's bounding box. The exported sequence
    /// itself is NOT echoed — read it back with open_project/read_sequence on
    /// the written file.
    /// The response also carries `sequenceHash`/`revCompHash` of the project's
    /// current in-memory sequence (see get_project_overview) — they describe
    /// the project, not the exported region.
    #[tool]
    async fn save_file(
        &self,
        Parameters(request): Parameters<SaveFileRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.save_file_impl(request).await
    }

    /// Close (unload) one of YOUR agent-tab projects from memory without
    /// saving. Only projects bound via open_project can be closed — the
    /// user's own projects are refused. Closing is NOT a file operation: the
    /// file on disk is untouched. A project with unsaved changes is refused
    /// unless `force: true` (save first with save_file, or force to discard).
    /// Mirrors delete_project; the UI updates via broadcast. Returns {ok,
    /// message, projectId}. The response also carries
    /// `sequenceHash`/`revCompHash` (see get_project_overview) of the project
    /// state just before closing.
    #[tool]
    async fn close_project(
        &self,
        Parameters(request): Parameters<CloseProjectRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.close_project_impl(request).await
    }

    /// Replace sequence [start..end] (1-based inclusive) with `replacement`
    /// (empty = delete). A pure insertion before base N is `start=N, end=N-1`;
    /// ranges must not wrap (start > end+1 rejected). The replacement sequence
    /// is given either
    /// as a plain string (`replacement`) or read from a local sequence file
    /// (`replacement_path` — .gbk/.gb/.genbank/.dna/.rna/.fasta/.fa/.ab1 etc.,
    /// the same formats open_project accepts; exactly one of the two must be
    /// given). PREFER `replacement_path`: a file cannot be mistyped or
    /// truncated, so whenever the insert already exists as a file — or is a
    /// region of an open project you can export first with save_file's
    /// `region` — use the file. Use the `replacement` string only for short
    /// hand-authored
    /// edits (point mutations, short oligo-length inserts). The optional
    /// `strand` parameter sets the insertion direction: "+" (default) inserts
    /// the replacement exactly as given; "-" reverse-complements it first
    /// (e.g. when the source sequence is oriented on the opposite strand) —
    /// DNA projects only, rejected on RNA/protein projects. When the
    /// replacement comes from `replacement_path` and that file carries
    /// annotations, they travel with the sequence: features are clipped to the
    /// inserted span and rebased onto it (mirrored and strand-flipped when
    /// strand="-"), and primers (DNA projects only) are added with binding
    /// sites recomputed; names colliding with existing features/primers get a
    /// " (2)" suffix. Feature coordinates are shifted/clipped
    /// for the edit (features fully inside a deleted range are removed). When
    /// `expected_old` is given it must match the current [start..end] content
    /// case-insensitively or the edit is rejected with the actual content. On
    /// such a mismatch the failure response carries `currentContent` — the
    /// authoritative current [start..end] bases — plus a ±20 bp `mismatch`
    /// context block; copy `currentContent` verbatim as `expected_old` and
    /// retry instead of hand-building a long check string. Uses the same
    /// recompute path as update_sequence: enzymes, primer binding sites,
    /// feature translations AND every stored read alignment are rebuilt (an
    /// edit moves the template under the reads), so alignment data read
    /// earlier in the session may be superseded — re-read the region view if
    /// you rely on it. A no-op edit is also the way to refresh alignments
    /// stored by an older engine. Returns
    /// newLength, old/new region views, 30 bp sequence context on each side of
    /// the edit, and side-effect echo `removedFeatures`/`clippedFeatures`
    /// (both always present, empty arrays when none): removed lists features
    /// fully inside the deleted/replaced span ({name, ftype, location} with
    /// the pre-edit 1-based "start..end"); clipped lists features where at
    /// least one segment actually lost or gained bases ({name, ftype, before,
    /// after} as 1-based {start, end} bounding spans plus beforeSegments/
    /// afterSegments with the individual 1-based ranges in join order; a
    /// feature whose segments all merely shifted — e.g. a cross-origin feature
    /// downstream of a deletion — is NOT clipped). An equal-length replacement
    /// (deleted length == inserted length) keeps ALL features at their
    /// current coordinates — nothing is removed or clipped, so case
    /// normalization and point-mutation edits are safe inside features. `transferredFeatures`/
    /// `transferredPrimers` list annotation names brought in by
    /// `replacement_path` (omitted when none). The replacement is normalized
    /// to uppercase on every molecule type (matching update_sequence). On
    /// protein projects it must additionally be amino-acid letters (A-Z,
    /// optional trailing '*' stop codon); lengths are reported in aa (nt for
    /// RNA, bp for DNA). On DNA projects any U in the replacement is
    /// converted to T, and on RNA projects T to U (a cross-alphabet source
    /// such as an .rna file inserted into a DNA project would otherwise
    /// silently pollute the sequence — the enzyme recompute does not
    /// recognize U); when any base is converted the response carries a
    /// `note` field describing the direction and count. The response also
    /// carries `sequenceHash`/`revCompHash` of the NEW sequence (see
    /// get_project_overview).
    #[tool]
    async fn edit_sequence(
        &self,
        Parameters(request): Parameters<EditSequenceRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.edit_sequence_impl(request).await
    }

    /// Create or update a feature in one tool.
    ///
    /// `feature_id` OMITTED = create: `name` and `ftype` are required, plus a
    /// span — `start`+`end` (1-based inclusive, GenBank convention) for a
    /// simple feature or `segments` ([{start, end}], 5'→3' order) for a
    /// segmented one; the two forms are mutually exclusive. strand (".", "+",
    /// "-", default "+") and color (hex, default "#60A5FA") are optional.
    /// Returns {ok, message, projectId, featureId, regionView} around the new
    /// feature.
    ///
    /// `feature_id` GIVEN = update that feature's attributes in one call:
    /// give at least one of name/ftype/color/strand/start+end/segments or the
    /// call is rejected. `start`+`end` replace the whole span, `segments`
    /// replaces the segment breakdown — the two forms are mutually exclusive
    /// and neither touches the strand. strand must be ".", "+" or "-"; color
    /// is hex and also recolors existing segments. Returns {ok, message,
    /// projectId, regionView} around the feature. The response also carries
    /// `sequenceHash`/`revCompHash` (see get_project_overview).
    #[tool]
    async fn set_feature(
        &self,
        Parameters(request): Parameters<SetFeatureRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.set_feature_impl(request).await
    }

    /// Add a primer ("fwd" or "rev") and recompute its binding sites against
    /// the template. Primer sequences are short (~20-60 nt), so passing `seq`
    /// as plain text is the intended input here — no file input needed.
    /// The primer `name` must not collide with an existing primer or FEATURE
    /// name in the project — a name taken by a feature is rejected (choose a
    /// distinct name, e.g. append "-F"/"-R").
    /// Returns {ok, message, projectId, bindingSites, regionView}
    /// — bindingSites: [{strand, templateStart, templateEnd, tm, annealLen}].
    /// templateStart/templateEnd are 1-based inclusive (the bound range spans
    /// templateStart..templateEnd, GenBank-style). annealLen is the number
    /// of contiguous 3'-end bases matching the template (the anneal core; a
    /// non-pairing 5' tail is excluded).
    /// DNA-only: rejects RNA/protein projects (single-strand molecules carry
    /// no primers). The response also carries `sequenceHash`/`revCompHash`
    /// (see get_project_overview).
    #[tool]
    async fn add_primer(
        &self,
        Parameters(request): Parameters<AddPrimerRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.add_primer_impl(request).await
    }

    /// Align a read against the project template and APPEND it as a new
    /// alignment (never overwrites existing ones; ids are aln-1, aln-2, ...).
    /// Provide exactly one of:
    /// - `bases`: the read sequence as a plain string (whitespace/non-ACGT
    ///   chars are stripped). Use ONLY for short hand-authored reads; pasted
    ///   long sequences are error-prone.
    /// - `path` (PREFERRED): read the sequence from a file. If the read lives
    ///   in a file, or is a region of an open project (export it first with
    ///   save_file's `region`), use this — a file cannot be mistyped or
    ///   truncated. Supported file types:
    ///   `.gbk`/`.gb`/`.genbank` (GenBank), `.dna`/`.rna`/`.prot` (SnapGene
    ///   binary), `.gpt` (protein GenBank), `.fa`/`.fasta` (FASTA / plain
    ///   text sequence), `.ab1` (ABIF chromatogram; the basecalled PBAS
    ///   sequence is extracted).
    /// Giving neither or both is an error. A name is always required.
    ///
    /// `algorithm` selects the alignment engine: "blast" (default; BLAST
    /// engine ported from GenePad's gene-core, modelled on the NCBI blastn
    /// algorithm — finds every colinear segment, so split/multi-hit reads
    /// and reads with unalignable junk tails align in full) or
    /// "smith-waterman" (classic single local block plus at most one flank;
    /// a read that spans two distant template loci may lose one of them).
    /// Prefer the default unless the user asks for Smith-Waterman.
    ///
    /// Returns {ok, message, projectId, regionView, significant, identity,
    /// strand, segmentCount, alignedLength, mismatches, insertions,
    /// deletions, mismatchDetails, deletionDetails, insertionDetails,
    /// orientedSequence, coverage, name, alignmentId, alignments}.
    /// - `identity`: 0–1 fraction, full precision (not rounded).
    /// - `alignedLength`: template positions covered by the alignment (sum of
    ///   segment spans, bp).
    /// - `mismatches`/`insertions`/`deletions`: total base counts (identity
    ///   alone rounds away single mismatches).
    /// - `mismatchDetails`: [{pos, templateBase, readBase}] — one entry per
    ///   mismatched column; `pos` is the 1-based inclusive template position;
    ///   `readBase` is oriented to the template strand (already rev-comp'd
    ///   when strand is "-").
    /// - `deletionDetails`: [{pos, length, bases}] — consecutive deleted
    ///   template columns grouped into one entry; `pos` is the 1-based
    ///   inclusive template position of the first deleted base; entries
    ///   straddling the circular origin are merged.
    /// - `insertionDetails`: [{pos, bases, length}] — the extra read bases sit
    ///   between the 1-based template bases `pos` and `pos + 1` (on circular
    ///   templates pos = len means between the last and the first base).
    /// - `orientedSequence`: the FULL read sequence oriented to the template
    ///   (reverse-complemented when strand is "-"), so read bases line up
    ///   with the template coordinates used by mismatchDetails/coverage —
    ///   eyeball a window's read bases directly instead of reconstructing
    ///   them from the diff lists. Returned untruncated; reads from .ab1
    ///   files can exceed 1000 bp.
    /// - `coverage`: [{start, end}] — 1-based inclusive template spans the
    ///   read covers, one entry per segment; a read spanning the circular
    ///   origin yields two entries.
    /// - `alignments`: the project's FULL alignment list (including the one
    ///   just added). Every entry carries the stats {alignmentId, name,
    ///   identity, strand, segmentCount, alignedLength, mismatches,
    ///   insertions, deletions, coverage}; only the newly added alignment is
    ///   expanded with `mismatchDetails`, `deletionDetails`,
    ///   `insertionDetails` and `orientedSequence` — previously stored reads
    ///   stay stats-only so multi-read responses don't balloon (in compact
    ///   mode too). Pass
    ///   `compact: true` to omit `orientedSequence` from the top-level
    ///   summary and from the new alignment's entry, and to skip
    ///   the post-alignment `regionView`; use `read_sequence` or
    ///   `get_region_view` when you need the bases.
    ///   FOCUS: pass `region` ({start, end} 1-based inclusive, start > end
    ///   wraps the origin on circular templates) or `feature_id` (a project
    ///   feature's bounding span; `flank` adds context bp on each side) to
    ///   focus the response on a window — the diff-detail lists of the new
    ///   alignment are filtered to the window, the full `orientedSequence`
    ///   is omitted, and the `regionView` shows the window (its ALIGNMENT
    ///   VIEW section renders the window's read bases column-by-column).
    ///   This is the recommended way to check "is this site mutated?"
    ///   without digesting a full-length read. The total
    ///   mismatches/insertions/deletions counts still describe the WHOLE
    ///   read, the response echoes the applied window as `focus`, and an
    ///   `outsideWindow` block ({mismatches, insertions, deletions}) gives
    ///   the diff base counts OUTSIDE the window (all zero = every
    ///   difference of this read is inside the window).
    ///   `compact: true` additionally suppresses the `regionView`.
    /// A `coverageNote` is added (top-level and on the new alignment's entry)
    /// when the read's coverage is multi-segment with uncovered template bp
    /// between the segments — the engine never produces such gaps for
    /// origin-spanning reads (their segments are adjacent), so a non-zero note
    /// means the template region between segments was not covered by this
    /// read, not that the alignment is broken.
    /// On failure returns {ok: false, message, projectId, significant: false};
    /// a message starting with "No significant alignment found" states the
    /// reason (identity below the 0.60 minimum, or aligned span below the
    /// 50 bp minimum). The response also carries
    /// `sequenceHash`/`revCompHash` of the template (see get_project_overview).
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

    /// Find open reading frames (ATG→stop, both strands, all frames) on a
    /// project. min_aa defaults to 75. When add_as_features is true the ORFs
    /// are appended as real CDS features (through the add-feature path, with
    /// recompute/broadcast) and {ok, message, projectId, regionView} is
    /// returned; otherwise returns {projectId, orfs: [Feature]} with all
    /// coordinates 1-based inclusive (start/end and segments).
    /// DNA-only: rejects RNA/protein projects (single-strand, no ORFs).
    /// The response also carries `sequenceHash`/`revCompHash` (see
    /// get_project_overview).
    #[tool]
    async fn find_orfs(
        &self,
        Parameters(request): Parameters<FindOrfsRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.find_orfs_impl(request).await
    }

    /// Design primer candidates — same modes/parameters as the
    /// design_primer_candidates command. mode: "amplify" | "oepcr" |
    /// "mutagenesis"; segments are {start, end} 1-based inclusive.
    /// amplify: optional `fwd_enzyme`/`rev_enzyme` (enzyme names, e.g.
    /// "BamHI" — probe valid names via find_restriction_sites' unknown-name
    /// suggestions) add a 5' tail of
    /// `protect_bases` (default 3) GC protection bases + the recognition
    /// site; candidates expose tail/tailLen/annealLen and Tm covers the
    /// actual contiguous 3' match (see the field table below for how
    /// designedTm relates to it). The amplify response always carries an
    /// `orientation` note: the product's top strand IS the template top
    /// strand of seg — Fwd primes from its 5' (left) end, Rev from its 3'
    /// (right) end — so primer names follow the template top strand, NOT any
    /// feature's coding strand. When seg overlaps a CDS feature the response
    /// adds `cdsOverlaps` ([{featureId, name, strand, note}]); for a
    /// minus-strand CDS the note spells out that Fwd sits at the CDS's 3'
    /// end and Rev at its 5' end. Map primer names to coding direction via
    /// that `strand` — never assume Fwd = CDS 5'.
    /// mutagenesis: `mut_seq` is the desired PLUS-strand content of `seg`
    /// after the edit; it must be the same length as `seg` and differ at
    /// <= 3 bases or the call fails with the current template sequence.
    /// STRAND WARNING (most common agent mistake): `mut_seq` is ALWAYS
    /// PLUS-strand (template top-strand) content, even when the CDS you are
    /// editing is on the minus strand — for a minus-strand CDS you must
    /// reverse-complement the intended coding-strand edit yourself (e.g. a
    /// coding-strand GCG→AAG Ala→Lys change is `mut_seq: "CTT"`, the rev-comp
    /// of AAG). If you pass coding-strand sequence instead, the self-check
    /// block will show the WRONG amino acid.
    /// The response includes a `mutation` self-check block (diffs, plus/minus
    /// strand context, and CDS codon/amino-acid change when `seg` lies inside
    /// a CDS — joined multi-segment CDS features are supported — mind the CDS
    /// strand: for a minus-strand CDS the coding change is the reverse
    /// complement of the plus-strand edit) plus an `orientationHint` string
    /// that restates the strand semantics WITH the actual outcome (CDS
    /// strand, codonAfter, amino acid after) — ALWAYS read `aaAfter`/
    /// `orientationHint` and confirm it is the amino acid you intended before
    /// using the primers. In that block `segStart`/`segEnd`
    /// are 1-based inclusive template coordinates and each diff's `offset` is
    /// the 1-based position within `seg`; `cds.codonIndex`
    /// is 1-based within the CDS (the codon that changes) and the amino-acid
    /// position is reported in
    /// TWO conventions: `cds.aaPosition1Based` counts the initiator Met as
    /// residue 1 (always equal to codonIndex), while
    /// `cds.aaPositionExcludingMet`
    /// excludes it (absent for the first codon) — the latter matches common
    /// literature numbering, e.g. mEGFP A206K shows up as
    /// aaPositionExcludingMet=206 / aaPosition1Based=207. Check which
    /// convention your task's numbering uses.
    /// Replacing every base of `seg`
    /// adds a `warning` (likely wrong strand/location) but is not rejected —
    /// EXCEPT when `seg` is exactly one or more complete codons of a CDS
    /// (codon-aligned, length divisible by 3, CDS context computable): a
    /// whole-codon swap (e.g. Ala→Lys, GCG→AAG) is an expected operation and
    /// does NOT warn. The warning is kept whenever the CDS context cannot be
    /// confirmed (seg outside any CDS, or not codon-aligned).
    /// In amplify mode the response always includes an `internalSites` array
    /// (empty when no enzyme recognition site occurs inside the amplified
    /// segment; non-empty entries {enzyme, start, end, strand}, 1-based
    /// inclusive, plus a `warning` that digestion would cut the product).
    /// Returns {projectId, groups: [PrimerGroup], mutation?, tmBasis,
    /// internalSites + orientation + cdsOverlaps? (amplify)}.
    ///
    /// Each PrimerGroup is {name, type: "fwd"|"rev", candidates,
    /// defaultIndex}: `candidates` are length variants ordered by anneal-core
    /// length ascending, and `defaultIndex` points at the RECOMMENDED
    /// candidate — the one whose Tm is closest to `target_tm` — use
    /// `groups[i].candidates[groups[i].defaultIndex]` instead of guessing.
    /// Each candidate is {seq, tail, tailLen, annealLen, tm, gc,
    /// designedAnnealLen?, designedTm?}:
    /// - `seq`: full primer sequence 5'→3' (tail + anneal core).
    /// - `tail`: 5' tail sequence (empty when the primer has no tail);
    ///   `tailLen` is its length in bases.
    /// - `annealLen`: anneal-core length in bases — the ACTUAL contiguous 3'
    ///   match against the template after unification.
    /// - `tm`: melting temperature (°C) of that actual contiguous 3' match,
    ///   rounded to 0.1. Because a 5' tail can accidentally pair with the
    ///   template adjacent to the designed site, `tm` may exceed the designed
    ///   core Tm for tailed primers.
    /// - `gc`: GC fraction of the FULL `seq`, one decimal.
    /// - `designedTm`/`designedAnnealLen`: the anneal-core values BEFORE
    ///   3'-end unification, preserved for reference and omitted when they
    ///   equal `tm`/`annealLen`. For PCR annealing temperature of a
    ///   5'-tailed primer, reference `designedTm` (the anneal core you
    ///   designed); `tm` is the actual 3' contiguous match that may include
    ///   accidental tail pairing.
    /// `tmBasis` always restates this basis.
    /// DNA-only: rejects RNA/protein projects (no primer design on
    /// single-strand molecules). The response also carries
    /// `sequenceHash`/`revCompHash` of the template (see get_project_overview).
    #[tool]
    async fn design_primers(
        &self,
        Parameters(request): Parameters<DesignPrimersRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.design_primers_impl(request).await
    }

    /// Check whether the given primers (each {name, type: "fwd"|"rev", seq})
    /// can bind to a project's sequence, without persisting them. Primer
    /// sequences are short (~20-60 nt), so plain text is the intended input
    /// here. Same engine as check_primers_binding. Returns {projectId, tmBasis, results: [{id,
    /// binds, bindingSiteCount, site, sites}]}. `bindingSiteCount` is the
    /// number of binding sites (0 when the primer does not bind); `site` is
    /// the best one ({strand, templateStart, templateEnd, tm, annealLen,
    /// mismatchedTail, alignedTemplate, matchMask} or null) and `sites`
    /// lists ALL sites best-first (Tm
    /// descending, same field shape as `site`) — use `sites` for off-target
    /// detection. templateStart/templateEnd are 1-based inclusive (the bound
    /// range spans templateStart..templateEnd, GenBank-style). `binds: true`
    /// means the 3' anneal core matched —
    /// the primer may still carry mismatches at its 5' end. `mismatchedTail`
    /// is the number of 5'-most bases NOT part of the contiguous 3' match
    /// (0 when the whole primer anneals; >0 for mutagenesis primers and
    /// enzyme-tail primers). `annealLen` counts only the contiguous 3' match.
    /// Every site also carries a full-length template coverage view:
    /// `alignedTemplate` and `matchMask` are exactly the primer's length,
    /// 5'→3' — `alignedTemplate` holds the template base each primer position
    /// faces (complemented for strand -1 so it compares directly against the
    /// primer; '-' where a 5' tail hangs off the end of a LINEAR template)
    /// and `matchMask` marks each position '|' (match), '.' (mismatch) or
    /// '-' (no template base). When `mismatchedTail` > 0, 5' tail bases that
    /// happen to match the template bases adjacent to the anneal core extend
    /// annealLen and raise Tm beyond design_primers' values — expected, not
    /// anomalous binding; read the mask to see exactly which tail bases pair.
    /// `tmBasis` (always present) states this Tm/annealLen basis. Unlike
    /// design_primers (which reports the DESIGNED anneal core), this tool
    /// recomputes the ACTUAL contiguous 3'-end match — the canonical case is
    /// an enzyme-tail primer (e.g. GCG+GGATCC+anneal core) whose tail's 3'
    /// side matches the template next to the binding site, so check's
    /// annealLen/Tm come out higher than design's.
    /// Primer-pair amplicon size can be derived from the binding sites
    /// reported here: the product spans the fwd primer's forward-strand site
    /// start (templateStart) to the rev primer's reverse-strand site end
    /// (templateEnd), inclusive. To obtain the amplicon itself (as a file),
    /// use save_file's fwd_primer/rev_primer region mode.
    /// DNA-only: rejects RNA/protein projects (no primer binding on
    /// single-strand molecules). The response also carries
    /// `sequenceHash`/`revCompHash` of the template (see get_project_overview).
    #[tool]
    async fn check_primer_binding(
        &self,
        Parameters(request): Parameters<CheckPrimerBindingRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.check_primer_binding_impl(request).await
    }

    /// Convert sequences between molecule types — DNA/RNA/protein — with
    /// codon optimization where it applies. BATCH: `items` takes 1-64
    /// independent conversion items; a failing item does not abort the others
    /// (its slot in `results` carries {ok: false, error}). A single
    /// conversion may omit `items` and put the item fields at the top level.
    ///
    /// Conversion matrix (per item, `from`/`to` ∈ "dna" | "rna" | "protein"):
    /// - dna→dna: codon optimization when `species` (or another optimizer
    ///   parameter) is given, else the sequence passes through unchanged;
    ///   `revComp: true` reverse-complements (not combinable with
    ///   optimization). Project mode lives here.
    /// - dna→rna / rna→dna: T↔U conversion (optional revComp).
    /// - dna→protein / rna→protein: translation (frame 0; a trailing
    ///   partial codon is dropped).
    /// - protein→dna / protein→rna: REVERSE TRANSLATION with codon
    ///   optimization (`species` required — a key from list_species such as
    ///   "e_coli", "h_sapiens"; `method` = use_best_codon (default) |
    ///   match_codon_usage | harmonize_rca, the latter using
    ///   `original_species` as the source table; `avoid_enzyme_sites` takes
    ///   IUPAC recognition sequences to avoid).
    /// - protein→protein: rejected. revComp with a protein side: rejected.
    /// `from` defaults: project mode → dna; `input_path` → the file's
    /// molecule type; `sequence` → dna. `to` defaults: dna for a protein
    /// input, otherwise same as `from`.
    ///
    /// Exactly one input mode per item:
    /// - `project_id` + `feature_id` (both required; dna→dna codon
    ///   optimization ONLY, DNA projects only): optimize the CDS/mRNA feature
    ///   inside an open project. `apply=false` (default) is a read-only
    ///   preview; `apply=true` replaces the feature's coding bases in the
    ///   template (equal-length synonymous substitution, coordinates
    ///   unchanged) through the same recompute+broadcast path as
    ///   edit_sequence. `output_path` is REJECTED here — apply then save_file.
    /// - `sequence`: raw sequence text (whitespace/digits ignored). Use ONLY
    ///   for short hand-authored sequences — pasted long sequences are
    ///   error-prone, so whenever the sequence exists as a file use
    ///   `input_path`, and when it is a region of an open project export it
    ///   first with save_file's `region`.
    /// - `input_path` (PREFERRED for real sequences): local file parsed with
    ///   file_io (.gbk/.gb/.genbank/.dna/.rna/.fasta/.fa/.ab1 nucleotide,
    ///   .gpt/.prot protein). A file cannot be mistyped or truncated. With
    ///   `feature_id` on a DNA file + `species`, that file CDS is optimized
    ///   and the written sequence carries the full file with the CDS
    ///   replaced.
    ///
    /// Returns {ok, results: [{index, ok, from, to, sequence?, length?,
    /// message?, path?, projectId?, regionView?, error?, ...}]}. Successful
    /// items carry the converted `sequence` text and its `length`; codon-
    /// optimizing items additionally carry the optimizer fields (aa,
    /// codonCount, newCodons, caiBefore, caiAfter, gcBefore, gcAfter, repairs,
    /// repairCount, unresolved, method, species). `aa` includes a trailing
    /// '*' for the stop codon and `codonCount` counts it. repairs[].codonIndex
    /// is 1-based (1 = first codon, including the stop) and each unresolved
    /// entry is "<reason> <start>..<end>" with 1-based inclusive base offsets
    /// within the optimized sequence. `path` appears when
    /// `output_path` was given; `regionView` after an apply=true project
    /// write-back. Failed items carry {ok: false, error}; when EVERY item
    /// fails the whole call returns an error.
    ///
    /// `output_path` (sequence/input_path modes only): .gbk/.gb/.genbank →
    /// GenBank of the output molecule (DNA/RNA with the CDS annotated),
    /// .gpt → protein GenBank (protein output only), .fa/.fasta/.txt → bare
    /// sequence text. When the output carries a single whole-length CDS, the
    /// CDS feature is labeled after the SOURCE file stem (falling back to the
    /// output file stem). PREFER writing the result to a file (and
    /// open_project it afterwards) over reading the `sequence` text —
    /// sequences move between tools as files, not pasted text. When
    /// `output_path` already exists, `overwrite: true` is required (same rule
    /// as save_file).
    /// Each result also carries `sequenceHash`/`revCompHash` (see
    /// get_project_overview) of the INPUT sequence: project mode → the
    /// project after a possible apply; input_path → the input file's
    /// sequence; sequence → the given text.
    #[tool]
    async fn convert_sequence(
        &self,
        Parameters(request): Parameters<ConvertSequenceRequest>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        self.convert_sequence_impl(request).await
    }

}

#[tool_handler(name = "LibreGene", instructions = "Agent tabs: open_project opens a sequence file AND binds it as your agent tab in the main window in one step (locked against user input; every tool call re-locks it). A path that is already open but not bound belongs to the user — copy the file with bash `cp` to a new path and open_project the copy. Mutating tools refuse projects not bound as an agent tab. Files over pasted text: whenever a sequence exists as a file (or can be written to one), prefer file-based I/O over pasting sequence text into tool arguments — pasted sequences are error-prone (transcription slips, truncation, wrong strand). Open sequence files with open_project; insert/replace from a file via edit_sequence's replacement_path; hand reads to add_alignment via path; feed convert_sequence via input_path and collect its result via output_path; to create a new file from a known region of an open project, save_file with `region` (by coordinates, feature, enzymes/cuts, or primers) then open_project the result — never retype the sequence into another tool. Plain-text sequence parameters stay available for short hand-authored input (primers ~20-60 nt, point mutations, short inserts) or when no file exists. read_sequence is for inspecting bases (and resolving coordinates), not for moving sequences between tools. Every tool takes a required project_id; list_projects' activeId is the project the user is viewing (informational only) — avoid it when several agents work in parallel.")]
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
