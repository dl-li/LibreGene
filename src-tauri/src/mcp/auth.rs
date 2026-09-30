//! Bearer-token persistence for the MCP server.

use tauri::{AppHandle, Manager, Runtime};

/// Constant-time string equality for the bearer token, so a local caller
/// cannot recover it byte by byte via timing. Length leaks are accepted (the
/// token is a fixed-length random string).
pub(crate) fn token_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b.iter()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Path of the persisted bearer token file. Best-effort: returns None when
/// the config dir is unavailable (e.g. under the mock test runtime).
pub(crate) fn token_file_path<R: Runtime>(app: &AppHandle<R>) -> Option<std::path::PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("mcp_auth_token"))
}

pub(crate) fn persist_token<R: Runtime>(app: &AppHandle<R>, token: &str) {
    if let Some(path) = token_file_path(app) {
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                log::warn!("failed to create MCP token dir {}: {}", parent.display(), e);
            }
            restrict_token_dir(parent);
        }
        write_token_file(&path, token);
        // Covers files that already existed with loose permissions (mode()
        // only applies at creation time).
        restrict_token_file(&path);
    }
}

/// Create with 0600 from the start so the token never exists at the
/// umask-default 0644, not even between write and chmod.
#[cfg(unix)]
pub(crate) fn write_token_file(path: &std::path::Path, token: &str) {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    if let Err(e) = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .and_then(|mut f| f.write_all(token.as_bytes()))
    {
        // The in-memory token keeps working; only the persistence diverged.
        log::warn!("failed to persist MCP auth token to {}: {}", path.display(), e);
    }
}

#[cfg(not(unix))]
pub(crate) fn write_token_file(path: &std::path::Path, token: &str) {
    if let Err(e) = std::fs::write(path, token) {
        log::warn!("failed to persist MCP auth token to {}: {}", path.display(), e);
    }
}

/// Tighten permissions on the token file itself.
///
/// The token is the single shared secret guarding the loopback MCP server,
/// so on multi-user Unix a default 0644 would let other local users read it
/// and impersonate the MCP client. 0600 restricts it to the owner.
#[cfg(unix)]
pub(crate) fn restrict_token_file(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
pub(crate) fn restrict_token_file(_path: &std::path::Path) {
    // Windows %APPDATA% inherits a user-only ACL by default; token_file_path
    // already derives from app_config_dir, so no extra tightening is needed.
}

#[cfg(unix)]
pub(crate) fn restrict_token_dir(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o700);
        let _ = std::fs::set_permissions(path, perms);
    }
}

#[cfg(not(unix))]
pub(crate) fn restrict_token_dir(_path: &std::path::Path) {}

/// Load the persisted token, or generate and persist a fresh one on first run.
pub(crate) fn load_or_create_token<R: Runtime>(app: &AppHandle<R>) -> String {
    if let Some(path) = token_file_path(app) {
        match std::fs::read_to_string(&path) {
            Ok(contents) => {
                let token = contents.trim().to_string();
                if !token.is_empty() {
                    // Token files persisted by older versions may still be 0644;
                    // tighten on load so upgrading users are covered without
                    // having to rotate the token.
                    if let Some(parent) = path.parent() {
                        restrict_token_dir(parent);
                    }
                    restrict_token_file(&path);
                    return token;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            // Unreadable token file (permissions, I/O): regenerating silently
            // would desync the running server from the on-disk token.
            Err(e) => log::warn!(
                "failed to read persisted MCP auth token from {}: {} — generating a fresh one",
                path.display(),
                e
            ),
        }
        let token = generate_auth_token();
        persist_token(app, &token);
        return token;
    }
    generate_auth_token()
}

/// Generate a 32-byte random bearer token, hex-encoded (64 chars).
/// Filled from the OS CSPRNG (getrandom) on every platform; the time/pid
/// mixing below is only a last-resort fallback when the CSPRNG itself fails
/// (a local-only shared secret, so no crypto crate is pulled in).
pub(crate) fn generate_auth_token() -> String {
    let mut buf = [0u8; 32];
    if let Err(e) = getrandom::getrandom(&mut buf) {
        log::warn!("OS CSPRNG failed ({e}); falling back to time/pid mixing for the MCP auth token");
        use std::time::{SystemTime, UNIX_EPOCH};
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
            ^ (std::process::id() as u64);
        let mut s = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        for b in buf.iter_mut() {
            s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            *b = (s >> 33) as u8;
        }
    }
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}
