import type { MachineData, PowerState, Snapshot } from '../types';

export const EMPTY_SNAPSHOT: Snapshot = { states: {}, vboxOk: false, dockerOk: false };

/** Estado efectivo de un nodo a partir del último snapshot del backend. */
export function resolveState(data: Pick<MachineData, 'kind' | 'target'>, snap: Snapshot): PowerState {
  const known = snap.states[`${data.kind}:${data.target}`];
  if (known) return known;
  const toolOk = data.kind === 'vbox' ? snap.vboxOk : snap.dockerOk;
  return toolOk ? 'missing' : 'unknown';
}

export const STATE_LABEL: Record<PowerState, string> = {
  running: 'En ejecución',
  stopped: 'Detenida',
  paused: 'Pausada',
  missing: 'Sin crear',
  unknown: 'Sin conexión',
};
