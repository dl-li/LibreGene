//! Settings → General → "Set as default app": register LibreGene as the OS
//! handler for every supported sequence-file extension.

use serde_json::{json, Value};

/// Mirrors `bundle.fileAssociations` in tauri.conf.json (extensions and
/// grouping) — keep in sync. The MIME type is only used on Linux.
const ASSOC_GROUPS: &[(&str, &[&str], &str)] = &[
    (
        "GenBank",
        &["gbk", "gb", "genbank", "gbf", "gbff"],
        "application/x-genbank",
    ),
    (
        "GenPept/protein",
        &["gpt", "gp", "gpe", "gpff", "prot"],
        "application/x-genpept",
    ),
    ("SnapGene", &["dna", "rna"], "application/x-snapgene"),
    (
        "FASTA",
        &["fasta", "fa", "fna", "fas", "ffn", "fsa", "faa", "frn"],
        "text/x-fasta",
    ),
    ("ABI trace", &["ab1"], "application/x-ab1"),
    ("Plain sequence", &["seq"], "text/x-seq"),
];

#[tauri::command]
pub(crate) async fn set_default_file_associations(
    app: tauri::AppHandle,
) -> Result<Value, String> {
    let bundle_id = app.config().identifier.clone();
    tokio::task::spawn_blocking(move || imp::set_all(&bundle_id))
        .await
        .map_err(|e| e.to_string())?
}

fn finish(groups: Vec<Value>, failures: &[&str]) -> Value {
    if failures.is_empty() {
        json!({
            "ok": true,
            "message": "LibreGene is now the default app for all supported formats.",
            "groups": groups,
        })
    } else {
        json!({
            "ok": false,
            "message": format!("Could not set the default for: {}", failures.join(", ")),
            "groups": groups,
        })
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::{finish, ASSOC_GROUPS};
    use serde_json::{json, Value};
    use std::ffi::{c_char, c_void, CString};

    type CFStringRef = *const c_void;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringCreateWithCString(
            alloc: *const c_void,
            c_str: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFRelease(cf: *const c_void);
    }

    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        fn UTTypeCreatePreferredIdentifierForTag(
            tag_class: CFStringRef,
            tag: CFStringRef,
            conforming_to: CFStringRef,
        ) -> CFStringRef;
        fn LSSetDefaultRoleHandlerForContentType(
            content_type: CFStringRef,
            role: u32,
            handler_bundle_id: CFStringRef,
        ) -> i32;
    }

    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;
    const K_LS_ROLES_ALL: u32 = 0xffff_ffff;

    struct CFStr(CFStringRef);
    impl CFStr {
        fn new(s: &str) -> Result<Self, String> {
            let c = CString::new(s).map_err(|_| format!("invalid string '{s}'"))?;
            let r = unsafe {
                CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), K_CF_STRING_ENCODING_UTF8)
            };
            if r.is_null() {
                Err(format!("CFStringCreateWithCString failed for '{s}'"))
            } else {
                Ok(Self(r))
            }
        }
    }
    impl Drop for CFStr {
        fn drop(&mut self) {
            unsafe { CFRelease(self.0) }
        }
    }

    fn set_default_for_ext(ext: &str, bundle_id: &CFStr) -> Result<(), String> {
        let tag_class = CFStr::new("public.filename-extension")?;
        let ext_tag = CFStr::new(ext)?;
        let uti =
            unsafe { UTTypeCreatePreferredIdentifierForTag(tag_class.0, ext_tag.0, std::ptr::null()) };
        if uti.is_null() {
            return Err(format!("no UTI registered for .{ext}"));
        }
        // The UTI string follows the Create rule (+1 retain).
        let status =
            unsafe { LSSetDefaultRoleHandlerForContentType(uti, K_LS_ROLES_ALL, bundle_id.0) };
        unsafe { CFRelease(uti) };
        if status == 0 {
            Ok(())
        } else {
            Err(format!("LaunchServices error {status}"))
        }
    }

    pub(super) fn set_all(bundle_id: &str) -> Result<Value, String> {
        let bid = CFStr::new(bundle_id)?;
        let mut groups = Vec::new();
        let mut failures = Vec::new();
        for (name, exts, _) in ASSOC_GROUPS {
            let errs: Vec<String> = exts
                .iter()
                .filter_map(|ext| set_default_for_ext(ext, &bid).err().map(|e| format!(".{ext}: {e}")))
                .collect();
            if errs.is_empty() {
                groups.push(json!({ "name": name, "ok": true }));
            } else {
                groups.push(json!({ "name": name, "ok": false, "error": errs.join("; ") }));
                failures.push(*name);
            }
        }
        Ok(finish(groups, &failures))
    }
}

#[cfg(target_os = "windows")]
mod imp {
    use super::{finish, ASSOC_GROUPS};
    use serde_json::{json, Value};
    use std::ffi::c_void;
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    #[link(name = "shell32")]
    extern "system" {
        fn SHChangeNotify(event_id: i32, flags: u32, item1: *const c_void, item2: *const c_void);
    }
    const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
    const SHCNF_IDLIST: u32 = 0;

    /// Per-user (HKCU) registration: takes precedence over the machine-wide
    /// classes written by the installer and needs no admin rights.
    fn set_default_for_ext(ext: &str, label: &str, exe: &str) -> Result<(), String> {
        let progid = format!("LibreGene.{ext}");
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (ext_key, _) = hkcu
            .create_subkey(format!("Software\\Classes\\.{ext}"))
            .map_err(|e| e.to_string())?;
        ext_key.set_value("", &progid).map_err(|e| e.to_string())?;
        let (open_with, _) = ext_key
            .create_subkey("OpenWithProgids")
            .map_err(|e| e.to_string())?;
        open_with
            .set_value(&progid, &"")
            .map_err(|e| e.to_string())?;
        let (prog, _) = hkcu
            .create_subkey(format!("Software\\Classes\\{progid}"))
            .map_err(|e| e.to_string())?;
        prog.set_value("", &format!("{label} (.{ext})"))
            .map_err(|e| e.to_string())?;
        let (icon, _) = prog
            .create_subkey("DefaultIcon")
            .map_err(|e| e.to_string())?;
        icon.set_value("", &format!("\"{exe}\",0"))
            .map_err(|e| e.to_string())?;
        let (cmd, _) = prog
            .create_subkey("shell\\open\\command")
            .map_err(|e| e.to_string())?;
        cmd.set_value("", &format!("\"{exe}\" \"%1\""))
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub(super) fn set_all(_bundle_id: &str) -> Result<Value, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = exe.to_string_lossy().into_owned();
        let mut groups = Vec::new();
        let mut failures = Vec::new();
        for (name, exts, _) in ASSOC_GROUPS {
            let errs: Vec<String> = exts
                .iter()
                .filter_map(|ext| {
                    set_default_for_ext(ext, name, &exe)
                        .err()
                        .map(|e| format!(".{ext}: {e}"))
                })
                .collect();
            if errs.is_empty() {
                groups.push(json!({ "name": name, "ok": true }));
            } else {
                groups.push(json!({ "name": name, "ok": false, "error": errs.join("; ") }));
                failures.push(*name);
            }
        }
        // Tell the shell the association cache is stale.
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, std::ptr::null(), std::ptr::null()) };
        Ok(finish(groups, &failures))
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::{finish, ASSOC_GROUPS};
    use serde_json::{json, Value};
    use std::process::Command;

    fn mime_package_xml() -> String {
        let mut out = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <mime-info xmlns=\"http://www.freedesktop.org/standards/shared-mime-info\">\n",
        );
        for (name, exts, mime) in ASSOC_GROUPS {
            out.push_str(&format!("  <mime-type type=\"{mime}\">\n    <comment>{name} file</comment>\n"));
            for ext in *exts {
                out.push_str(&format!("    <glob pattern=\"*.{ext}\"/>\n"));
            }
            out.push_str("  </mime-type>\n");
        }
        out.push_str("</mime-info>\n");
        out
    }

    /// Linux file managers match by MIME type, not extension: install custom
    /// MIME definitions under ~/.local/share/mime, then point each type at
    /// our .desktop file. Best-effort — inside a Flatpak sandbox xdg-mime may
    /// only affect the sandbox's own view, and failures are reported per type.
    pub(super) fn set_all(_bundle_id: &str) -> Result<Value, String> {
        let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_string())?;
        let mime_dir = format!("{home}/.local/share/mime");
        let pkg_dir = format!("{mime_dir}/packages");
        std::fs::create_dir_all(&pkg_dir).map_err(|e| e.to_string())?;
        std::fs::write(format!("{pkg_dir}/libregene.xml"), mime_package_xml())
            .map_err(|e| e.to_string())?;
        let _ = Command::new("update-mime-database").arg(&mime_dir).output();
        let mut groups = Vec::new();
        let mut failures = Vec::new();
        for &(name, _, mime) in ASSOC_GROUPS {
            let ok = Command::new("xdg-mime")
                .args(["default", "com.libregene.app.desktop", mime])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if ok {
                groups.push(json!({ "name": name, "ok": true }));
            } else {
                groups.push(json!({ "name": name, "ok": false, "error": "xdg-mime failed" }));
                failures.push(name);
            }
        }
        Ok(finish(groups, &failures))
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
mod imp {
    use super::finish;
    use serde_json::Value;

    pub(super) fn set_all(_bundle_id: &str) -> Result<Value, String> {
        Ok(finish(Vec::new(), &["this platform"]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::SEQ_EXTS;

    #[test]
    fn assoc_groups_match_sequence_extensions() {
        let grouped: Vec<&str> = ASSOC_GROUPS
            .iter()
            .flat_map(|(_, exts, _)| exts.iter().copied())
            .collect();
        for ext in SEQ_EXTS {
            assert!(
                grouped.contains(ext),
                "SEQ_EXTS entry .{ext} is missing from ASSOC_GROUPS"
            );
        }
        assert_eq!(
            grouped.len(),
            SEQ_EXTS.len(),
            "ASSOC_GROUPS must register exactly the SEQ_EXTS extensions"
        );
    }

    #[test]
    fn assoc_groups_have_unique_extensions() {
        let mut grouped: Vec<&str> = ASSOC_GROUPS
            .iter()
            .flat_map(|(_, exts, _)| exts.iter().copied())
            .collect();
        grouped.sort_unstable();
        let len = grouped.len();
        grouped.dedup();
        assert_eq!(len, grouped.len(), "duplicate extension in ASSOC_GROUPS");
    }
}
