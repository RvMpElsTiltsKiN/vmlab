//! Arranque de Docker Desktop en segundo plano.
//!
//! Docker Desktop no ofrece un comando oficial para arrancar solo el motor:
//! `DockerCli.exe` únicamente cambia de motor o lo apaga. Lo que sí respeta es la
//! opción `OpenUIOnStartupDisabled` de `settings-store.json`, que impide abrir el
//! panel al iniciarse. VMLab activa esa opción, lanza `Docker Desktop.exe` oculto
//! y espera a que el motor acepte conexiones (named pipe o socket).
//!
//! Nota: en Windows el archivo de ajustes debe escribirse **sin BOM**; Docker lo
//! rechaza y el backend se cae al arrancar.
use crate::error::{CoreError, Result};
use crate::model::DockerStartInfo;
use crate::runner::run_ok;
use crate::tools::Tools;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// Docker puede tardar bastante en arrancar WSL y el motor la primera vez.
const PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const POLL_EVERY: Duration = Duration::from_secs(2);

/// Ruta del ejecutable de Docker Desktop, si está instalado.
pub fn desktop_exe() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    #[cfg(windows)]
    {
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(
                PathBuf::from(&local)
                    .join("Programs")
                    .join("DockerDesktop")
                    .join("Docker Desktop.exe"),
            );
        }
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            candidates.push(PathBuf::from(&pf).join("Docker").join("Docker").join("Docker Desktop.exe"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        candidates.push("/Applications/Docker.app/Contents/MacOS/Docker Desktop".into());
    }

    candidates.into_iter().find(|p| p.is_file())
}

/// `true` si el motor de Docker ya responde (`docker version` obtiene la versión del servidor).
pub async fn engine_ready(tools: &Tools) -> bool {
    let Ok(bin) = tools.docker() else { return false };
    match run_ok(bin, &["version", "--format", "{{.Server.Version}}"], PROBE_TIMEOUT).await {
        Ok(out) => !out.stdout.trim().is_empty(),
        Err(_) => false,
    }
}

/// Arranca Docker Desktop si hace falta y espera a que el motor esté listo.
///
/// `wait_secs` se limita a 10–300 s. Nunca deja procesos colgados: el lanzador se
/// desacopla y el sondeo termina por timeout.
pub async fn start(tools: &Tools, wait_secs: u64) -> Result<DockerStartInfo> {
    let exe = desktop_exe();

    if engine_ready(tools).await {
        return Ok(DockerStartInfo {
            already_running: true,
            launched: false,
            engine_ready: true,
            waited_secs: 0,
            desktop: exe.map(|p| p.display().to_string()).unwrap_or_default(),
            message: "Docker ya estaba en marcha.".into(),
        });
    }

    let exe = exe.ok_or(CoreError::ToolMissing("Docker Desktop"))?;

    // Si Docker Desktop ya está abierto pero aún arrancando, no se vuelve a lanzar:
    // una segunda ejecución podría abrir el panel. Sus ajustes ya están cargados.
    let was_open = desktop_running();
    if !was_open {
        // Mejor esfuerzo: si no se puede reescribir su configuración, se arranca igual.
        let _ = prepare_settings();
        spawn_desktop(&exe)?;
    }

    let started = Instant::now();
    let limit = Duration::from_secs(wait_secs.clamp(10, 300));

    loop {
        if engine_ready(tools).await {
            return Ok(DockerStartInfo {
                already_running: was_open,
                launched: !was_open,
                engine_ready: true,
                waited_secs: started.elapsed().as_secs(),
                desktop: exe.display().to_string(),
                message: if was_open {
                    "Docker Desktop ya estaba abierto; el motor está listo.".into()
                } else {
                    "Docker Desktop arrancó en segundo plano y el motor está listo.".into()
                },
            });
        }
        if started.elapsed() >= limit {
            break;
        }
        tokio::time::sleep(POLL_EVERY).await;
    }

    Ok(DockerStartInfo {
        already_running: was_open,
        launched: !was_open,
        engine_ready: false,
        waited_secs: started.elapsed().as_secs(),
        desktop: exe.display().to_string(),
        message: if was_open {
            "Docker Desktop ya estaba abierto, pero el motor no responde. Revisa su ventana o el estado de WSL.".into()
        } else {
            "Docker Desktop se lanzó, pero el motor no respondió. Abre Docker Desktop para ver el error o revisa WSL.".into()
        },
    })
}

fn io_err(e: std::io::Error) -> CoreError {
    CoreError::Io(e.to_string())
}

/// Devuelve el JSON de ajustes con el panel desactivado, conservando el resto de claves.
pub fn silent_settings(json: &str) -> Result<String> {
    // Un BOM al principio hace que Docker rechace el archivo; se descarta.
    let cleaned = json.strip_prefix('\u{feff}').unwrap_or(json);
    let mut value: serde_json::Value = if cleaned.trim().is_empty() {
        serde_json::json!({})
    } else {
        serde_json::from_str(cleaned)
            .map_err(|e| CoreError::Invalid(format!("configuración de Docker Desktop ilegible: {e}")))?
    };
    let obj = value
        .as_object_mut()
        .ok_or_else(|| CoreError::Invalid("la configuración de Docker Desktop no es un objeto JSON".into()))?;
    if obj.get("OpenUIOnStartupDisabled").and_then(serde_json::Value::as_bool) == Some(true) {
        return Ok(cleaned.to_string());
    }
    obj.insert("OpenUIOnStartupDisabled".into(), serde_json::Value::Bool(true));
    serde_json::to_string_pretty(&value).map_err(|e| CoreError::Io(e.to_string()))
}

fn settings_path() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA")
            .map(|p| PathBuf::from(p).join("Docker").join("settings-store.json"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|p| {
            PathBuf::from(p)
                .join("Library")
                .join("Group Containers")
                .join("group.com.docker")
                .join("settings-store.json")
        })
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".docker").join("desktop").join("settings-store.json"))
    }
}

/// Activa «no abrir el panel al iniciar» antes de lanzar Docker Desktop.
/// Si el archivo no existe todavía (primer arranque) no se inventa: Docker lo creará.
fn prepare_settings() -> Result<()> {
    let Some(path) = settings_path() else { return Ok(()) };
    if !path.is_file() {
        return Ok(());
    }
    apply_silent(&path)
}

fn apply_silent(path: &Path) -> Result<()> {
    let current = std::fs::read_to_string(path).map_err(io_err)?;
    let next = silent_settings(&current)?;
    if next == current {
        return Ok(());
    }
    // Temporal + rename: si algo falla, Docker conserva su configuración válida.
    let tmp = path.with_extension("json.vmlab.tmp");
    std::fs::write(&tmp, next.as_bytes()).map_err(io_err)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        io_err(e)
    })
}

/// `true` si el backend de Docker Desktop está vivo (aunque el motor aún no lo esté).
fn desktop_running() -> bool {
    #[cfg(windows)]
    {
        Path::new(r"\\.\pipe\dockerBackendApiServer").exists()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

fn spawn_desktop(exe: &Path) -> Result<()> {
    let mut cmd = std::process::Command::new(exe);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Sin consola ni ventana propia: la visibilidad la controla
        // OpenUIOnStartupDisabled dentro de Docker Desktop.
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn()
        .map(|_| ())
        .map_err(|source| CoreError::Spawn { program: "Docker Desktop".into(), source })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enables_the_setting_without_losing_others() {
        let out = silent_settings(r#"{"AutoStart":false,"EnableDockerAI":true}"#).unwrap();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["AutoStart"], serde_json::Value::Bool(false));
        assert_eq!(v["EnableDockerAI"], serde_json::Value::Bool(true));
        assert_eq!(v["OpenUIOnStartupDisabled"], serde_json::Value::Bool(true));
        assert!(!out.starts_with('\u{feff}'));
    }

    #[test]
    fn is_idempotent() {
        let once = silent_settings("{}").unwrap();
        let twice = silent_settings(&once).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn replaces_the_file_without_a_bom() {
        let path = std::env::temp_dir().join(format!("vmlab-settings-{}.json", std::process::id()));
        std::fs::write(&path, r#"{"AutoStart":false,"OpenUIOnStartupDisabled":false}"#).unwrap();
        apply_silent(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_ne!(bytes[0], 0xEF);
        let text = String::from_utf8(bytes).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["AutoStart"], serde_json::Value::Bool(false));
        assert_eq!(v["OpenUIOnStartupDisabled"], serde_json::Value::Bool(true));
        // Segunda pasada: no deja temporales ni cambia el contenido.
        apply_silent(&path).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert!(!path.with_extension("json.vmlab.tmp").exists());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_damaged_settings() {
        assert!(silent_settings("{ not json").is_err());
    }
}
