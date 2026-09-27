//! blastn parameters (NCBI blast_options.h defaults). Ported from GenePad
//! (https://genepad.cn, GenePad team).

#[derive(Clone, Debug)]
pub struct BlastnOptions {
    /// Word size (BLAST_WORDSIZE_NUCL=11; megablast=28).
    pub word_size: usize,
    /// Match reward (BLAST_REWARD=1).
    pub reward: i32,
    /// Mismatch penalty (BLAST_PENALTY=-3).
    pub penalty: i32,
    /// Gap open penalty (BLAST_GAP_OPEN_NUCL=5; megablast=0 → greedy).
    pub gap_open: i32,
    /// Gap extend penalty (BLAST_GAP_EXTN_NUCL=2; megablast=0).
    pub gap_extend: i32,
    /// Ungapped X-drop (BLAST_UNGAPPED_X_DROPOFF_NUCL=20).
    pub x_drop_ungapped: i32,
    /// Gapped X-drop in bits, converted to raw score via λ
    /// (BLAST_GAP_X_DROPOFF_NUCL=30).
    pub x_drop_gapped_bits: f64,
    /// Bit threshold that triggers gapped extension
    /// (BLAST_GAP_TRIGGER_NUCL=27.0).
    pub gap_trigger_bits: f64,
    /// E-value threshold (BLAST_EXPECT_VALUE=10.0).
    pub expect_value: f64,
    /// true = greedy gapped (gap_open=0); false = X-drop DP.
    pub megablast: bool,
    /// true = DUST soft-mask the query.
    pub dust: bool,
    /// Hard cap on kept HSPs (BLAST_HITLIST_SIZE concept).
    pub max_hsps: usize,
}

impl BlastnOptions {
    /// Traditional blastn defaults (word=11, reward=1, penalty=-3, gap=5/2,
    /// X-drop DP).
    pub fn blastn_default() -> Self {
        Self {
            word_size: 11,
            reward: 1,
            penalty: -3,
            gap_open: 5,
            gap_extend: 2,
            x_drop_ungapped: 20,
            x_drop_gapped_bits: 30.0,
            gap_trigger_bits: 27.0,
            expect_value: 10.0,
            megablast: false,
            dust: true,
            max_hsps: 500,
        }
    }

    /// megablast defaults (word=28, gap=0/0, greedy).
    pub fn megablast_default() -> Self {
        Self {
            word_size: 28,
            reward: 1,
            penalty: -3,
            gap_open: 0,
            gap_extend: 0,
            x_drop_ungapped: 20,
            x_drop_gapped_bits: 25.0, // BLAST_GAP_X_DROPOFF_GREEDY=25
            gap_trigger_bits: 27.0,
            expect_value: 10.0,
            megablast: true,
            dust: true,
            max_hsps: 500,
        }
    }
}

impl Default for BlastnOptions {
    fn default() -> Self {
        Self::blastn_default()
    }
}
