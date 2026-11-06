//! Sonda de comunicación del lienzo.
//!
//! El color de cada cable depende de datos reales, no solo de que la máquina esté
//! encendida: se lee la IPv4 del adaptador que gestiona VMLab (guest properties de
//! VirtualBox) o de la red `vmlab-*` del contenedor. Nunca se ejecuta nada dentro
//! del guest, así que no hacen falta usuario ni contraseña.
use crate::model::{LinkInfo, NodeKind, NodeProbe, ProbeNode};
use crate::tools::Tools;
use crate::{docker, vbox};

const DEFAULT_ADAPTER: u8 = 2;

async fn probe_one(tools: &Tools, node: ProbeNode) -> NodeProbe {
    let link: crate::error::Result<LinkInfo> = match node.kind {
        NodeKind::Vbox => {
            vbox::guest_link(tools, &node.target, node.adapter.unwrap_or(DEFAULT_ADAPTER)).await
        }
        NodeKind::Docker => docker::vmlab_link(tools, &node.target).await,
    };
    match link {
        Ok(l) => NodeProbe {
            kind: node.kind,
            target: node.target,
            segment: l.segment,
            ipv4: l.ipv4,
            note: l.note,
        },
        Err(e) => NodeProbe {
            kind: node.kind,
            target: node.target,
            segment: None,
            ipv4: None,
            note: Some(e.to_string()),
        },
    }
}

/// Comprueba los nodos en serie: VirtualBox serializa el acceso a sus máquinas y
/// lanzar varias consultas a la vez provoca bloqueos temporales.
pub async fn probe(tools: &Tools, nodes: Vec<ProbeNode>) -> Vec<NodeProbe> {
    let mut out = Vec::with_capacity(nodes.len());
    for node in nodes {
        out.push(probe_one(tools, node).await);
    }
    out
}
