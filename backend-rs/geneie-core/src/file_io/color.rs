pub fn normalize_color(raw: &str) -> String {
    let s = raw.trim();
    if s.starts_with('#') && s.len() == 7 {
        s.to_lowercase()
    } else if s.is_empty() {
        String::new()
    } else {
        // Try to interpret as color name —- return empty for unknown
        String::new()
    }
}

pub fn default_color(kind: &str) -> &str {
    match kind.to_lowercase().as_str() {
        "cds" => "#60A5FA",
        "promoter" => "#34d399",
        "terminator" => "#9ca3af",
        "intron" => "#f472b6",
        "rep_origin" => "#fbbf24",
        "misc_feature" => "#c084fc",
        _ => "#c084fc",
    }
}

pub fn adjust_color_readability(color: &str) -> String {
    // If the color is too light, darken it slightly for readability on white.
    if color.is_empty() || !color.starts_with('#') || color.len() != 7 {
        return if color.is_empty() {
            "#c084fc".to_string()
        } else {
            color.to_string()
        };
    }
    // Keep the color but ensure it has enough contrast.
    // Simple heuristic: if all RGB components are > 0xE0, darken.
    let r = u8::from_str_radix(&color[1..3], 16).unwrap_or(0);
    let g = u8::from_str_radix(&color[3..5], 16).unwrap_or(0);
    let b = u8::from_str_radix(&color[5..7], 16).unwrap_or(0);
    if r > 0xE0 && g > 0xE0 && b > 0xE0 {
        format!("#{:02x}{:02x}{:02x}", r / 2, g / 2, b / 2)
    } else {
        color.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_color_hex() {
        assert_eq!(normalize_color("#AABBCC"), "#aabbcc");
    }

    #[test]
    fn test_normalize_color_empty() {
        assert_eq!(normalize_color(""), "");
    }

    #[test]
    fn test_default_color_cds() {
        assert_eq!(default_color("CDS"), "#60A5FA");
    }

    #[test]
    fn test_adjust_light_color() {
        let adjusted = adjust_color_readability("#F0F0F0");
        assert_ne!(adjusted, "#F0F0F0");
    }
}
