//! Orquestación: combina `vbox`, `docker` y `topology`. Es la única API que usa la capa Tauri.
use crate::error::{CoreError, Result};
use crate::model::*;
use crate::tools::Tools;
use crate::{connectivity, docker, docker_desktop, guest, topology, vbox};
use std::collections::{HashMap, HashSet};

fn vbox_state(running: bool) -> PowerState {
    if running { PowerState::Running } else { PowerState::Stopped }
}

pub async fn inventory(tools: &Tools) -> Inventory {
    let (vb, dk) = tokio::join!(
        async { if tools.vboxmanage.is_some() { Some(vbox::inventory(tools).await) } else { None } },
        async { if tools.docker.is_some() { Some(docker::list(tools).await) } else { None } },
    );
    let mut inv = Inventory::default();
    match vb {
        Some(Ok(list)) => {
            inv.vbox = list
                .into_iter()
                .map(|v| MachineInfo {
                    kind: NodeKind::Vbox,
                    target: v.uuid,
                    label: v.name,
                    state: vbox_state(v.running),
                    image: None,
                    guest_additions: Some(v.guest_additions),
                })
                .collect()
        }
        Some(Err(e)) => inv.vbox_error = Some(e.to_string()),
        None => inv.vbox_error = Some(CoreError::ToolMissing("VBoxManage").to_string()),
    }
    match dk {
        Some(Ok(list)) => {
            inv.docker = list
                .into_iter()
                .map(|c| MachineInfo {
                    kind: NodeKind::Docker,
                    target: c.name.clone(),
                    label: c.name,
                    state: c.state,
                    image: Some(c.image),
                    guest_additions: None,
                })
                .collect()
        }
        Some(Err(e)) => {
            let raw = e.to_string();
            inv.docker_engine_off = docker::engine_is_down(&raw);
            inv.docker_error = Some(if inv.docker_engine_off {
                "Docker Desktop no está en marcha. Pulsa «Iniciar Docker» para arrancarlo en segundo plano.".into()
            } else {
                raw.clone()
            });
            inv.docker_error_detail = Some(raw);
        }
        None => {
            inv.docker_error = Some(CoreError::ToolMissing("docker").to_string());
            inv.docker_engine_off = false;
            inv.docker_error_detail = None;
        }
    }
    inv
}

/// Estado de todas las máquinas en 2 llamadas (una por tecnología), pensado para sondeo periódico.
pub async fn snapshot(tools: &Tools) -> Snapshot {
    let (vb, dk) = tokio::join!(
        async { if tools.vboxmanage.is_some() { vbox::inventory(tools).await.ok() } else { None } },
        async { if tools.docker.is_some() { docker::list(tools).await.ok() } else { None } },
    );
    let mut states: HashMap<String, PowerState> = HashMap::new();
    let (vbox_ok, docker_ok) = (vb.is_some(), dk.is_some());
    for vm in vb.unwrap_or_default() {
        let s = vbox_state(vm.running);
        states.insert(NodeKind::Vbox.key(&vm.uuid), s);
        states.insert(NodeKind::Vbox.key(&vm.name), s);
    }
    for c in dk.unwrap_or_default() {
        states.insert(NodeKind::Docker.key(&c.name), c.state);
    }
    Snapshot { states, vbox_ok, docker_ok }
}

pub async fn act(tools: &Tools, kind: NodeKind, target: &str, image: Option<&str>, a: Action) -> Result<()> {
    match kind {
        NodeKind::Vbox => vbox::action(tools, target, a).await,
        NodeKind::Docker => docker::action(tools, target, image, a).await,
    }
}

/// Detecta el gestor de red Linux dentro de la VM (Guest Additions).
pub async fn guest_probe(tools: &Tools, target: &str, username: &str, password: &str) -> Result<GuestProfile> {
    guest::probe(tools, target, username, password).await
}

/// Escribe y aplica la configuración de red dentro de la VM Linux.
pub async fn guest_apply(tools: &Tools, request: GuestConfigRequest) -> Result<GuestReport> {
    guest::apply(tools, request).await
}

/// Prepara la instalación de Guest Additions: monta la ISO y abre la VM con ventana.
pub async fn guest_additions_prepare(tools: &Tools, target: &str) -> Result<GuestAdditionsPrep> {
    vbox::prepare_guest_additions(tools, target).await
}

/// Comprueba la comunicación real de los nodos del lienzo (red interna e IPv4).
pub async fn network_probe(tools: &Tools, nodes: Vec<ProbeNode>) -> Vec<NodeProbe> {
    connectivity::probe(tools, nodes).await
}

/// Arranca Docker Desktop en segundo plano (sin abrir el panel) y espera al motor.
pub async fn docker_start(tools: &Tools, wait_secs: u64) -> Result<DockerStartInfo> {
    docker_desktop::start(tools, wait_secs).await
}

/// Aplica la topología del lienzo al sistema. Nunca aborta a la mitad: acumula errores por nodo.
pub async fn apply(tools: &Tools, t: &Topology) -> ApplyReport {
    let plan = topology::plan(t);
    let mut rep = ApplyReport { warnings: plan.warnings.clone(), ..Default::default() };
    let running: HashSet<String> = if tools.vboxmanage.is_some() {
        vbox::running_keys(tools).await.unwrap_or_default()
    } else {
        HashSet::new()
    };

    for seg in &plan.segments {
        rep.networks.push(seg.name.clone());
        let mut docker_net_ready = false;
        for n in &seg.nodes {
            let net = n.net.clone().unwrap_or_default();
            if n.kind == NodeKind::Vbox && net.mode != NetMode::Intnet {
                rep.warnings.push(format!(
                    "{} usa {}: no se conectará a la red interna {} del lienzo.",
                    n.display(),
                    net.mode.label(),
                    seg.name
                ));
            }
            let res: Result<()> = match n.kind {
                NodeKind::Vbox if running.contains(&n.target) => Err(CoreError::Invalid(
                    "apaga la VM para poder cambiar su adaptador de red".into(),
                )),
                NodeKind::Vbox => vbox::set_nic(tools, &n.target, &net, Some(&seg.name)).await,
                NodeKind::Docker => {
                    let ready = if docker_net_ready { Ok(()) } else { docker::ensure_network(tools, &seg.name).await };
                    docker_net_ready = ready.is_ok();
                    match ready {
                        Ok(()) => docker::set_networks(tools, &n.target, Some(&seg.name)).await,
                        Err(e) => Err(e),
                    }
                }
            };
            match res {
                Ok(()) => rep.applied += 1,
                Err(e) => rep.errors.push(format!("{}: {e}", n.display())),
            }
        }
    }

    // DHCP automático solo para los segmentos que realmente usan la red interna de VMLab.
    let vmlab_intnet = |nodes: &[NodeSpec]| {
        nodes
            .iter()
            .any(|n| n.kind == NodeKind::Vbox && n.net.as_ref().map_or(true, |c| c.mode == NetMode::Intnet))
    };
    for seg in plan.segments.iter().filter(|s| vmlab_intnet(&s.nodes)) {
        if let Err(e) = vbox::ensure_dhcp(tools, &seg.name).await {
            rep.warnings.push(format!("No se pudo crear el servidor DHCP de {}: {e}. Tendrás que asignar IPs a mano.", seg.name));
        }
    }

    for n in &plan.isolated {
        let net = n.net.clone().unwrap_or_default();
        let res = match n.kind {
            // Una VM encendida y sin enlaces no se toca (evita ruido al aplicar con la red en marcha).
            NodeKind::Vbox if running.contains(&n.target) => Ok(()),
            NodeKind::Vbox => vbox::set_nic(tools, &n.target, &net, None).await,
            NodeKind::Docker => docker::set_networks(tools, &n.target, None).await,
        };
        if let Err(e) = res {
            rep.errors.push(format!("{}: {e}", n.display()));
        }
    }

    if tools.docker.is_some() {
        let _ = docker::prune_networks(tools).await;
    }
    rep
}
