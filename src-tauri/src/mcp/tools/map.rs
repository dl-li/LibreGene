use rmcp::{handler::server::wrapper::Json, ErrorData};
use tauri::Runtime;

use crate::mcp::support::{fail_envelope, insert_seq_hashes, ok_envelope, unit_for};
use crate::mcp::types::RenderMapRequest;
use crate::mcp::LibreGeneMcp;

impl<R: Runtime> LibreGeneMcp<R> {
    pub(crate) async fn render_map_impl(
        &self,
        request: RenderMapRequest,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let (id, project) = self.resolve_project_light(request.project_id).await?;
        let hashes =
            libregene_core::utils::orientation_hashes(&project.sequence, &project.molecule_type);
        let unit = unit_for(&project.molecule_type);
        let fail = |message: String| {
            let mut value = fail_envelope(&id, message);
            value["unit"] = serde_json::json!(unit);
            insert_seq_hashes(&mut value, &hashes);
            Json(value)
        };
        let output_path = match request.output_path {
            Some(path) => {
                crate::validate_user_path(&path, &["svg"]).map_err(|e| {
                    ErrorData::invalid_params(format!("invalid outputPath: {}", e), None)
                })?;
                let path = crate::normalize_project_path(&path)
                    .map_err(|e| ErrorData::invalid_params(e, None))?;
                if std::path::Path::new(&path).exists() && !request.overwrite.unwrap_or(false) {
                    return Ok(fail(format!(
                        "{} already exists — pass overwrite: true to replace it, or choose a different path",
                        path
                    )));
                }
                Some(path)
            }
            None => None,
        };
        let feature_count = project.features.len();
        let include_labels = request.include_labels.unwrap_or(true);
        let render_project = project.clone();
        let view = request.view.clone();
        let mut image = tokio::task::spawn_blocking(move || {
            libregene_core::map::render_map(&render_project, view.as_deref(), include_labels)
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?
        .map_err(|e| ErrorData::invalid_params(e, None))?;
        let mut response = ok_envelope(
            &id,
            match &output_path {
                Some(path) => format!("Rendered {} map to {}", image.view, path),
                None => format!("Rendered {} map as inline SVG", image.view),
            },
            None,
        );
        response["unit"] = serde_json::json!(unit);
        response["view"] = serde_json::json!(image.view);
        response["width"] = serde_json::json!(image.width);
        response["height"] = serde_json::json!(image.height);
        response["featureCount"] = serde_json::json!(feature_count);
        if let Some(path) = output_path {
            let svg = std::mem::take(&mut image.svg);
            let bytes = svg.len();
            let write_path = path.clone();
            tokio::task::spawn_blocking(move || {
                libregene_core::file_io::write_atomic(std::path::Path::new(&write_path), |file| {
                    std::io::Write::write_all(file, svg.as_bytes())
                })
            })
            .await
            .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?
            .map_err(|e| {
                ErrorData::invalid_params(format!("failed to write {}: {}", path, e), None)
            })?;
            response["path"] = serde_json::json!(path);
            response["bytesWritten"] = serde_json::json!(bytes);
        } else {
            response["svg"] = serde_json::json!(image.svg);
        }
        insert_seq_hashes(&mut response, &hashes);
        Ok(Json(response))
    }
}
