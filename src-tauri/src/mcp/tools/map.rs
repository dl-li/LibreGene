use base64::Engine;
use rmcp::{handler::server::wrapper::Json, ErrorData};
use tauri::Runtime;

use crate::mcp::support::{fail_envelope, insert_seq_hashes, ok_envelope, unit_for};
use crate::mcp::types::RenderMapRequest;
use crate::mcp::LibreGeneMcp;

fn render_jpeg(svg: &str) -> Result<Vec<u8>, String> {
    let mut options = resvg::usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = resvg::usvg::Tree::from_str(svg, &options)
        .map_err(|e| format!("failed to parse map SVG: {}", e))?;
    let size = tree.size().to_int_size();
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or_else(|| "map image dimensions are invalid".to_string())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::default(),
        &mut pixmap.as_mut(),
    );
    let width = u16::try_from(pixmap.width())
        .map_err(|_| "map image is too wide for JPEG output".to_string())?;
    let height = u16::try_from(pixmap.height())
        .map_err(|_| "map image is too tall for JPEG output".to_string())?;
    let mut bytes = Vec::new();
    jpeg_encoder::Encoder::new(&mut bytes, 90)
        .encode(pixmap.data(), width, height, jpeg_encoder::ColorType::Rgba)
        .map_err(|e| format!("failed to encode map JPEG: {}", e))?;
    Ok(bytes)
}

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
        if let Some(format) = request.format.as_deref() {
            if !matches!(format, "jpg" | "jpeg") {
                return Ok(fail(format!(
                    "invalid format '{}': render_map only outputs JPG",
                    format
                )));
            }
        }
        let output_path = match request.output_path {
            Some(path) => {
                crate::validate_user_path(&path, &["jpg", "jpeg"]).map_err(|e| {
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
        let svg = std::mem::take(&mut image.svg);
        let jpeg = tokio::task::spawn_blocking(move || render_jpeg(&svg))
            .await
            .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?
            .map_err(|e| ErrorData::invalid_params(e, None))?;
        let mut response = ok_envelope(
            &id,
            match &output_path {
                Some(path) => format!("Rendered {} map to {}", image.view, path),
                None => format!("Rendered {} map as inline JPEG", image.view),
            },
            None,
        );
        response["unit"] = serde_json::json!(unit);
        response["view"] = serde_json::json!(image.view);
        response["width"] = serde_json::json!(image.width);
        response["height"] = serde_json::json!(image.height);
        response["featureCount"] = serde_json::json!(feature_count);
        response["format"] = serde_json::json!("jpg");
        response["mimeType"] = serde_json::json!("image/jpeg");
        if let Some(path) = output_path {
            let byte_count = jpeg.len();
            let write_path = path.clone();
            tokio::task::spawn_blocking(move || {
                libregene_core::file_io::write_atomic(std::path::Path::new(&write_path), |file| {
                    std::io::Write::write_all(file, &jpeg)
                })
            })
            .await
            .map_err(|e| ErrorData::internal_error(format!("task join error: {}", e), None))?
            .map_err(|e| {
                ErrorData::invalid_params(format!("failed to write {}: {}", path, e), None)
            })?;
            response["path"] = serde_json::json!(path);
            response["bytesWritten"] = serde_json::json!(byte_count);
        } else {
            response["jpg"] =
                serde_json::json!(base64::engine::general_purpose::STANDARD.encode(jpeg));
        }
        insert_seq_hashes(&mut response, &hashes);
        Ok(Json(response))
    }
}
