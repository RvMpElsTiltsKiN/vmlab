import type { MachineNode, NodeProbe, Snapshot } from '../types';
import { resolveState } from './status';

/** Clave de sonda: `vbox:<uuid>` o `docker:<nombre>`. */
export const probeKey = (kind: string, target: string) => `${kind}:${target}`;

/** VMLab usa /24 en todos sus segmentos (red interna y red de Docker). */
export function sameSubnet(a: string, b: string): boolean {
  const pa = a.split('.');
  const pb = b.split('.');
  return pa.length === 4 && pb.length === 4 && pa.slice(0, 3).join('.') === pb.slice(0, 3).join('.');
}

function probeOf(node: MachineNode, probes: Record<string, NodeProbe>): NodeProbe | undefined {
  return probes[probeKey(node.data.kind, node.data.target)];
}

/** ¿Los dos extremos del cable se ven en la misma red y tienen IPv4? */
export function linkLive(
  source: MachineNode,
  target: MachineNode,
  probes: Record<string, NodeProbe>,
  snapshot: Snapshot,
): boolean {
  if (
    resolveState(source.data, snapshot) !== 'running' ||
    resolveState(target.data, snapshot) !== 'running'
  ) {
    return false;
  }
  const a = probeOf(source, probes);
  const b = probeOf(target, probes);
  return !!(
    a?.segment &&
    b?.segment &&
    a.segment === b.segment &&
    a.ipv4 &&
    b.ipv4 &&
    sameSubnet(a.ipv4, b.ipv4)
  );
}

/** ¿Todo el segmento del lienzo tiene comunicación? */
export function groupLive(
  members: MachineNode[],
  probes: Record<string, NodeProbe>,
  snapshot: Snapshot,
): boolean {
  if (members.length < 2) return false;
  if (members.some((n) => resolveState(n.data, snapshot) !== 'running')) return false;
  const links = members.map((n) => probeOf(n, probes));
  const first = links[0];
  if (!first?.segment || !first.ipv4) return false;
  const segment = first.segment;
  const ipv4 = first.ipv4;
  return links.every((l) => {
    if (!l?.segment || !l.ipv4) return false;
    return l.segment === segment && sameSubnet(l.ipv4, ipv4);
  });
}
