use std::fmt::Write as _;

use crate::models::{Feature, ProjectData};

const TWO_PI: f64 = std::f64::consts::PI * 2.0;
const ARC_EPS: f64 = 1e-4;
const HEAD_FLARE: f64 = 4.0;
const BG: &str = "#fdfbf7";

pub struct MapImage {
    pub svg: String,
    pub view: &'static str,
    pub width: f64,
    pub height: f64,
}

fn unit(project: &ProjectData) -> &'static str {
    match project.molecule_type.as_str() {
        "rna" => "nt",
        "protein" => "aa",
        _ => "bp",
    }
}

fn n(value: f64) -> String {
    let mut s = format!("{value:.3}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    if s == "-0" {
        "0".to_string()
    } else {
        s
    }
}

fn esc(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn text_width(text: &str) -> f64 {
    text.chars().count() as f64 * 8.0
}

fn ellipsize(text: &str, max_width: f64) -> String {
    if text_width(text) <= max_width {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    if chars.len() < 5 {
        return "…".to_string();
    }
    let mut lo = 2usize;
    let mut hi = chars.len() - 2;
    let mut best = "…".to_string();
    while lo <= hi {
        let keep = (lo + hi) / 2;
        let head = keep.div_ceil(2);
        let tail = keep - head;
        let candidate = format!(
            "{}…{}",
            chars[..head].iter().collect::<String>(),
            chars[chars.len() - tail..].iter().collect::<String>()
        );
        if text_width(&candidate) <= max_width {
            best = candidate;
            lo = keep + 1;
        } else {
            hi = keep - 1;
        }
    }
    best
}

fn pol(cx: f64, cy: f64, radius: f64, theta: f64) -> (f64, f64) {
    (cx + radius * theta.sin(), cy - radius * theta.cos())
}

fn point(p: (f64, f64)) -> String {
    format!("{} {}", n(p.0), n(p.1))
}

fn arc_arrow_path(r_out: f64, r_in: f64, th0: f64, th1: f64, strand: &str) -> String {
    let r_mid = (r_out + r_in) / 2.0;
    let head = (((r_out - r_in + HEAD_FLARE * 2.0) * 1.1) / r_mid).min((th1 - th0) * 0.6);
    if th1 - th0 < 1e-4 {
        return String::new();
    }
    let large = if th1 - th0 - head > std::f64::consts::PI {
        1
    } else {
        0
    };
    if strand == "-" {
        let x0o = pol(0.0, 0.0, r_out, th0 + head);
        let x1o = pol(0.0, 0.0, r_out, th1);
        let x1i = pol(0.0, 0.0, r_in, th1);
        let x0i = pol(0.0, 0.0, r_in, th0 + head);
        let x0of = pol(0.0, 0.0, r_out + HEAD_FLARE, th0 + head);
        let x0if = pol(0.0, 0.0, r_in - HEAD_FLARE, th0 + head);
        let tip = pol(0.0, 0.0, r_mid, th0);
        format!(
            "M {} A {} {} 0 {large} 1 {} L {} A {} {} 0 {large} 0 {} L {} L {} L {} Z",
            point(x0o),
            n(r_out),
            n(r_out),
            point(x1o),
            point(x1i),
            n(r_in),
            n(r_in),
            point(x0i),
            point(x0if),
            point(tip),
            point(x0of)
        )
    } else {
        let x0o = pol(0.0, 0.0, r_out, th0);
        let x1o = pol(0.0, 0.0, r_out, th1 - head);
        let x1i = pol(0.0, 0.0, r_in, th1 - head);
        let x0i = pol(0.0, 0.0, r_in, th0);
        let x1of = pol(0.0, 0.0, r_out + HEAD_FLARE, th1 - head);
        let x1if = pol(0.0, 0.0, r_in - HEAD_FLARE, th1 - head);
        let tip = pol(0.0, 0.0, r_mid, th1);
        format!(
            "M {} A {} {} 0 {large} 1 {} L {} L {} L {} L {} A {} {} 0 {large} 0 {} Z",
            point(x0o),
            n(r_out),
            n(r_out),
            point(x1o),
            point(x1of),
            point(tip),
            point(x1if),
            point(x1i),
            n(r_in),
            n(r_in),
            point(x0i)
        )
    }
}

fn arc_sector_path(r_out: f64, r_in: f64, th0: f64, th1: f64) -> String {
    if th1 - th0 >= TWO_PI - 1e-6 {
        let a1 = th0 + std::f64::consts::PI + ARC_EPS;
        let x0o = pol(0.0, 0.0, r_out, th0);
        let x1o = pol(0.0, 0.0, r_out, a1);
        let x0i = pol(0.0, 0.0, r_in, th0);
        let x1i = pol(0.0, 0.0, r_in, a1);
        return format!(
            "M {} A {} {} 0 1 1 {} A {} {} 0 0 1 {} Z M {} A {} {} 0 0 0 {} A {} {} 0 1 0 {} Z",
            point(x0o),
            n(r_out),
            n(r_out),
            point(x1o),
            n(r_out),
            n(r_out),
            point(x0o),
            point(x0i),
            n(r_in),
            n(r_in),
            point(x1i),
            n(r_in),
            n(r_in),
            point(x0i)
        );
    }
    let x0o = pol(0.0, 0.0, r_out, th0);
    let x1o = pol(0.0, 0.0, r_out, th1);
    let x1i = pol(0.0, 0.0, r_in, th1);
    let x0i = pol(0.0, 0.0, r_in, th0);
    let large = if th1 - th0 > std::f64::consts::PI {
        1
    } else {
        0
    };
    format!(
        "M {} A {} {} 0 {large} 1 {} L {} A {} {} 0 {large} 0 {} Z",
        point(x0o),
        n(r_out),
        n(r_out),
        point(x1o),
        point(x1i),
        n(r_in),
        n(r_in),
        point(x0i)
    )
}

fn feature_segments(feature: &Feature) -> Vec<(i64, i64)> {
    if feature.segments.is_empty() {
        vec![(feature.start, feature.end)]
    } else {
        feature.segments.iter().map(|s| (s.start, s.end)).collect()
    }
}

fn unwrap_runs(feature: &Feature, length: i64) -> Vec<(i64, i64)> {
    let mut runs: Vec<(i64, i64)> = Vec::new();
    let mut offset = 0;
    let mut previous_start = i64::MIN;
    for (start, end) in feature_segments(feature) {
        let mut a = start + offset;
        let mut b = end + offset;
        if b < a {
            b += length;
        }
        if a < previous_start {
            offset += length;
            a += length;
            b += length;
        }
        previous_start = a;
        if let Some(last) = runs.last_mut().filter(|last| a <= last.1 + 1) {
            last.1 = last.1.max(b);
        } else {
            runs.push((a, b));
        }
    }
    runs
}

fn normalized_segments(feature: &Feature, length: i64) -> Vec<(i64, i64)> {
    let mut out = Vec::new();
    for (start, end) in feature_segments(feature) {
        if end >= start {
            out.push((start, end));
        } else {
            out.push((start, length - 1));
            out.push((0, end));
        }
    }
    out.sort_by_key(|segment| segment.0);
    out
}

fn feature_length(feature: &Feature, length: i64) -> i64 {
    normalized_segments(feature, length)
        .into_iter()
        .map(|(start, end)| end - start + 1)
        .sum()
}

fn tip_buried(feature: &Feature, features: &[Feature], length: i64) -> bool {
    let segments = feature_segments(feature);
    let tip = if feature.strand == "-" {
        segments[0].0
    } else {
        segments[segments.len() - 1].1
    };
    let own_length = feature_length(feature, length);
    features.iter().any(|other| {
        other.id != feature.id
            && feature_length(other, length) > own_length
            && normalized_segments(other, length)
                .into_iter()
                .any(|(start, end)| tip >= start && tip <= end)
    })
}

struct CircularLabel<'a> {
    feature: &'a Feature,
    theta: f64,
    width: f64,
    x: f64,
    y: f64,
    side: i8,
}

fn circular_labels(features: &[Feature], length: i64, radius: f64) -> Vec<CircularLabel<'_>> {
    let label_radius = radius + 40.0;
    let mut labels = Vec::new();
    for sign in [1i8, -1i8] {
        let mut side: Vec<CircularLabel> = features
            .iter()
            .map(|feature| {
                let runs = unwrap_runs(feature, length);
                let midpoint = ((runs[0].0 + runs[runs.len() - 1].1 + 1) as f64 / 2.0)
                    .rem_euclid(length as f64);
                let theta = midpoint / length as f64 * TWO_PI;
                CircularLabel {
                    feature,
                    theta,
                    width: text_width(&feature.name),
                    x: 0.0,
                    y: -label_radius * theta.cos(),
                    side: sign,
                }
            })
            .filter(|label| {
                let label_side = if label.theta.sin() >= 0.0 { 1 } else { -1 };
                label_side == sign
            })
            .collect();
        side.sort_by(|a, b| {
            (-a.theta.cos())
                .partial_cmp(&(-b.theta.cos()))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for i in 1..side.len() {
            side[i].y = side[i].y.max(side[i - 1].y + 17.0);
        }
        if let (Some(first), Some(last)) = (side.first(), side.last()) {
            let over_bottom = last.y - label_radius;
            if over_bottom > 0.0 {
                let shift = -over_bottom.min((first.y + label_radius).max(0.0));
                for label in &mut side {
                    label.y += shift;
                }
            }
        }
        if let (Some(first), Some(last)) = (side.first(), side.last()) {
            let over_top = -label_radius - first.y;
            if over_top > 0.0 {
                let shift = over_top.min((label_radius - last.y).max(0.0));
                for label in &mut side {
                    label.y += shift;
                }
            }
        }
        for label in &mut side {
            let ay = label.y.abs();
            label.x = if ay <= label_radius * 0.8 {
                sign as f64 * (label_radius * label_radius - ay * ay).sqrt()
            } else {
                sign as f64 * label_radius * 0.85
            };
        }
        labels.extend(side);
    }
    labels
}

fn render_circular(project: &ProjectData, include_labels: bool) -> MapImage {
    let length = project.length;
    let radius = 150.0;
    let half = 9.0;
    let label_radius = radius + 40.0;
    let labels = if include_labels {
        circular_labels(&project.features, length, radius)
    } else {
        Vec::new()
    };
    let mut min_x = -(radius + half);
    let mut max_x = radius + half;
    let mut min_y = -(radius + half);
    let mut max_y = radius + half;
    for label in &labels {
        let tx = label.x
            + if label.side > 0 {
                6.0
            } else {
                -6.0 - label.width
            };
        min_x = min_x.min(tx);
        max_x = max_x.max(tx + label.width);
        min_y = min_y.min(label.y - 9.0);
        max_y = max_y.max(label.y + 9.0);
    }
    let x = min_x - 14.0;
    let y = min_y - 14.0;
    let width = max_x - min_x + 28.0;
    let height = max_y - min_y + 28.0;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{} {} {} {}\" width=\"{}\" height=\"{}\" role=\"img\" aria-label=\"{}\">",
        n(x), n(y), n(width), n(height), n(width), n(height), esc(&project.name)
    );
    let _ = write!(
        svg,
        "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
        n(x),
        n(y),
        n(width),
        n(height),
        BG
    );
    let _ = write!(svg, "<circle cx=\"0\" cy=\"0\" r=\"150\" fill=\"none\" stroke=\"#8a8577\" stroke-width=\"1.5\"/>");
    for feature in &project.features {
        let runs = unwrap_runs(feature, length);
        let directed = matches!(feature.strand.as_str(), "+" | "-");
        let buried = directed && tip_buried(feature, &project.features, length);
        for (index, (start, end)) in runs.iter().enumerate() {
            let th0 = *start as f64 / length as f64 * TWO_PI;
            let th1 = (*end + 1) as f64 / length as f64 * TWO_PI;
            let tip = directed
                && !buried
                && (feature.strand == "-" && index == 0
                    || feature.strand != "-" && index + 1 == runs.len());
            let path = if tip {
                arc_arrow_path(radius + half, radius - half, th0, th1, &feature.strand)
            } else {
                arc_sector_path(radius + half, radius - half, th0, th1)
            };
            let color = if feature.color.is_empty() {
                "#9e9e9e"
            } else {
                &feature.color
            };
            let _ = write!(svg, "<path d=\"{}\" fill=\"{}\" fill-opacity=\"0.85\" stroke=\"#333\" stroke-width=\"1.2\"/>", path, esc(color));
        }
    }
    for label in &labels {
        let start = pol(0.0, 0.0, radius + half + 2.0, label.theta);
        let elbow = pol(0.0, 0.0, label_radius - 12.0, label.theta);
        let end_x = label.x + label.side as f64 * 3.0;
        let _ = write!(svg, "<polyline points=\"{},{} {},{} {},{}\" fill=\"none\" stroke=\"#bbb\" stroke-width=\"0.7\"/>", n(start.0), n(start.1), n(elbow.0), n(elbow.1), n(end_x), n(label.y));
    }
    for label in &labels {
        let x = label.x + label.side as f64 * 6.0;
        let anchor = if label.side > 0 { "start" } else { "end" };
        let _ = write!(svg, "<text x=\"{}\" y=\"{}\" text-anchor=\"{}\" dominant-baseline=\"middle\" font-size=\"12\" font-weight=\"600\" font-family=\"TeX Gyre Heros\" fill=\"#333\" stroke=\"{}\" stroke-width=\"3\" stroke-linejoin=\"round\" paint-order=\"stroke\">{}</text>", n(x), n(label.y), anchor, BG, esc(&label.feature.name));
    }
    let title = ellipsize(&project.name, 2.0 * (radius - half) - 50.0);
    let _ = write!(svg, "<text x=\"0\" y=\"-11\" text-anchor=\"middle\" dominant-baseline=\"central\" font-size=\"17\" font-weight=\"bold\" font-family=\"TeX Gyre Heros\" fill=\"#222\">{}</text>", esc(&title));
    let _ = write!(svg, "<text x=\"0\" y=\"11\" text-anchor=\"middle\" dominant-baseline=\"central\" font-size=\"12\" font-family=\"TeX Gyre Heros\" fill=\"#555\">{} {}</text></svg>", length, unit(project));
    MapImage {
        svg,
        view: "circular",
        width,
        height,
    }
}

struct LinearLabel<'a> {
    feature: &'a Feature,
    level: usize,
    center: f64,
}

fn linear_piece_points(start: f64, end: f64, line_y: f64, tip: bool, strand: &str) -> String {
    let head = ((end - start) / 2.0).clamp(2.0, 10.0);
    let h = 8.0;
    let flare = h + HEAD_FLARE;
    if !tip {
        return format!(
            "{},{} {},{} {},{} {},{}",
            n(start),
            n(line_y - h),
            n(end),
            n(line_y - h),
            n(end),
            n(line_y + h),
            n(start),
            n(line_y + h)
        );
    }
    if strand == "-" {
        format!(
            "{},{} {},{} {},{} {},{} {},{} {},{} {},{}",
            n(end),
            n(line_y - h),
            n(start + head),
            n(line_y - h),
            n(start + head),
            n(line_y - flare),
            n(start),
            n(line_y),
            n(start + head),
            n(line_y + flare),
            n(start + head),
            n(line_y + h),
            n(end),
            n(line_y + h)
        )
    } else {
        format!(
            "{},{} {},{} {},{} {},{} {},{} {},{} {},{}",
            n(start),
            n(line_y - h),
            n(end - head),
            n(line_y - h),
            n(end - head),
            n(line_y - flare),
            n(end),
            n(line_y),
            n(end - head),
            n(line_y + flare),
            n(end - head),
            n(line_y + h),
            n(start),
            n(line_y + h)
        )
    }
}

fn render_linear(project: &ProjectData, include_labels: bool) -> MapImage {
    let length = project.length;
    let x0 = 24.0;
    let x1 = 616.0;
    let px = |position: f64| x0 + position / length as f64 * (x1 - x0);
    let mut labels: Vec<LinearLabel> = Vec::new();
    if include_labels {
        let mut positioned: Vec<(&Feature, f64, f64)> = project
            .features
            .iter()
            .map(|feature| {
                let runs = unwrap_runs(feature, length);
                let midpoint = ((runs[0].0 + runs[runs.len() - 1].1 + 1) as f64 / 2.0)
                    .rem_euclid(length as f64);
                (feature, midpoint, text_width(&feature.name))
            })
            .collect();
        positioned.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut level_end: Vec<f64> = Vec::new();
        for (feature, midpoint, width) in positioned {
            let center = (width / 2.0 + 4.0).max((640.0 - width / 2.0 - 4.0).min(px(midpoint)));
            let mut level = 0;
            while level_end
                .get(level)
                .is_some_and(|end| center - width / 2.0 < *end + 6.0)
            {
                level += 1;
            }
            if level == level_end.len() {
                level_end.push(center + width / 2.0);
            } else {
                level_end[level] = center + width / 2.0;
            }
            labels.push(LinearLabel {
                feature,
                level,
                center,
            });
        }
    }
    let max_level = labels.iter().map(|label| label.level).max().unwrap_or(0);
    let line_y = if include_labels {
        74.0 + max_level as f64 * 16.0
    } else {
        26.0
    };
    let height = line_y + if project.name.is_empty() { 34.0 } else { 52.0 };
    let mut svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 640 {}\" width=\"640\" height=\"{}\" role=\"img\" aria-label=\"{}\">", n(height), n(height), esc(&project.name));
    let _ = write!(
        svg,
        "<rect x=\"0\" y=\"0\" width=\"640\" height=\"{}\" fill=\"{}\"/>",
        n(height),
        BG
    );
    let _ = write!(
        svg,
        "<line x1=\"24\" y1=\"{}\" x2=\"616\" y2=\"{}\" stroke=\"#8a8577\" stroke-width=\"1.5\"/>",
        n(line_y),
        n(line_y)
    );
    for feature in &project.features {
        let directed = matches!(feature.strand.as_str(), "+" | "-");
        let buried = directed && tip_buried(feature, &project.features, length);
        let runs = unwrap_runs(feature, length);
        let wraps = runs.iter().any(|run| run.1 >= length);
        let mut pieces = Vec::new();
        if wraps {
            for (start, end) in runs {
                pieces.push((start, end.min(length - 1)));
                if end >= length {
                    pieces.push((0, end - length));
                }
            }
        } else {
            pieces.push((feature.start, feature.end));
        }
        for (index, (start, end)) in pieces.iter().enumerate() {
            let tip = directed
                && !buried
                && (feature.strand == "-" && index == 0
                    || feature.strand != "-" && index + 1 == pieces.len());
            let points = linear_piece_points(
                px(*start as f64),
                px((*end + 1) as f64),
                line_y,
                tip,
                &feature.strand,
            );
            let color = if feature.color.is_empty() {
                "#9e9e9e"
            } else {
                &feature.color
            };
            let _ = write!(svg, "<polygon points=\"{}\" fill=\"{}\" fill-opacity=\"0.85\" stroke=\"#333\" stroke-width=\"1.2\"/>", points, esc(color));
        }
    }
    for label in &labels {
        let y2 = line_y - 26.0 - label.level as f64 * 16.0;
        let _ = write!(
            svg,
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#aaa\" stroke-width=\"0.7\"/>",
            n(label.center),
            n(line_y - 10.0),
            n(label.center),
            n(y2)
        );
    }
    for label in &labels {
        let y = line_y - 30.0 - label.level as f64 * 16.0;
        let _ = write!(svg, "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"11\" font-family=\"TeX Gyre Heros\" fill=\"#333\" stroke=\"{}\" stroke-width=\"3\" stroke-linejoin=\"round\" paint-order=\"stroke\">{}</text>", n(label.center), n(y), BG, esc(&label.feature.name));
    }
    if !project.name.is_empty() {
        let title = ellipsize(
            &format!("{} · {} {}", project.name, length, unit(project)),
            572.0,
        );
        let _ = write!(svg, "<text x=\"320\" y=\"{}\" text-anchor=\"middle\" dominant-baseline=\"central\" font-size=\"13\" font-weight=\"bold\" font-family=\"TeX Gyre Heros\" fill=\"#222\">{}</text>", n(line_y + 32.0), esc(&title));
    }
    svg.push_str("</svg>");
    MapImage {
        svg,
        view: "linear",
        width: 640.0,
        height,
    }
}

pub fn render_map(
    project: &ProjectData,
    view: Option<&str>,
    include_labels: bool,
) -> Result<MapImage, String> {
    if project.length <= 0 || project.sequence.is_empty() {
        return Err("Cannot render an empty sequence".to_string());
    }
    for feature in &project.features {
        let valid = |start: i64, end: i64| {
            start >= 0 && end >= 0 && start < project.length && end < project.length
        };
        if !valid(feature.start, feature.end)
            || feature_segments(feature)
                .into_iter()
                .any(|(start, end)| !valid(start, end))
        {
            return Err(format!(
                "Feature '{}' has coordinates outside the sequence (length {})",
                feature.name, project.length
            ));
        }
    }
    let view = view.unwrap_or(&project.topology);
    match view {
        "circular" => Ok(render_circular(project, include_labels)),
        "linear" => Ok(render_linear(project, include_labels)),
        _ => Err("view must be \"circular\" or \"linear\"".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(topology: &str) -> ProjectData {
        ProjectData {
            name: "Demo & <vector>".to_string(),
            sequence: "ACGT".repeat(25),
            length: 100,
            topology: topology.to_string(),
            molecule_type: "dna".to_string(),
            features: vec![
                Feature {
                    id: "f1".to_string(),
                    name: "AmpR & <bla>".to_string(),
                    start: 90,
                    end: 9,
                    color: "#60A5FA".to_string(),
                    ftype: "CDS".to_string(),
                    segments: vec![
                        crate::models::Segment {
                            start: 90,
                            end: 99,
                            color: None,
                        },
                        crate::models::Segment {
                            start: 0,
                            end: 9,
                            color: None,
                        },
                    ],
                    strand: "+".to_string(),
                    notes: String::new(),
                    translation: String::new(),
                    qualifiers: Vec::new(),
                },
                Feature {
                    id: "f2".to_string(),
                    name: "reverse".to_string(),
                    start: 30,
                    end: 50,
                    color: "#F87171".to_string(),
                    ftype: "CDS".to_string(),
                    segments: Vec::new(),
                    strand: "-".to_string(),
                    notes: String::new(),
                    translation: String::new(),
                    qualifiers: Vec::new(),
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn circular_svg_renders_wrapping_features_and_escapes_text() {
        let image = render_map(&project("circular"), None, true).unwrap();
        assert_eq!(image.view, "circular");
        assert!(image.svg.starts_with("<svg xmlns="));
        assert!(image.svg.ends_with("</svg>"));
        assert!(image.svg.contains("AmpR &amp; &lt;bla&gt;"));
        assert!(image.svg.contains("Demo &amp; &lt;vector&gt;"));
        assert!(image.svg.contains("100 bp"));
        assert!(!image.svg.contains("NaN"));
        assert!(image.svg.contains("<polyline"));
    }

    #[test]
    fn linear_svg_can_hide_labels_and_override_topology() {
        let image = render_map(&project("circular"), Some("linear"), false).unwrap();
        assert_eq!(image.view, "linear");
        assert_eq!(image.width, 640.0);
        assert!(!image.svg.contains("AmpR &amp;"));
        assert!(image.svg.contains("Demo &amp; &lt;vector&gt; · 100 bp"));
        assert!(image.svg.contains("<polygon"));
    }

    #[test]
    fn render_rejects_empty_sequences_and_unknown_views() {
        let empty = ProjectData::default();
        assert!(render_map(&empty, None, true).is_err());
        assert!(render_map(&project("linear"), Some("map"), true).is_err());
    }

    #[test]
    fn protein_length_uses_amino_acid_units() {
        let mut protein = project("linear");
        protein.molecule_type = "protein".to_string();
        protein.sequence = "M".repeat(100);
        let image = render_map(&protein, None, true).unwrap();
        assert!(image.svg.contains("100 aa"));
    }
}
