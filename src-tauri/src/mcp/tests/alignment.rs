use super::common::*;
use crate::mcp::*;
use libregene_core::models::ProjectData;

    #[tokio::test]
    async fn add_alignment_returns_oriented_sequence_and_coverage() {
        let project = alignment_test_project("linear");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        // Read = template[50..150] with one base flipped at index 60 (pos 110).
        let mut read = template[50..150].to_string();
        let i = 60;
        let orig = read.as_bytes()[i];
        let flipped = if orig == b'A' { b'C' } else { b'A' };
        read.replace_range(i..i + 1, &(flipped as char).to_string());

        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "read1".to_string(),
                bases: Some(read.clone()),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["strand"], "+");
        assert_eq!(v["orientedSequence"], read, "{v}");
        assert_eq!(
            v["coverage"],
            serde_json::json!([{ "start": 51, "end": 150 }]),
            "{v}"
        );
        assert_eq!(
            v["mismatchDetails"],
            serde_json::json!([{
                "position": 111,
                "templateBase": (orig as char).to_string(),
                "readBase": (flipped as char).to_string(),
            }]),
            "{v}"
        );
        // The alignments array carries the same new fields per entry.
        let entry = &v["alignments"][0];
        assert_eq!(entry["orientedSequence"], read, "{entry}");
        assert_eq!(
            entry["coverage"],
            serde_json::json!([{ "start": 51, "end": 150 }]),
            "{entry}"
        );

        // Region view over the mismatch lists it in the diff section.
        let out = server
            .get_region_view(Parameters(RegionRequest {
                project_id: "aln_test".to_string(),
                start: 101,
                end: 121,
                ..Default::default()
            }))
            .await
            .unwrap();
        let text = out.0["text"].as_str().unwrap().to_string();
        assert!(text.contains("ALIGNMENT DIFFS IN REGION"), "{text}");
        assert!(
            text.contains(&format!("mismatch at 111: {} > {}", orig as char, flipped as char)),
            "{text}"
        );

        // Window overlapping the read but not the diff.
        let out = server
            .get_region_view(Parameters(RegionRequest {
                project_id: "aln_test".to_string(),
                start: 51,
                end: 61,
                ..Default::default()
            }))
            .await
            .unwrap();
        let text = out.0["text"].as_str().unwrap().to_string();
        assert!(text.contains("no differences in window"), "{text}");
        assert!(!text.contains("mismatch at 111"), "{text}");

        // Window outside the read: no diff section at all.
        let out = server
            .get_region_view(Parameters(RegionRequest {
                project_id: "aln_test".to_string(),
                start: 1,
                end: 41,
                ..Default::default()
            }))
            .await
            .unwrap();
        let text = out.0["text"].as_str().unwrap().to_string();
        assert!(!text.contains("ALIGNMENT DIFFS IN REGION"), "{text}");
    }

    #[tokio::test]
    async fn add_alignment_focus_region_filters_diffs_and_omits_oriented_sequence() {
        let project = alignment_test_project("linear");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        // Read = template[50..150] with mismatches at pos 61 (outside the
        // focus window) and pos 111 (inside).
        let mut read = template[50..150].to_string();
        for i in [10usize, 60] {
            let orig = read.as_bytes()[i];
            let flipped = if orig == b'A' { b'C' } else { b'A' };
            read.replace_range(i..i + 1, &(flipped as char).to_string());
        }

        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "focused".to_string(),
                bases: Some(read),
                path: None,
                region: Some(SegParam { start: 100, end: 120 }),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        // Only the in-window mismatch is detailed; totals stay global.
        let details = v["mismatchDetails"].as_array().unwrap();
        assert_eq!(details.len(), 1, "{v}");
        assert_eq!(details[0]["position"], 111, "{v}");
        assert_eq!(v["mismatches"], 2, "{v}");
        // Full read omitted; focused text + window echo present.
        assert!(v.get("orientedSequence").is_none(), "{v}");
        assert!(v.get("text").is_some(), "{v}");
        assert_eq!(v["window"]["start"], 100, "{v}");
        assert_eq!(v["window"]["end"], 120, "{v}");
        // window counts the in-window mismatch; the top-level total stays global.
        assert_eq!(v["window"]["mismatches"], 1, "{v}");
        assert_eq!(
            v["window"],
            serde_json::json!({
                "start": 100,
                "end": 120,
                "featureId": serde_json::Value::Null,
                "flank": 0,
                "mismatches": 1,
                "insertions": 0,
                "deletions": 0,
                "note": "mismatchDetails/deletionDetails/insertionDetails are filtered to this window; total mismatches/insertions/deletions still describe the whole read",
            }),
            "{v}"
        );
        // The alignments entry mirrors the filtering.
        let entry = &v["alignments"][0];
        assert!(entry.get("orientedSequence").is_none(), "{entry}");
        assert_eq!(entry["mismatchDetails"].as_array().unwrap().len(), 1, "{entry}");
    }

    #[tokio::test]
    async fn add_alignment_focus_feature_id_and_validation() {
        let mut project = alignment_test_project("linear");
        // 0-based 100..110 -> 1-based 101..111.
        project.features = vec![feature("f1", "site", 100, 110, "+")];
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let mut read = template[50..150].to_string();
        let i = 60; // mismatch at 1-based pos 111, inside the feature span
        let orig = read.as_bytes()[i];
        let flipped = if orig == b'A' { b'C' } else { b'A' };
        read.replace_range(i..i + 1, &(flipped as char).to_string());

        // feature_id focus with flank 5 -> window 96..116.
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "ffocus".to_string(),
                bases: Some(read.clone()),
                path: None,
                feature_id: Some("f1".to_string()),
                flank: Some(5),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["window"]["start"], 96, "{v}");
        assert_eq!(v["window"]["end"], 116, "{v}");
        assert_eq!(v["window"]["featureId"], "f1", "{v}");
        assert_eq!(v["window"]["mismatches"], 1, "{v}");
        assert_eq!(v["window"]["flank"], 5, "{v}");
        assert_eq!(v["mismatchDetails"].as_array().unwrap().len(), 1, "{v}");

        // Unknown feature id is rejected before aligning.
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "bad".to_string(),
                bases: Some(read.clone()),
                path: None,
                feature_id: Some("nope".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("not found"),
            "{}",
            out.0
        );

        // region + feature_id together are rejected.
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "both".to_string(),
                bases: Some(read),
                path: None,
                region: Some(SegParam { start: 1, end: 10 }),
                feature_id: Some("f1".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"]
                .as_str()
                .unwrap()
                .contains("mutually exclusive"),
            "{}",
            out.0
        );
    }

    #[tokio::test]
    async fn add_alignment_reverse_strand_oriented_sequence_is_revcomp() {
        let project = alignment_test_project("linear");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let read = libregene_core::utils::reverse_complement(&template[50..120]);
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "rev_read".to_string(),
                bases: Some(read),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["strand"], "-", "{v}");
        // Oriented to the template: the rev-comp of the raw read.
        assert_eq!(v["orientedSequence"], template[50..120], "{v}");
        assert_eq!(
            v["coverage"],
            serde_json::json!([{ "start": 51, "end": 120 }]),
            "{v}"
        );
    }

    #[tokio::test]
    async fn add_alignment_circular_coverage_splits_at_origin() {
        let project = alignment_test_project("circular");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let read = format!("{}{}", &template[170..200], &template[0..25]);
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "wrap_read".to_string(),
                bases: Some(read.clone()),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert_eq!(v["orientedSequence"], read, "{v}");
        assert_eq!(
            v["coverage"],
            serde_json::json!([{ "start": 171, "end": 200 }, { "start": 1, "end": 25 }]),
            "{v}"
        );
    }

    #[tokio::test]
    async fn add_alignment_compact_omits_oriented_sequence_and_region_view() {
        let project = alignment_test_project("linear");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let read = template[50..150].to_string();

        // compact=true drops orientedSequence and text, keeps coverage.
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "compact_read".to_string(),
                bases: Some(read.clone()),
                path: None,
                compact: Some(true),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        assert!(v.get("orientedSequence").is_none(), "{v}");
        assert!(v.get("text").is_none(), "{v}");
        assert_eq!(
            v["coverage"],
            serde_json::json!([{ "start": 51, "end": 150 }]),
            "{v}"
        );
        let entry = &v["alignments"][0];
        assert!(entry.get("orientedSequence").is_none(), "{entry}");
        assert!(entry.get("coverage").is_some(), "{entry}");

        // compact=false / omitted keeps orientedSequence and text.
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "full_read".to_string(),
                bases: Some(read),
                path: None,
                compact: Some(false),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert!(v.get("orientedSequence").is_some(), "{v}");
        assert!(v.get("text").is_some(), "{v}");
        // alignments[1] is the newly added read → full detail; alignments[0]
        // is the earlier compact read → stats-only (no orientedSequence,
        // no per-column details).
        assert!(v["alignments"][1].get("orientedSequence").is_some(), "{v}");
        assert!(v["alignments"][0].get("orientedSequence").is_none(), "{v}");
        assert!(v["alignments"][0].get("mismatchDetails").is_none(), "{v}");
        assert!(v["alignments"][0].get("coverage").is_some(), "{v}");
    }

    #[tokio::test]
    async fn add_alignment_slims_existing_alignments_in_full_responses() {
        let project = alignment_test_project("linear");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let mut read1 = template[50..150].to_string();
        let i = 60;
        let orig = read1.as_bytes()[i];
        let flipped = if orig == b'A' { b'C' } else { b'A' };
        read1.replace_range(i..i + 1, &(flipped as char).to_string());
        server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "read1".to_string(),
                bases: Some(read1.clone()),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();

        let read2 = template[60..130].to_string();
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "read2".to_string(),
                bases: Some(read2),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["alignments"].as_array().unwrap().len(), 2, "{v}");
        // The new read is fully expanded.
        assert_eq!(v["alignments"][1]["name"], "read2", "{v}");
        assert!(v["alignments"][1].get("orientedSequence").is_some(), "{v}");
        // The previously stored read is stats-only.
        assert_eq!(v["alignments"][0]["name"], "read1", "{v}");
        assert!(v["alignments"][0].get("orientedSequence").is_none(), "{v}");
        assert!(v["alignments"][0].get("mismatchDetails").is_none(), "{v}");
        assert!(v["alignments"][0].get("deletionDetails").is_none(), "{v}");
        assert!(v["alignments"][0].get("insertionDetails").is_none(), "{v}");
        for key in [
            "alignmentId",
            "name",
            "identity",
            "strand",
            "segmentCount",
            "alignedLength",
            "mismatches",
            "insertions",
            "deletions",
            "coverage",
        ] {
            assert!(v["alignments"][0].get(key).is_some(), "missing {key}: {v}");
        }
        // No coverage gaps for a single-segment read: no coverageNote.
        assert!(v.get("coverageNote").is_none(), "{v}");
    }

    #[test]
    fn uncovered_between_segments_measures_gaps_only() {
        use libregene_core::models::{AlignSegment, Alignment};
        let aln = |segs: Vec<(usize, usize, &str)>| Alignment {
            id: String::new(),
            name: String::new(),
            length: 0,
            strand: "+".into(),
            identity: 1.0,
            segments: segs
                .into_iter()
                .map(|(s, e, c)| AlignSegment {
                    start: s,
                    end: e,
                    chars: c.to_string(),
                })
                .collect(),
            insertions: Vec::new(),
            seq: String::new(),
            trace_path: None,
        };
        // Single segment: no gap.
        assert_eq!(uncovered_between_segments(&aln(vec![(10, 29, "x")]), 60, false), 0);
        // Two segments with a 10 bp hole between them (linear).
        assert_eq!(
            uncovered_between_segments(&aln(vec![(10, 29, "x"), (40, 49, "y")]), 60, false),
            10
        );
        // Origin-spanning circular read: segments are adjacent at the wrap.
        assert_eq!(
            uncovered_between_segments(&aln(vec![(50, 59, "x"), (0, 9, "y")]), 60, true),
            0
        );
        // Circular segments with a real hole.
        assert_eq!(
            uncovered_between_segments(&aln(vec![(10, 29, "x"), (40, 49, "y")]), 60, true),
            10
        );
    }

    #[test]
    fn focus_filter_counts_partial_deletion_overlap() {
        // A deletion kept by a partial overlap must count only its in-window
        // bases against the window, not its full length.
        let mut v = serde_json::json!({
            "mismatches": 0, "insertions": 0, "deletions": 10,
            "mismatchDetails": [],
            "insertionDetails": [],
            "deletionDetails": [{"position": 8, "length": 10, "bases": "XXXXXXXXXX"}],
        });
        // In-window base counts: (mismatches, insertions, deletions) — only the
        // 3 bases of the deletion inside 1..10 count.
        assert_eq!(
            filter_alignment_json_focus(&mut v, 1, 10, 100, false),
            (0, 0, 3)
        );
        assert_eq!(v["deletionDetails"].as_array().unwrap().len(), 1, "{v}");
    }

    #[test]
    fn focus_filter_wraps_origin_merged_deletion() {
        // Circular tlen=20: a merged deletion at 1-based pos 18 length 5
        // covers bases 18,19,20,1,2 (coordinates past tlen wrap back).
        let mk = || serde_json::json!({
            "mismatches": 0, "insertions": 0, "deletions": 5,
            "mismatchDetails": [],
            "insertionDetails": [],
            "deletionDetails": [{"position": 18, "length": 5, "bases": "XXXXX"}],
        });
        let mut v = mk();
        // Window 1..3 covers the wrapped arc 1,2 → 2 in-window bases.
        assert_eq!(filter_alignment_json_focus(&mut v, 1, 3, 20, true), (0, 0, 2));
        assert_eq!(v["deletionDetails"].as_array().unwrap().len(), 1, "{v}");
        let mut v = mk();
        // Window 18..20 covers the terminal arc → 3 in-window bases.
        assert_eq!(filter_alignment_json_focus(&mut v, 18, 20, 20, true), (0, 0, 3));
        // A window touching neither arc drops the entry entirely.
        let mut v = mk();
        assert_eq!(filter_alignment_json_focus(&mut v, 5, 10, 20, true), (0, 0, 0));
        assert_eq!(v["deletionDetails"].as_array().unwrap().len(), 0, "{v}");
    }

    #[tokio::test]
    async fn add_alignment_compact_slims_history_and_focuses_new_diffs() {
        let project = alignment_test_project("linear");
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "r1".to_string(),
                bases: Some(template[50..150].to_string()),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], true, "{}", out.0);

        // Second read with mismatches at 1-based 61 (outside focus) and 111
        // (inside), added with compact + focus: the history entry must be
        // stats-only (previously compact kept FULL details for history) and
        // the new entry must be focus-filtered with in-window counts
        // (previously compact skipped the focus filter entirely).
        let mut read2 = template[50..150].to_string();
        for i in [10usize, 60] {
            let orig = read2.as_bytes()[i];
            let flipped = if orig == b'A' { b'C' } else { b'A' };
            read2.replace_range(i..i + 1, &(flipped as char).to_string());
        }
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "aln_test".to_string(),
                name: "r2".to_string(),
                bases: Some(read2),
                path: None,
                compact: Some(true),
                region: Some(SegParam { start: 100, end: 120 }),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        let hist = &v["alignments"][0];
        assert!(hist.get("mismatchDetails").is_none(), "history stats-only: {hist}");
        let new = &v["alignments"][1];
        let det = new["mismatchDetails"].as_array().unwrap();
        assert_eq!(det.len(), 1, "{new}");
        assert_eq!(det[0]["position"], 111, "{new}");
        assert!(new.get("orientedSequence").is_none(), "{new}");
        assert_eq!(v["window"]["start"], 100, "{v}");
        assert_eq!(v["window"]["mismatches"], 1, "{v}");
        assert_eq!(v["mismatches"], 2, "total stays whole-read: {v}");
        assert!(v.get("text").is_none(), "compact suppresses text: {v}");
    }

    #[tokio::test]
    async fn add_alignment_reports_destroyed_and_created_sites() {
        // 200 bp template with a single top-strand BbsI site (GAAGAC) and no
        // BamHI site. A read that flips the BbsI site and installs GGATCC
        // elsewhere must report both a destroyed and a created site.
        let mut seq = "ACGT".repeat(50);
        seq.replace_range(40..46, "GAAGAC");
        let mut project = ProjectData {
            name: "sites".to_string(),
            sequence: seq,
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        libregene_core::enzyme::recompute(&mut project);
        assert!(
            project.enzymes.iter().any(|e| e.name == "BbsI" && e.rec_start == 40),
            "test template must carry the BbsI site"
        );
        assert!(
            !project.enzymes.iter().any(|e| e.name == "BamHI"),
            "test template must not carry a BamHI site"
        );
        let template = project.sequence.clone();
        let server = handler_with_project(project).await;

        let mut read = template.clone();
        read.replace_range(42..43, "T"); // GAAGAC -> GTAGAC destroys BbsI
        read.replace_range(100..106, "GGATCC"); // installs a BamHI site

        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "sites".to_string(),
                name: "read".to_string(),
                bases: Some(read),
                path: None,
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        let sites = v["affectedSites"].as_array().expect("affectedSites array");
        let bbs = sites
            .iter()
            .find(|s| s["enzyme"] == "BbsI")
            .expect("BbsI entry");
        assert_eq!(bbs["status"], "destroyed", "{bbs}");
        assert_eq!(bbs["recStart"], 41, "{bbs}");
        assert_eq!(bbs["recEnd"], 46, "{bbs}");
        assert_eq!(bbs["changedBases"][0]["position"], 43, "{bbs}");
        assert_eq!(bbs["changedBases"][0]["templateBase"], "A", "{bbs}");
        assert_eq!(bbs["changedBases"][0]["readBase"], "T", "{bbs}");
        let bam = sites
            .iter()
            .find(|s| s["enzyme"] == "BamHI")
            .expect("BamHI entry");
        assert_eq!(bam["status"], "created", "{bam}");
        assert_eq!(bam["recStart"], 101, "{bam}");
        assert_eq!(bam["recEnd"], 106, "{bam}");
        // Intact sites are hidden unless requested.
        assert!(!sites.iter().any(|s| s["status"] == "intact"), "{v}");
        // The new alignment entry mirrors the impact block.
        assert!(
            v["alignments"][0]["affectedSites"].as_array().is_some(),
            "{v}"
        );

        // An exact-match read (includeIntactSites) lists still-matching sites.
        let out = server
            .add_alignment(Parameters(AddAlignmentRequest {
                project_id: "sites".to_string(),
                name: "read2".to_string(),
                bases: Some(template),
                path: None,
                include_intact_sites: Some(true),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        let sites = v["affectedSites"].as_array().unwrap();
        assert!(
            sites
                .iter()
                .any(|s| s["enzyme"] == "BbsI" && s["status"] == "intact"),
            "{v}"
        );
    }
