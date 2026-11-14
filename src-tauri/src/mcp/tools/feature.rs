use rmcp::{handler::server::wrapper::Json, ErrorData};
use tauri::Runtime;

use libregene_core::models::{Feature, ProjectData, Segment};

use crate::mcp::next_id;
use crate::mcp::support::{
    fail_envelope, feature_json_1based, insert_seq_hashes, ok_envelope, unit_for,
};
use crate::mcp::types::{
    FeatureAction, FeatureOperation, FeatureSegmentSpec, UpdateFeaturesRequest,
};
use crate::mcp::LibreGeneMcp;

struct FeatureBatchOutcome {
    results: Vec<serde_json::Value>,
    created: usize,
    updated: usize,
    deleted: usize,
    focus: Option<(i64, i64)>,
}

struct FeatureOperationError {
    index: usize,
    action: &'static str,
    feature_id: Option<String>,
    message: String,
}

fn action_name(action: FeatureAction) -> &'static str {
    match action {
        FeatureAction::Create => "create",
        FeatureAction::Update => "update",
        FeatureAction::Delete => "delete",
    }
}

fn resolve_feature_span(
    start: Option<i64>,
    end: Option<i64>,
    segments: Option<Vec<FeatureSegmentSpec>>,
) -> Result<(Vec<Segment>, i64, i64), String> {
    if segments.is_some() && (start.is_some() || end.is_some()) {
        return Err("segments is mutually exclusive with start/end".to_string());
    }
    match (start, end, segments) {
        (Some(s), Some(e), None) => {
            if s < 1 || e < s {
                return Err(format!(
                    "invalid span {}..{}: need 1 <= start <= end (1-based inclusive)",
                    s, e
                ));
            }
            Ok((
                vec![Segment {
                    start: s - 1,
                    end: e - 1,
                    color: None,
                }],
                s - 1,
                e - 1,
            ))
        }
        (None, None, Some(segs)) => {
            if segs.is_empty() {
                return Err("segments must not be empty".to_string());
            }
            let mut out = Vec::with_capacity(segs.len());
            for seg in &segs {
                if seg.start < 1 || seg.end < seg.start {
                    return Err(format!(
                        "invalid segment {}..{}: need 1 <= start <= end (1-based inclusive)",
                        seg.start, seg.end
                    ));
                }
                out.push(Segment {
                    start: seg.start - 1,
                    end: seg.end - 1,
                    color: None,
                });
            }
            let descents = out.windows(2).filter(|w| w[1].start < w[0].start).count();
            let ordered = descents <= 1
                && (descents == 0 || out.first().unwrap().start > out.last().unwrap().end);
            if !ordered {
                return Err(
                    "segments are not in encoding order (ascending starts; a wrapping feature leads with its tail, e.g. join(8886..9326, 1..219))"
                        .to_string(),
                );
            }
            let s = out.first().unwrap().start;
            let e = out.last().unwrap().end;
            Ok((out, s, e))
        }
        (Some(_), None, None) | (None, Some(_), None) => {
            Err("start and end must be given together".to_string())
        }
        (None, None, None) => Err("give start+end or segments".to_string()),
        _ => Err("invalid span parameters".to_string()),
    }
}

fn validate_span(
    project: &ProjectData,
    segments: &[Segment],
    start: i64,
    end: i64,
) -> Result<(), String> {
    let max_coord = segments
        .iter()
        .map(|s| s.end.max(s.start))
        .max()
        .unwrap_or(0)
        .max(start)
        .max(end);
    if max_coord >= project.length {
        return Err(format!(
            "feature span {}..{} is out of range for project length {} (1-based inclusive)",
            start + 1,
            end + 1,
            project.length
        ));
    }
    Ok(())
}

fn focus_span(f: &Feature) -> (i64, i64) {
    if f.segments.is_empty() {
        (f.start, f.end)
    } else {
        (
            f.segments.iter().map(|s| s.start).min().unwrap_or(f.start),
            f.segments.iter().map(|s| s.end).max().unwrap_or(f.end),
        )
    }
}

fn has_edit_fields(op: &FeatureOperation) -> bool {
    op.name.is_some()
        || op.ftype.is_some()
        || op.color.is_some()
        || op.strand.is_some()
        || op.start.is_some()
        || op.end.is_some()
        || op.segments.is_some()
        || op.notes.is_some()
}

fn apply_create(
    project: &ProjectData,
    features: &mut Vec<Feature>,
    op: FeatureOperation,
) -> Result<Feature, String> {
    if op.feature_id.is_some() {
        return Err("featureId is forbidden for create".to_string());
    }
    let name = match op.name {
        Some(n) if !n.is_empty() => n,
        _ => return Err("name is required and must not be empty for create".to_string()),
    };
    let ftype = match op.ftype {
        Some(t) if !t.is_empty() => t,
        _ => return Err("ftype is required and must not be empty for create".to_string()),
    };
    let (segments, start, end) = resolve_feature_span(op.start, op.end, op.segments)?;
    validate_span(project, &segments, start, end)?;
    let strand = op.strand.unwrap_or_else(|| "+".to_string());
    if !matches!(strand.as_str(), "." | "+" | "-") {
        return Err("Invalid strand: must be ., +, or -".to_string());
    }
    let mut feature_id = next_id("feature");
    while features.iter().any(|f| f.id == feature_id) {
        feature_id = next_id("feature");
    }
    let feature = Feature {
        id: feature_id,
        name,
        start,
        end,
        color: op.color.unwrap_or_else(|| "#60A5FA".to_string()),
        ftype,
        segments,
        strand,
        notes: op.notes.unwrap_or_default(),
        translation: String::new(),
        qualifiers: Vec::new(),
    };
    features.push(feature.clone());
    Ok(feature)
}

fn apply_update(
    project: &ProjectData,
    features: &mut [Feature],
    op: FeatureOperation,
) -> Result<Feature, String> {
    let feature_id = op
        .feature_id
        .clone()
        .ok_or_else(|| "featureId is required for update".to_string())?;
    if !has_edit_fields(&op) {
        return Err(
            "Nothing to update: give at least one of name/ftype/color/strand/start+end/segments/notes"
                .to_string(),
        );
    }
    if op.name.as_ref().is_some_and(|n| n.is_empty()) {
        return Err("name must not be empty".to_string());
    }
    if op.ftype.as_ref().is_some_and(|t| t.is_empty()) {
        return Err("ftype must not be empty".to_string());
    }
    if op
        .strand
        .as_ref()
        .is_some_and(|s| !matches!(s.as_str(), "." | "+" | "-"))
    {
        return Err("Invalid strand: must be ., +, or -".to_string());
    }
    let has_span = op.start.is_some() || op.end.is_some() || op.segments.is_some();
    let new_span = if has_span {
        let span = resolve_feature_span(op.start, op.end, op.segments)?;
        validate_span(project, &span.0, span.1, span.2)?;
        Some(span)
    } else {
        None
    };
    let f = features
        .iter_mut()
        .find(|f| f.id == feature_id)
        .ok_or_else(|| format!("Feature not found: {}", feature_id))?;
    if let Some((segments, start, end)) = new_span {
        f.segments = segments;
        f.start = start;
        f.end = end;
    }
    if let Some(v) = op.name {
        f.name = v;
    }
    if let Some(v) = op.ftype {
        f.ftype = v;
    }
    if let Some(v) = op.color {
        f.color = v.clone();
        for seg in &mut f.segments {
            seg.color = Some(v.clone());
        }
    }
    if let Some(v) = op.strand {
        f.strand = v;
    }
    if let Some(v) = op.notes {
        f.notes = v;
    }
    Ok(f.clone())
}

fn apply_delete(features: &mut Vec<Feature>, op: FeatureOperation) -> Result<Feature, String> {
    let feature_id = op
        .feature_id
        .clone()
        .ok_or_else(|| "featureId is required for delete".to_string())?;
    if has_edit_fields(&op) {
        return Err("delete accepts only action and featureId".to_string());
    }
    let pos = features
        .iter()
        .position(|f| f.id == feature_id)
        .ok_or_else(|| format!("Feature not found: {}", feature_id))?;
    Ok(features.remove(pos))
}

fn apply_feature_batch(
    project: &mut ProjectData,
    operations: Vec<FeatureOperation>,
) -> Result<FeatureBatchOutcome, FeatureOperationError> {
    let mut features = project.features.clone();
    let mut outcome = FeatureBatchOutcome {
        results: Vec::with_capacity(operations.len()),
        created: 0,
        updated: 0,
        deleted: 0,
        focus: None,
    };
    for (index, op) in operations.into_iter().enumerate() {
        let action = action_name(op.action);
        let feature_id = op.feature_id.clone();
        let applied = match op.action {
            FeatureAction::Create => apply_create(project, &mut features, op),
            FeatureAction::Update => apply_update(project, &mut features, op),
            FeatureAction::Delete => apply_delete(&mut features, op),
        }
        .map_err(|message| FeatureOperationError {
            index,
            action,
            feature_id,
            message,
        })?;
        if outcome.focus.is_none() {
            outcome.focus = Some(focus_span(&applied));
        }
        match action {
            "create" => outcome.created += 1,
            "update" => outcome.updated += 1,
            "delete" => outcome.deleted += 1,
            _ => unreachable!(),
        }
        outcome.results.push(serde_json::json!({
            "index": index,
            "action": action,
            "featureId": applied.id,
            "feature": feature_json_1based(&applied),
        }));
    }
    project.features = features;
    libregene_core::translate::refresh_feature_translations(project);
    for result in &mut outcome.results {
        if result["action"] == "delete" {
            continue;
        }
        if let Some(feature) = project
            .features
            .iter()
            .find(|f| Some(f.id.as_str()) == result["featureId"].as_str())
        {
            result["feature"] = feature_json_1based(feature);
        }
    }
    Ok(outcome)
}

impl<R: Runtime> LibreGeneMcp<R> {
    pub(crate) async fn update_features_impl(
        &self,
        request: UpdateFeaturesRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project_light(request.project_id).await?;
        self.require_agent_tab(&id).await?;
        let seq_hashes =
            libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        let unit = unit_for(&project.molecule_type);
        let fail = |msg: String, failed: Option<serde_json::Value>| -> Json<serde_json::Value> {
            let mut v = fail_envelope(&id, msg);
            v["unit"] = serde_json::json!(unit);
            if let Some(failed) = failed {
                v["failedOperation"] = failed;
            }
            insert_seq_hashes(&mut v, &seq_hashes);
            Json(v)
        };
        if request.operations.is_empty() || request.operations.len() > 256 {
            return Ok(fail(
                "operations must contain 1-256 feature operations".to_string(),
                None,
            ));
        }

        let include_text = request.include_text.unwrap_or(false);
        let operations = request.operations;
        let mut outcome = None;
        let mut failed_operation = None;
        let payload = crate::do_update_features(
            &self.app_handle,
            &self.pm,
            &self.agent_tabs,
            None,
            &id,
            |project| match apply_feature_batch(project, operations) {
                Ok(applied) => {
                    outcome = Some(applied);
                    Ok(())
                }
                Err(err) => {
                    failed_operation = Some(serde_json::json!({
                        "index": err.index,
                        "action": err.action,
                        "featureId": err.feature_id,
                    }));
                    Err(err.message)
                }
            },
        )
        .await;
        let payload = match payload {
            Ok(payload) => payload,
            Err(err) => return Ok(fail(err, failed_operation)),
        };
        if let Some(err) = Self::payload_error(&payload) {
            return Ok(fail(err, failed_operation));
        }
        let outcome = outcome.ok_or_else(|| {
            ErrorData::internal_error("feature batch completed without a result", None)
        })?;
        if let Some((s, e)) = outcome.focus {
            crate::payload::emit_mutation_focus(&self.app_handle, &id, s, e, "feature");
        }
        let text = if include_text {
            match outcome.focus {
                Some(span) => self.digest_region(&id, Some(span), true, false).await,
                None => None,
            }
        } else {
            None
        };
        let message = format!(
            "Applied {} feature operation(s) atomically: {} created, {} updated, {} deleted",
            outcome.results.len(),
            outcome.created,
            outcome.updated,
            outcome.deleted
        );
        let mut v = ok_envelope(&id, message, text);
        v["unit"] = serde_json::json!(unit);
        v["operationCount"] = serde_json::json!(outcome.results.len());
        v["createdCount"] = serde_json::json!(outcome.created);
        v["updatedCount"] = serde_json::json!(outcome.updated);
        v["deletedCount"] = serde_json::json!(outcome.deleted);
        v["results"] = serde_json::json!(outcome.results);
        insert_seq_hashes(&mut v, &seq_hashes);
        Ok(Json(v))
    }
}
