//! Ejecución de procesos externos: sin shell, con timeout y sin ventana de consola en Windows.
use crate::error::{CoreError, Result};
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
}

fn name_of(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// Ejecuta `program args...` y devuelve la salida aunque el código de retorno sea ≠ 0.
pub async fn run(program: &Path, args: &[&str], limit: Duration) -> Result<Output> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let child = cmd.spawn().map_err(|source| CoreError::Spawn { program: name_of(program), source })?;
    match tokio::time::timeout(limit, child.wait_with_output()).await {
        // Al expirar se descarta el future y `kill_on_drop` mata el proceso.
        Err(_) => Err(CoreError::Timeout { program: name_of(program), secs: limit.as_secs() }),
        Ok(Err(source)) => Err(CoreError::Spawn { program: name_of(program), source }),
        Ok(Ok(o)) => Ok(Output {
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            code: o.status.code().unwrap_or(-1),
        }),
    }
}

/// Como [`run`], pero un código ≠ 0 se convierte en [`CoreError::Failed`].
pub async fn run_ok(program: &Path, args: &[&str], limit: Duration) -> Result<Output> {
    let out = run(program, args, limit).await?;
    if out.code == 0 {
        return Ok(out);
    }
    let msg = if out.stderr.trim().is_empty() { &out.stdout } else { &out.stderr };
    let stderr: String = msg.trim().chars().take(500).collect();
    Err(CoreError::Failed { program: name_of(program), code: out.code, stderr })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn captures_stdout_without_shell() {
        // `$HOME` NO se expande: prueba de que no hay shell de por medio.
        let o = run_ok(Path::new("/bin/echo"), &["$HOME;id"], Duration::from_secs(5)).await.unwrap();
        assert_eq!(o.stdout.trim(), "$HOME;id");
    }

    #[tokio::test]
    async fn nonzero_exit_is_error() {
        let e = run_ok(Path::new("/bin/ls"), &["/no/existe/vmlab"], Duration::from_secs(5)).await.unwrap_err();
        assert!(matches!(e, CoreError::Failed { .. }));
    }

    #[tokio::test]
    async fn timeout_kills_process() {
        let e = run(Path::new("/bin/sleep"), &["10"], Duration::from_millis(200)).await.unwrap_err();
        assert!(matches!(e, CoreError::Timeout { .. }));
    }
}
