//! Migración única desde el identificador antiguo `dev.lablab.app`.
//!
//! Al cambiar el identificador de la app, Tauri y WebView2 usan carpetas de datos
//! nuevas (`%APPDATA%` y `%LOCALAPPDATA%`). Antes de crear la ventana se copian el
//! espacio de trabajo guardado y el almacenamiento local (tema y bloqueo del lienzo)
//! de la instalación anterior. Es idempotente: nunca sobrescribe datos nuevos.

#[cfg(windows)]
pub fn legacy_data() {
    use std::path::PathBuf;

    let (Some(roaming), Some(local)) = (std::env::var_os("APPDATA"), std::env::var_os("LOCALAPPDATA")) else {
        return;
    };
    let roaming = PathBuf::from(roaming);
    let local = PathBuf::from(local);

    let old_workspace = roaming.join("dev.lablab.app").join("workspace.json");
    let new_workspace = roaming.join("dev.vmlab.app").join("workspace.json");
    copy_if_missing(&old_workspace, &new_workspace);

    let old_storage = local
        .join("dev.lablab.app")
        .join("EBWebView")
        .join("Default")
        .join("Local Storage")
        .join("leveldb");
    let new_storage = local
        .join("dev.vmlab.app")
        .join("EBWebView")
        .join("Default")
        .join("Local Storage")
        .join("leveldb");
    copy_dir_if_missing(&old_storage, &new_storage);
}

#[cfg(not(windows))]
pub fn legacy_data() {}

#[cfg(windows)]
fn copy_if_missing(from: &std::path::Path, to: &std::path::Path) {
    if !from.is_file() || to.exists() {
        return;
    }
    if let Some(parent) = to.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::copy(from, to);
}

/// Copia un LevelDB de localStorage solo si el destino aún no tiene datos.
/// Se omiten los ficheros de bloqueo y de diagnóstico: WebView2 los recrea.
#[cfg(windows)]
fn copy_dir_if_missing(from: &std::path::Path, to: &std::path::Path) {
    if !from.is_dir() || to.join("CURRENT").exists() {
        return;
    }
    if std::fs::create_dir_all(to).is_err() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(from) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if matches!(name.to_str(), Some("LOCK" | "LOG" | "LOG.old")) {
            continue;
        }
        if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            let _ = std::fs::copy(entry.path(), to.join(name));
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::fs;

    fn temp(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("vmlab-migrate-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn copies_the_workspace_only_once() {
        let root = temp("workspace");
        let old = root.join("old.json");
        let new = root.join("new.json");
        fs::write(&old, "{\"version\":1}").unwrap();
        copy_if_missing(&old, &new);
        assert_eq!(fs::read_to_string(&new).unwrap(), "{\"version\":1}");
        fs::write(&new, "{\"version\":2}").unwrap();
        copy_if_missing(&old, &new);
        assert_eq!(fs::read_to_string(&new).unwrap(), "{\"version\":2}");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn copies_local_storage_without_lock_files() {
        let root = temp("storage");
        let old = root.join("old");
        let new = root.join("new");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join("CURRENT"), "MANIFEST-000001\n").unwrap();
        fs::write(old.join("000003.log"), "datos").unwrap();
        fs::write(old.join("LOCK"), "").unwrap();
        copy_dir_if_missing(&old, &new);
        assert!(new.join("CURRENT").is_file());
        assert!(new.join("000003.log").is_file());
        assert!(!new.join("LOCK").exists());
        let _ = fs::remove_dir_all(&root);
    }
}
