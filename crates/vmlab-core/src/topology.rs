//! Planificación pura (sin I/O): agrupa los nodos conectados por aristas en segmentos de red.
//! Cada componente conexo con ≥ 2 nodos es una red local; el resto son nodos aislados.
use crate::model::{NodeKind, NodeSpec, Topology};
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub name: String,
    pub nodes: Vec<NodeSpec>,
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub segments: Vec<Segment>,
    pub isolated: Vec<NodeSpec>,
    pub warnings: Vec<String>,
}

fn find(parent: &mut [usize], x: usize) -> usize {
    let mut root = x;
    while parent[root] != root {
        root = parent[root];
    }
    let mut cur = x;
    while parent[cur] != root {
        let next = parent[cur];
        parent[cur] = root;
        cur = next;
    }
    root
}

/// Nombre determinista: depende solo del conjunto de nodos, no del orden de entrada.
fn segment_name(ids: &mut Vec<&str>) -> String {
    ids.sort_unstable();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a
    for id in ids.iter() {
        for b in id.bytes().chain(std::iter::once(0xff)) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("vmlab-{:08x}", (h & 0xffff_ffff) as u32)
}

pub fn plan(t: &Topology) -> Plan {
    let index: HashMap<&str, usize> = t.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let mut parent: Vec<usize> = (0..t.nodes.len()).collect();
    for e in &t.edges {
        if let (Some(&a), Some(&b)) = (index.get(e.from.as_str()), index.get(e.to.as_str())) {
            let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
            if ra != rb {
                parent[ra] = rb;
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..t.nodes.len() {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }

    let mut plan = Plan::default();
    for members in groups.into_values() {
        let nodes: Vec<NodeSpec> = members.iter().map(|&i| t.nodes[i].clone()).collect();
        if nodes.len() < 2 {
            plan.isolated.extend(nodes);
            continue;
        }
        let name = segment_name(&mut nodes.iter().map(|n| n.id.as_str()).collect());
        let has_vbox = nodes.iter().any(|n| n.kind == NodeKind::Vbox);
        let has_docker = nodes.iter().any(|n| n.kind == NodeKind::Docker);
        if has_vbox && has_docker {
            plan.warnings.push(format!(
                "La red {name} mezcla VirtualBox y Docker: cada tecnología se conecta a su propia red, pero entre ellas no hay enlace automático."
            ));
        }
        plan.segments.push(Segment { name, nodes });
    }
    plan.segments.sort_by(|a, b| a.name.cmp(&b.name));
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Edge;

    fn n(id: &str, kind: NodeKind) -> NodeSpec {
        NodeSpec { id: id.into(), kind, target: id.into(), image: None, label: None, net: None }
    }
    fn e(a: &str, b: &str) -> Edge {
        Edge { from: a.into(), to: b.into() }
    }

    #[test]
    fn groups_components_and_isolates() {
        let t = Topology {
            nodes: vec![n("a", NodeKind::Vbox), n("b", NodeKind::Vbox), n("c", NodeKind::Docker), n("d", NodeKind::Docker), n("e", NodeKind::Docker)],
            edges: vec![e("a", "b"), e("c", "d"), e("x", "a"), e("e", "e")],
        };
        let p = plan(&t);
        assert_eq!(p.segments.len(), 2);
        assert_eq!(p.isolated.len(), 1);
        assert_eq!(p.isolated[0].id, "e");
        assert!(p.warnings.is_empty());
    }

    #[test]
    fn transitive_links_merge() {
        let t = Topology {
            nodes: vec![n("a", NodeKind::Docker), n("b", NodeKind::Docker), n("c", NodeKind::Docker)],
            edges: vec![e("a", "b"), e("c", "b")],
        };
        let p = plan(&t);
        assert_eq!(p.segments.len(), 1);
        assert_eq!(p.segments[0].nodes.len(), 3);
    }

    #[test]
    fn name_is_order_independent_and_valid() {
        let t1 = Topology { nodes: vec![n("a", NodeKind::Docker), n("b", NodeKind::Docker)], edges: vec![e("a", "b")] };
        let t2 = Topology { nodes: vec![n("b", NodeKind::Docker), n("a", NodeKind::Docker)], edges: vec![e("b", "a")] };
        let (p1, p2) = (plan(&t1), plan(&t2));
        assert_eq!(p1.segments[0].name, p2.segments[0].name);
        assert!(crate::validate::network_name(&p1.segments[0].name).is_ok());
    }

    #[test]
    fn mixed_segment_warns() {
        let t = Topology { nodes: vec![n("a", NodeKind::Vbox), n("b", NodeKind::Docker)], edges: vec![e("a", "b")] };
        assert_eq!(plan(&t).warnings.len(), 1);
    }
}
