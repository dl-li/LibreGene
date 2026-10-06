use super::common::*;
use crate::mcp::*;
use libregene_core::models::ProjectData;

    #[tokio::test]
    async fn design_primers_amplify_reports_orientation_and_cds_strand() {
        // Minus-strand CDS overlapping the seg: the response must spell out
        // the product orientation and the CDS strand so Fwd/Rev are not
        // misread as coding-direction names.
        let project = ProjectData {
            name: "amp_test".to_string(),
            sequence: synthetic_dna(200, 5),
            length: 200,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("cds1", "mEGFP", 60, 120, "-")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "amp_test".to_string(),
                mode: "amplify".to_string(),
                seg: Some(SegParam { start: 51, end: 151 }),
                name: Some("Amp".to_string()),
                target_tm: 55.0,
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert!(v["groups"].as_array().is_some_and(|g| g.len() == 2), "{v}");
        let orientation = v["orientation"].as_str().expect("orientation note");
        assert!(orientation.contains("seg 51..151"), "{orientation}");
        assert!(orientation.contains("template top strand"), "{orientation}");
        let overlaps = v["cdsOverlaps"].as_array().expect("cdsOverlaps");
        assert_eq!(overlaps.len(), 1, "{v}");
        assert_eq!(overlaps[0]["name"], "mEGFP");
        assert_eq!(overlaps[0]["strand"], "-");
        assert!(
            overlaps[0]["note"].as_str().unwrap().contains("MINUS strand"),
            "{}",
            overlaps[0]["note"]
        );
        assert!(v["internalSites"].as_array().unwrap().is_empty(), "{v}");
    }

    #[tokio::test]
    async fn design_primers_mutagenesis_whole_codon_skips_warning() {
        // Codon-aligned full replacement inside a CDS: no warning. The same
        // full replacement OUTSIDE any CDS keeps the warning.
        let mut bytes = vec![b'A'; 90];
        bytes[60] = b'C';
        bytes[61] = b'G';
        bytes[62] = b'C';
        let project = ProjectData {
            name: "mut_test".to_string(),
            sequence: String::from_utf8(bytes).unwrap(),
            length: 90,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("cds1", "orf", 30, 89, "+")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;

        // Whole-codon swap CGC -> AAA (Arg -> Lys): expected operation.
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "mut_test".to_string(),
                mode: "mutagenesis".to_string(),
                seg: Some(SegParam { start: 61, end: 63 }),
                site_name: Some("A11K".to_string()),
                target_tm: 55.0,
                mut_seq: Some("AAA".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert!(
            v["mutation"].get("warning").is_none(),
            "whole-codon swap must not warn: {v}"
        );
        // The mutation block is 1-based: seg 61..63, codonIndex ==
        // aaPosition1Based (aa numbering conventions are untouched).
        assert_eq!(v["mutation"]["segStart"], 61, "{v}");
        assert_eq!(v["mutation"]["segEnd"], 63, "{v}");
        assert_eq!(v["mutation"]["cds"]["codonIndex"], 11, "{v}");
        assert_eq!(v["mutation"]["cds"]["aaBefore"], "Arg", "{v}");
        assert_eq!(v["mutation"]["cds"]["aaAfter"], "Lys", "{v}");
        assert_eq!(v["mutation"]["cds"]["aaPosition1Based"], 11, "{v}");
        assert_eq!(v["mutation"]["cds"]["aaPositionExcludingMet"], 10, "{v}");

        // Same 3-base full replacement outside any CDS: warning kept.
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "mut_test".to_string(),
                mode: "mutagenesis".to_string(),
                seg: Some(SegParam { start: 11, end: 13 }),
                site_name: Some("M1".to_string()),
                target_tm: 55.0,
                mut_seq: Some("CCC".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        let w = v["mutation"]["warning"]
            .as_str()
            .expect("non-CDS full replacement must warn");
        assert!(w.contains("PLUS-strand"), "{w}");
    }

    #[tokio::test]
    async fn design_primers_mutagenesis_orientation_hint_minus_strand() {
        // Minus-strand CDS: mut_seq is PLUS-strand content, so the coding
        // effect is its reverse complement; the orientationHint must state
        // the CDS strand and the actual codon/amino-acid outcome.
        let mut bytes = vec![b'A'; 90];
        // Plus-strand CGC at 60..62 (0-based) -> coding (minus) GCG = Ala.
        bytes[60] = b'C';
        bytes[61] = b'G';
        bytes[62] = b'C';
        let project = ProjectData {
            name: "mut_hint".to_string(),
            sequence: String::from_utf8(bytes).unwrap(),
            length: 90,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("cds1", "orf", 30, 89, "-")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;

        // Coding GCG->AAG (Ala->Lys) on a minus-strand CDS is plus-strand
        // mut_seq = rev-comp(AAG) = CTT.
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "mut_hint".to_string(),
                mode: "mutagenesis".to_string(),
                seg: Some(SegParam { start: 61, end: 63 }),
                site_name: Some("A11K".to_string()),
                target_tm: 55.0,
                mut_seq: Some("CTT".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["mutation"]["cds"]["aaAfter"], "Lys", "{v}");
        let hint = v["mutation"]["orientationHint"]
            .as_str()
            .expect("orientationHint present");
        assert!(hint.contains("MINUS strand"), "{hint}");
        assert!(hint.contains("AAG") && hint.contains("Lys"), "{hint}");
        assert!(hint.contains("reverse-complement"), "{hint}");

        // Plus-strand CDS: the hint confirms the direct read-out instead.
        let project = ProjectData {
            name: "mut_hint2".to_string(),
            sequence: {
                let mut b = vec![b'A'; 90];
                b[60] = b'C';
                b[61] = b'G';
                b[62] = b'C';
                String::from_utf8(b).unwrap()
            },
            length: 90,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("cds1", "orf", 30, 89, "+")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "mut_hint2".to_string(),
                mode: "mutagenesis".to_string(),
                seg: Some(SegParam { start: 61, end: 63 }),
                site_name: Some("A11K".to_string()),
                target_tm: 55.0,
                mut_seq: Some("AAA".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let hint = out.0["mutation"]["orientationHint"]
            .as_str()
            .expect("orientationHint present");
        assert!(hint.contains("PLUS strand"), "{hint}");
        assert!(hint.contains("AAA") && hint.contains("Lys"), "{hint}");
    }

    #[tokio::test]
    async fn design_primers_unified_tm_matches_check_primer_binding() {
        // Construct a template where the fwd enzyme tail's 3' side accidentally
        // pairs with the template upstream of the anneal core. The unified
        // annealLen/Tm must match a separate check_primer_binding call.
        let mut seq = synthetic_dna(120, 42);
        // BamHI site (GGATCC) is the 3'-most 6 bases of the default fwd tail
        // GCG + GGATCC. Place it immediately 5' of the fwd anneal core.
        seq.replace_range(30..36, "GGATCC");
        let project = ProjectData {
            name: "tail_test".to_string(),
            sequence: seq.clone(),
            length: 120,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "tail_test".to_string(),
                mode: "amplify".to_string(),
                seg: Some(SegParam { start: 37, end: 77 }),
                name: Some("Amp".to_string()),
                target_tm: 55.0,
                fwd_enzyme: Some("BamHI".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert!(v.get("tmBasis").is_some(), "{v}");
        let fwd_group = v["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["type"] == "fwd")
            .cloned()
            .expect("fwd group");
        let default_idx = fwd_group["recommendedIndex"].as_u64().map(|n| n as usize).unwrap_or(0);
        let cand = &fwd_group["candidates"][default_idx];
        let primer_seq = cand["seq"].as_str().unwrap().to_string();
        let designed_len = cand["designedAnnealLength"].as_u64().unwrap() as usize;
        let unified_len = cand["annealLength"].as_u64().unwrap() as usize;
        assert!(
            unified_len > designed_len,
            "tail should extend anneal_len: designed={designed_len}, unified={unified_len}"
        );

        // Verify the same values come out of check_primer_binding.
        let chk = server
            .check_primer_binding(Parameters(CheckPrimerBindingRequest {
                project_id: "tail_test".to_string(),
                primers: vec![PrimerInput {
                    name: "cand".to_string(),
                    r#type: "fwd".to_string(),
                    seq: primer_seq,
                }],
            }))
            .await
            .unwrap();
        let site = &chk.0["results"][0]["site"];
        assert_eq!(
            site["annealLength"].as_u64().unwrap() as usize,
            unified_len,
            "annealLength mismatch"
        );
        assert!(
            (site["tm"].as_f64().unwrap() - cand["tm"].as_f64().unwrap()).abs() < 0.05,
            "tm mismatch: check={} design={}",
            site["tm"],
            cand["tm"]
        );
    }

    #[tokio::test]
    async fn design_primers_unified_tm_matches_check_primer_binding_rev() {
        // Rev enzyme tail whose 3' side accidentally pairs with the template
        // downstream of the rev anneal core. The unified Tm must match what
        // check_primer_binding reports (the engine reverses the matched bases
        // for rev primers before computing Tm).
        let mut seq = synthetic_dna(120, 42);
        // HindIII tail = protect GCG + AAGCTT. Place AAGCTT immediately 3' of
        // the rev anneal core (which ends at seg.end = 77 0-based) so the
        // tail's 3'-most 6 bases pair.
        seq.replace_range(77..83, "AAGCTT");
        let project = ProjectData {
            name: "tail_rev_test".to_string(),
            sequence: seq.clone(),
            length: 120,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "tail_rev_test".to_string(),
                mode: "amplify".to_string(),
                seg: Some(SegParam { start: 37, end: 77 }),
                name: Some("Amp".to_string()),
                target_tm: 55.0,
                rev_enzyme: Some("HindIII".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        let rev_group = v["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|g| g["type"] == "rev")
            .cloned()
            .expect("rev group");
        let default_idx = rev_group["recommendedIndex"].as_u64().map(|n| n as usize).unwrap_or(0);
        let cand = &rev_group["candidates"][default_idx];
        let primer_seq = cand["seq"].as_str().unwrap().to_string();
        let designed_len = cand["designedAnnealLength"].as_u64().unwrap() as usize;
        let unified_len = cand["annealLength"].as_u64().unwrap() as usize;
        assert!(
            unified_len > designed_len,
            "rev tail should extend anneal_len: designed={designed_len}, unified={unified_len}"
        );

        let chk = server
            .check_primer_binding(Parameters(CheckPrimerBindingRequest {
                project_id: "tail_rev_test".to_string(),
                primers: vec![PrimerInput {
                    name: "cand".to_string(),
                    r#type: "rev".to_string(),
                    seq: primer_seq,
                }],
            }))
            .await
            .unwrap();
        let site = &chk.0["results"][0]["site"];
        assert_eq!(
            site["annealLength"].as_u64().unwrap() as usize,
            unified_len,
            "annealLength mismatch"
        );
        assert!(
            (site["tm"].as_f64().unwrap() - cand["tm"].as_f64().unwrap()).abs() < 0.05,
            "tm mismatch: check={} design={}",
            site["tm"],
            cand["tm"]
        );
    }

    #[tokio::test]
    async fn design_primers_validates_segment_bounds() {
        let server = handler_with_project(dna_test_project()).await;
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "feat".to_string(),
                mode: "amplify".to_string(),
                seg: Some(SegParam { start: 90, end: 120 }),
                target_tm: 55.0,
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("out of bounds"),
            "{}",
            out.0
        );
        // start > end on a LINEAR template is rejected.
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "feat".to_string(),
                mode: "amplify".to_string(),
                seg: Some(SegParam { start: 90, end: 10 }),
                target_tm: 55.0,
                ..Default::default()
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("wraps the origin"),
            "{}",
            out.0
        );
        // start > end on a CIRCULAR template wraps and designs fine.
        let mut p = dna_test_project();
        p.topology = "circular".to_string();
        let server = handler_with_project(p).await;
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "feat".to_string(),
                mode: "amplify".to_string(),
                seg: Some(SegParam { start: 90, end: 10 }),
                target_tm: 55.0,
                ..Default::default()
            }))
            .await
            .unwrap();
        assert!(
            out.0["groups"].as_array().is_some_and(|g| g.len() == 2),
            "{}",
            out.0
        );
    }

    #[tokio::test]
    async fn primer_tools_validate_seq_and_type() {
        let server = handler_with_project(dna_test_project()).await;
        let out = server
            .add_primer(Parameters(AddPrimerRequest {
                project_id: "feat".to_string(),
                name: "p1".to_string(),
                r#type: "fwd".to_string(),
                seq: "123 ---".to_string(),
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("empty"),
            "{}",
            out.0
        );
        let out = server
            .add_primer(Parameters(AddPrimerRequest {
                project_id: "feat".to_string(),
                name: "p1".to_string(),
                r#type: "sideways".to_string(),
                seq: "ACGTACGT".to_string(),
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        assert!(
            out.0["message"].as_str().unwrap().contains("fwd"),
            "{}",
            out.0
        );
        // check_primer_binding shares the validation.
        let out = server
            .check_primer_binding(Parameters(CheckPrimerBindingRequest {
                project_id: "feat".to_string(),
                primers: vec![PrimerInput {
                    name: "x".to_string(),
                    r#type: "bad".to_string(),
                    seq: "ACGTACGT".to_string(),
                }],
            }))
            .await
            .unwrap();
        assert_eq!(out.0["ok"], false, "{}", out.0);
        // Rejected primers were not persisted.
        let pm = server.pm.read().await;
        assert!(pm.get_project_by_id("feat").unwrap().primers.is_empty());
    }

    #[tokio::test]
    async fn design_primers_mutagenesis_reports_plus_strand_edit_and_length_hint() {
        let mut bytes = vec![b'A'; 90];
        bytes[60] = b'C';
        bytes[61] = b'G';
        bytes[62] = b'C';
        let project = ProjectData {
            name: "mut_echo".to_string(),
            sequence: String::from_utf8(bytes).unwrap(),
            length: 90,
            topology: "linear".to_string(),
            molecule_type: "dna".to_string(),
            features: vec![feature("cds1", "orf", 30, 89, "+")],
            ..Default::default()
        };
        let server = handler_with_project(project).await;
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "mut_echo".to_string(),
                mode: "mutagenesis".to_string(),
                seg: Some(SegParam { start: 61, end: 63 }),
                target_tm: 55.0,
                mut_seq: Some("AAA".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], true, "{v}");
        let edit = v["plusStrandEdit"].as_str().expect("plusStrandEdit");
        assert!(edit.contains("CGC") && edit.contains("AAA"), "{edit}");
        let notes = v["notes"].as_array().expect("notes array");
        assert!(
            notes
                .iter()
                .filter_map(|n| n.as_str())
                .any(|n| n.contains("seg length + 2")),
            "{v}"
        );

        // A mutSeq length mismatch states both expected and got explicitly.
        let out = server
            .design_primers(Parameters(DesignPrimersRequest {
                project_id: "mut_echo".to_string(),
                mode: "mutagenesis".to_string(),
                seg: Some(SegParam { start: 61, end: 63 }),
                target_tm: 55.0,
                mut_seq: Some("AAAA".to_string()),
                ..Default::default()
            }))
            .await
            .unwrap();
        let v = out.0;
        assert_eq!(v["ok"], false, "{v}");
        let msg = v["message"].as_str().unwrap();
        assert!(msg.contains("expected 3") && msg.contains("got 4"), "{msg}");
    }
