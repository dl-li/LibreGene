//! NCBI BLAST Common URL API submission (CMD=Put).
//!
//! One fixed "where does this sequence come from" preset per molecule type:
//! nucleotide → megablast vs core_nt, protein → blastp vs nr. Results are
//! viewed on the official web results page keyed by RID; this module only
//! submits and parses the RID/RTOE reply.

use std::time::Duration;

const BLAST_CGI: &str = "https://blast.ncbi.nlm.nih.gov/Blast.cgi";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlastSubmission {
    pub rid: String,
    pub rtoe_secs: u64,
}

/// Official web results page for a submitted search (auto-refreshes until done).
pub fn results_url(rid: &str) -> String {
    format!("{BLAST_CGI}?CMD=Get&FORMAT_TYPE=HTML&RID={rid}")
}

/// Submit a sequence to NCBI BLAST. `molecule_type` is "dna" | "rna" | "protein";
/// RNA is submitted as DNA (U→T) since BLAST has no RNA query mode.
pub fn submit(sequence: &str, molecule_type: &str) -> Result<BlastSubmission, String> {
    let mut seq: String = sequence
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if seq.is_empty() {
        return Err("empty query sequence".into());
    }
    let protein = molecule_type == "protein";
    if !protein {
        seq = seq.replace('U', "T");
    }
    let fasta = format!(">LibreGene query\n{seq}");

    let mut form: Vec<(&str, &str)> = vec![
        ("CMD", "Put"),
        ("QUERY", &fasta),
        ("TOOL", "LibreGene"),
        ("HITLIST_SIZE", "100"),
    ];
    if protein {
        form.push(("PROGRAM", "blastp"));
        form.push(("DATABASE", "nr"));
    } else {
        form.push(("PROGRAM", "blastn"));
        form.push(("MEGABLAST", "on"));
        form.push(("DATABASE", "core_nt"));
    }

    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(60)))
        .user_agent("LibreGene")
        .build()
        .into();
    let mut resp = agent
        .post(BLAST_CGI)
        .send_form(form)
        .map_err(|e| format!("BLAST submission failed: {e}"))?;
    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("failed to read BLAST reply: {e}"))?;
    parse_put_reply(&body).ok_or_else(|| {
        let snippet: String = body.chars().take(200).collect();
        format!("BLAST returned no RID: {snippet}")
    })
}

/// Parse the QBlastInfo reply block ("RID = …" / "RTOE = …" lines).
fn parse_put_reply(body: &str) -> Option<BlastSubmission> {
    let mut rid = None;
    let mut rtoe_secs = 0;
    for line in body.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("RID = ") {
            rid = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("RTOE = ") {
            rtoe_secs = v.trim().parse().unwrap_or(0);
        }
    }
    rid.map(|rid| BlastSubmission { rid, rtoe_secs })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_put_reply() {
        let body = "<html>\nQBlastInfoBegin\n    RID = ABC123XYZ\n    RTOE = 17\nQBlastInfoEnd\n";
        let sub = parse_put_reply(body).unwrap();
        assert_eq!(sub.rid, "ABC123XYZ");
        assert_eq!(sub.rtoe_secs, 17);
        assert!(results_url(&sub.rid).contains("RID=ABC123XYZ"));
    }

    #[test]
    fn put_reply_without_rid_is_none() {
        assert!(parse_put_reply("<html>Message ID#25 Error</html>").is_none());
    }

    #[test]
    fn rejects_empty_query() {
        assert!(submit("   \n ", "dna").is_err());
    }

    /// Live NCBI round-trip; run explicitly with:
    /// `cargo test -p libregene-core --lib blast_live -- --ignored`
    #[test]
    #[ignore = "hits the real NCBI BLAST endpoint"]
    fn blast_live_submit_returns_rid() {
        let seq = "GATTACAAGCTTAGCTTACGATCGATCGTTAGCTAGCTACGATCGATCGATGC";
        let sub = submit(seq, "dna").unwrap();
        assert!(!sub.rid.is_empty());
    }
}
