//! Control de contenedores con la CLI de Docker.
//! Cada segmento de red de VMLab es una red *bridge* de Docker llamada `vmlab-xxxxxxxx`.
use crate::error::{CoreError, Result};
use crate::model::{Action, LinkInfo, PowerState};
use crate::runner::run_ok;
use crate::tools::Tools;
use crate::validate;
use std::time::Duration;

const SHORT: Duration = Duration::from_secs(20);
const LONG: Duration = Duration::from_secs(300); // `docker run` puede tener que descargar la imagen

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerInfo {
    pub name: String,
    pub image: String,
    pub state: PowerState,
}

pub fn map_state(s: &str) -> PowerState {
    match s.trim() {
        "running" | "restarting" => PowerState::Running,
        "paused" => PowerState::Paused,
        "exited" | "created" | "dead" | "removing" => PowerState::Stopped,
        _ => PowerState::Unknown,
    }
}

/// Parsea la salida de `docker ps -a --format "{{.Names}}\t{{.Image}}\t{{.State}}"`.
pub fn parse_ps(out: &str) -> Vec<ContainerInfo> {
    out.lines()
        .filter_map(|l| {
            let mut p = l.split('\t');
            let (name, image, state) = (p.next()?, p.next()?, p.next()?);
            (!name.is_empty()).then(|| ContainerInfo { name: name.into(), image: image.into(), state: map_state(state) })
        })
        .collect()
}

/// `true` si el mensaje de error significa que el motor no está escuchando
/// (Docker Desktop cerrado, WSL detenido o pipe/socket inexistente).
pub fn engine_is_down(message: &str) -> bool {
    let m = message.to_ascii_lowercase();
    [
        "failed to connect to the docker api",
        "cannot connect to the docker daemon",
        "is the docker daemon running",
        "error during connect",
        "dockerdesktoplinuxengine",
        "pipe/docker_engine",
        "docker.sock",
    ]
    .iter()
    .any(|needle| m.contains(needle))
}

pub async fn list(tools: &Tools) -> Result<Vec<ContainerInfo>> {
    let out = run_ok(tools.docker()?, &["ps", "-a", "--format", "{{.Names}}\t{{.Image}}\t{{.State}}"], SHORT).await?;
    Ok(parse_ps(&out.stdout))
}

/// Pares `red=IP` de `docker inspect` (separados por `;`).
fn parse_networks(out: &str) -> Vec<(String, String)> {
    out.split(';')
        .filter_map(|pair| {
            let (name, ip) = pair.trim().split_once('=')?;
            (!name.is_empty()).then(|| (name.to_string(), ip.trim().to_string()))
        })
        .collect()
}

fn vmlab_of(nets: &[(String, String)]) -> Option<(String, String)> {
    // Durante la migración puede haber redes antiguas `lablab-*`; se prefieren las nuevas.
    let pick = nets
        .iter()
        .find(|(net, ip)| net.starts_with("vmlab-") && !ip.is_empty())
        .or_else(|| nets.iter().find(|(net, ip)| net.starts_with("lablab-") && !ip.is_empty()))?;
    Some((format!("docker:{}", pick.0), pick.1.clone()))
}

/// Red `vmlab-*` e IPv4 del contenedor. Sin credenciales: solo consulta Docker.
pub async fn vmlab_link(tools: &Tools, name: &str) -> Result<LinkInfo> {
    validate::container_name(name)?;
    let out = run_ok(
        tools.docker()?,
        &[
            "inspect",
            "-f",
            "{{range $k, $v := .NetworkSettings.Networks}}{{$k}}={{$v.IPAddress}};{{end}}",
            name,
        ],
        SHORT,
    )
    .await?;
    Ok(match vmlab_of(&parse_networks(&out.stdout)) {
        Some((segment, ipv4)) => LinkInfo { segment: Some(segment), ipv4: Some(ipv4), note: None },
        None => LinkInfo {
            segment: None,
            ipv4: None,
            note: Some("sin red VMLab o sin IP asignada".into()),
        },
    })
}

pub async fn action(tools: &Tools, name: &str, image: Option<&str>, action: Action) -> Result<()> {
    validate::container_name(name)?;
    let bin = tools.docker()?;
    match action {
        Action::Start | Action::StartGui => {
            match run_ok(bin, &["start", name], SHORT).await {
                Ok(_) => Ok(()),
                // El contenedor no existe todavía: se crea desde la imagen del nodo.
                Err(CoreError::Failed { stderr, .. }) if stderr.contains("No such container") => {
                    let image = image
                        .filter(|i| !i.is_empty())
                        .ok_or_else(|| CoreError::Invalid("el contenedor no existe y el nodo no tiene imagen".into()))?;
                    validate::docker_image(image)?;
                    run_ok(bin, &["run", "-d", "--name", name, image], LONG).await.map(|_| ())
                }
                Err(e) => Err(e),
            }
        }
        Action::Stop => run_ok(bin, &["stop", name], LONG).await.map(|_| ()),
        Action::ForceStop => run_ok(bin, &["kill", name], SHORT).await.map(|_| ()),
        Action::Restart => run_ok(bin, &["restart", name], LONG).await.map(|_| ()),
        Action::Pause => run_ok(bin, &["pause", name], SHORT).await.map(|_| ()),
        Action::Resume => run_ok(bin, &["unpause", name], SHORT).await.map(|_| ()),
    }
}

pub async fn ensure_network(tools: &Tools, net: &str) -> Result<()> {
    validate::network_name(net)?;
    let bin = tools.docker()?;
    if run_ok(bin, &["network", "inspect", net], SHORT).await.is_ok() {
        return Ok(());
    }
    run_ok(bin, &["network", "create", "--driver", "bridge", net], SHORT).await.map(|_| ())
}

async fn vmlab_networks_of(tools: &Tools, container: &str) -> Result<Vec<String>> {
    let out = run_ok(
        tools.docker()?,
        &["inspect", "-f", "{{range $k, $v := .NetworkSettings.Networks}}{{$k}} {{end}}", container],
        SHORT,
    )
    .await?;
    Ok(out
        .stdout
        .split_whitespace()
        .filter(|n| validate::managed_network(n))
        .map(String::from)
        .collect())
}

/// Deja al contenedor conectado **solo** a `keep` entre las redes gestionadas
/// (`vmlab-*` y las antiguas `lablab-*`, que se desconectan al migrar).
/// Es idempotente.
pub async fn set_networks(tools: &Tools, container: &str, keep: Option<&str>) -> Result<()> {
    validate::container_name(container)?;
    let bin = tools.docker()?;
    let current = match vmlab_networks_of(tools, container).await {
        Ok(c) => c,
        // Contenedor aún sin crear: nada que desconectar, y tampoco se puede conectar todavía.
        Err(CoreError::Failed { stderr, .. }) if stderr.contains("No such") => {
            return match keep {
                Some(_) => Err(CoreError::Invalid("el contenedor aún no existe: inícialo una vez y vuelve a aplicar".into())),
                None => Ok(()),
            };
        }
        Err(e) => return Err(e),
    };
    for net in current.iter().filter(|n| Some(n.as_str()) != keep) {
        run_ok(bin, &["network", "disconnect", "--force", net, container], SHORT).await?;
    }
    if let Some(net) = keep {
        validate::network_name(net)?;
        if !current.iter().any(|n| n == net) {
            run_ok(bin, &["network", "connect", net, container], SHORT).await?;
        }
    }
    Ok(())
}

/// Borra las redes `vmlab-*` y las antiguas `lablab-*` que ya no tienen
/// contenedores (las que estén en uso fallan y se ignoran).
pub async fn prune_networks(tools: &Tools) -> Result<()> {
    let bin = tools.docker()?;
    for filter in ["name=vmlab-", "name=lablab-"] {
        let out = run_ok(bin, &["network", "ls", "--filter", filter, "--format", "{{.Name}}"], SHORT).await?;
        for net in out.stdout.lines().map(str::trim).filter(|n| validate::managed_network(n)) {
            let _ = run_ok(bin, &["network", "rm", net], SHORT).await;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_an_engine_that_is_not_running() {
        let pipe = "docker terminó con error (1): failed to connect to the docker API at npipe:////./pipe/dockerDesktopLinuxEngine; check if the path is correct and if the daemon is running: open //./pipe/dockerDesktopLinuxEngine: El sistema no puede encontrar el archivo especificado.";
        assert!(engine_is_down(pipe));
        assert!(engine_is_down(
            "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?"
        ));
        assert!(!engine_is_down("docker terminó con error (125): No such container: web"));
        assert!(!engine_is_down("permission denied while trying to connect to the Docker daemon socket"));
    }

    #[test]
    fn parses_ps() {
        let out = "web\tnginx:latest\trunning\ndb\tpostgres:16\texited\n\nbroken line\n";
        let c = parse_ps(out);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].state, PowerState::Running);
        assert_eq!(c[1].state, PowerState::Stopped);
        assert_eq!(c[1].image, "postgres:16");
    }

    #[test]
    fn picks_the_vmlab_network_and_ignores_the_default_bridge() {
        let nets = parse_networks("bridge=172.17.0.2;vmlab-0a1b2c3d=172.18.0.2;");
        assert_eq!(nets.len(), 2);
        assert_eq!(
            vmlab_of(&nets),
            Some(("docker:vmlab-0a1b2c3d".into(), "172.18.0.2".into()))
        );
        assert_eq!(vmlab_of(&parse_networks("bridge=172.17.0.2;")), None);
        assert_eq!(vmlab_of(&parse_networks("vmlab-empty=;")), None);
    }

    #[test]
    fn recognizes_legacy_networks_during_migration_but_prefers_new_ones() {
        let legacy = parse_networks("bridge=172.17.0.2;lablab-0a1b2c3d=172.18.0.2;");
        assert_eq!(
            vmlab_of(&legacy),
            Some(("docker:lablab-0a1b2c3d".into(), "172.18.0.2".into()))
        );
        let both = parse_networks("lablab-0a1b2c3d=172.18.0.2;vmlab-0a1b2c3d=172.19.0.2;");
        assert_eq!(
            vmlab_of(&both),
            Some(("docker:vmlab-0a1b2c3d".into(), "172.19.0.2".into()))
        );
    }
}
