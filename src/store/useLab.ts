import { create } from 'zustand';
import {
  applyEdgeChanges,
  applyNodeChanges,
  type Connection,
  type Edge,
  type EdgeChange,
  type NodeChange,
} from '@xyflow/react';
import { probeKey } from '../lib/connectivity';
import { ipc } from '../lib/ipc';
import { EMPTY_SNAPSHOT, resolveState } from '../lib/status';
import { toPayload } from '../lib/topology';
import type {
  Action,
  ApplyReport,
  Inventory,
  MachineData,
  MachineNode,
  NodeProbe,
  PaletteItem,
  Snapshot,
  ToolsInfo,
} from '../types';

export type Theme = 'dark' | 'light';
export interface Toast {
  id: number;
  kind: 'ok' | 'info' | 'error';
  text: string;
}

interface LabState {
  nodes: MachineNode[];
  edges: Edge[];
  snapshot: Snapshot;
  tools: ToolsInfo | null;
  inventory: Inventory | null;
  busy: Record<string, boolean>;
  applying: boolean;
  /** Docker Desktop se está arrancando en segundo plano. */
  dockerStarting: boolean;
  report: ApplyReport | null;
  /** Último enlace observado por nodo (`kind:target`). */
  probes: Record<string, NodeProbe>;
  theme: Theme;
  toasts: Toast[];
  /** Un nodo del lienzo se está arrastrando sobre el inventario. */
  overSidebar: boolean;
  /** El lienzo está bloqueado: no se desplaza ni hace zoom, pero los nodos siguen arrastrándose. */
  canvasLocked: boolean;

  init: () => Promise<void>;
  refreshInventory: () => Promise<void>;
  startDocker: () => Promise<void>;
  onNodesChange: (c: NodeChange<MachineNode>[]) => void;
  onEdgesChange: (c: EdgeChange[]) => void;
  onConnect: (c: Connection) => void;
  addMachine: (item: PaletteItem, position?: { x: number; y: number }) => void;
  updateNode: (id: string, patch: Partial<MachineData>) => void;
  removeNode: (id: string) => void;
  act: (id: string, action: Action) => Promise<void>;
  actAll: (action: 'start' | 'stop') => Promise<void>;
  applyNetwork: () => Promise<void>;
  refreshProbes: () => Promise<void>;
  toggleTheme: () => void;
  toggleCanvasLock: () => void;
  setOverSidebar: (over: boolean) => void;
  toast: (kind: Toast['kind'], text: string) => void;
  dismissToast: (id: number) => void;
}

const THEME_KEY = 'vmlab.theme';
const CANVAS_LOCK_KEY = 'vmlab.canvasLocked';
const LEGACY_THEME_KEY = 'lablab.theme';
const LEGACY_CANVAS_LOCK_KEY = 'lablab.canvasLocked';
let toastSeq = 0;
let started = false;
let probeBusy = false;
let saveTimer: ReturnType<typeof setTimeout> | undefined;

/** Migra una clave del nombre antiguo (`lablab.*`) al nuevo (`vmlab.*`). */
function storedValue(key: string, legacy: string): string | null {
  try {
    const current = localStorage.getItem(key);
    if (current !== null) return current;
    const old = localStorage.getItem(legacy);
    if (old === null) return null;
    localStorage.setItem(key, old);
    localStorage.removeItem(legacy);
    return old;
  } catch {
    return null;
  }
}

function initialTheme(): Theme {
  const saved = storedValue(THEME_KEY, LEGACY_THEME_KEY);
  if (saved === 'light' || saved === 'dark') return saved;
  return window.matchMedia?.('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
}

function initialCanvasLocked(): boolean {
  return storedValue(CANVAS_LOCK_KEY, LEGACY_CANVAS_LOCK_KEY) === '1';
}

function applyTheme(t: Theme) {
  document.documentElement.dataset.theme = t;
  try { localStorage.setItem(THEME_KEY, t); } catch { /* sin almacenamiento */ }
}

const errText = (e: unknown) => (typeof e === 'string' ? e : e instanceof Error ? e.message : JSON.stringify(e));

/** Errores de la CLI cuando Docker Desktop está cerrado o su motor aún no escucha. */
const DOCKER_DOWN_RE = /failed to connect to the docker api|cannot connect to the docker daemon|is the docker daemon running|error during connect/i;

const dockerDown = (e: unknown) => DOCKER_DOWN_RE.test(errText(e));
const newId = () => `n_${Date.now().toString(36)}${Math.random().toString(36).slice(2, 6)}`;

export const useLab = create<LabState>()((set, get) => ({
  nodes: [],
  edges: [],
  snapshot: EMPTY_SNAPSHOT,
  tools: null,
  inventory: null,
  busy: {},
  applying: false,
  dockerStarting: false,
  report: null,
  probes: {},
  theme: initialTheme(),
  toasts: [],
  overSidebar: false,
  canvasLocked: initialCanvasLocked(),

  async init() {
    if (started) return; // React.StrictMode monta dos veces en desarrollo
    started = true;
    applyTheme(get().theme);

    try {
      const raw = await ipc.loadWorkspace();
      if (raw) {
        const ws = JSON.parse(raw) as { nodes?: MachineNode[]; edges?: Edge[] };
        if (Array.isArray(ws.nodes) && Array.isArray(ws.edges)) set({ nodes: ws.nodes, edges: ws.edges });
      }
    } catch (e) {
      get().toast('error', `No se pudo cargar el espacio de trabajo: ${errText(e)}`);
    }

    // Autoguardado (con debounce) solo cuando cambia el contenido real del lienzo.
    useLab.subscribe((s, prev) => {
      if (s.nodes === prev.nodes && s.edges === prev.edges) return;
      clearTimeout(saveTimer);
      saveTimer = setTimeout(() => {
        const { nodes, edges } = useLab.getState();
        const slim = nodes.map(({ id, type, position, data }) => ({ id, type, position, data }));
        ipc.saveWorkspace(JSON.stringify({ version: 1, nodes: slim, edges })).catch(() => {});
      }, 600);
    });

    try { set({ tools: await ipc.detectTools() }); } catch { /* se mostrará como no detectado */ }
    await get().refreshInventory();
    try { set({ snapshot: await ipc.refreshStatus() }); } catch { /* el monitor lo reintenta */ }
    await ipc.onStatus((snapshot) => set({ snapshot }));
  },

  async refreshInventory() {
    try {
      set({ inventory: await ipc.listMachines() });
    } catch (e) {
      get().toast('error', errText(e));
    }
  },

  async startDocker() {
    if (get().dockerStarting) return;
    set({ dockerStarting: true });
    get().toast('info', 'Iniciando Docker Desktop en segundo plano… puede tardar un minuto.');
    try {
      const result = await ipc.startDocker();
      get().toast(result.engineReady ? 'ok' : 'error', result.message);
      try { set({ tools: await ipc.detectTools() }); } catch { /* se mantiene lo detectado */ }
      await get().refreshInventory();
      try { set({ snapshot: await ipc.refreshStatus() }); } catch { /* el monitor lo reintenta */ }
      void get().refreshProbes();
    } catch (e) {
      get().toast('error', `No se pudo iniciar Docker: ${errText(e)}`);
    } finally {
      set({ dockerStarting: false });
    }
  },

  onNodesChange: (c) => set((s) => ({ nodes: applyNodeChanges(c, s.nodes) })),
  onEdgesChange: (c) => set((s) => ({ edges: applyEdgeChanges(c, s.edges) })),

  onConnect(c) {
    if (!c.source || !c.target || c.source === c.target) return;
    const exists = get().edges.some(
      (e) => (e.source === c.source && e.target === c.target) || (e.source === c.target && e.target === c.source),
    );
    if (exists) return;
    const edge: Edge = {
      id: `e_${c.source}_${c.target}`,
      source: c.source,
      target: c.target,
      sourceHandle: c.sourceHandle,
      targetHandle: c.targetHandle,
    };
    set((s) => ({ edges: [...s.edges, edge] }));
  },

  addMachine(item, position) {
    const { nodes, toast } = get();
    if (nodes.some((n) => n.data.kind === item.kind && n.data.target === item.target)) {
      toast('info', `«${item.label}» ya está en el lienzo`);
      return;
    }
    const n = nodes.length;
    const node: MachineNode = {
      id: newId(),
      type: 'machine',
      position: position ?? { x: 80 + (n % 4) * 230, y: 80 + Math.floor(n / 4) * 150 },
      data: { kind: item.kind, label: item.label, target: item.target, image: item.image },
    };
    set((s) => ({ nodes: [...s.nodes, node] }));
  },

  updateNode: (id, patch) =>
    set((s) => ({ nodes: s.nodes.map((n) => (n.id === id ? { ...n, data: { ...n.data, ...patch } } : n)) })),

  removeNode: (id) =>
    set((s) => ({
      nodes: s.nodes.filter((n) => n.id !== id),
      edges: s.edges.filter((e) => e.source !== id && e.target !== id),
    })),

  async act(id, action) {
    const node = get().nodes.find((n) => n.id === id);
    if (!node || get().busy[id]) return;
    set((s) => ({ busy: { ...s.busy, [id]: true } }));
    try {
      await ipc.machineAction(node.data.kind, node.data.target, node.data.image, action);
    } catch (e) {
      if (node.data.kind === 'docker' && dockerDown(e)) {
        get().toast('error', `${node.data.label}: Docker Desktop no está en marcha. Usa «Iniciar Docker» en el panel lateral.`);
        // Trae `dockerEngineOff` para que aparezca el botón de arranque.
        void get().refreshInventory();
      } else {
        get().toast('error', `${node.data.label}: ${errText(e)}`);
      }
    } finally {
      set((s) => ({ busy: { ...s.busy, [id]: false } }));
      try { set({ snapshot: await ipc.refreshStatus() }); } catch { /* el monitor lo reintenta */ }
      void get().refreshProbes();
    }
  },

  async actAll(action) {
    await Promise.allSettled(get().nodes.map((n) => get().act(n.id, action)));
  },

  async applyNetwork() {
    if (get().applying) return;
    // Mientras se reconfigura, los cables vuelven a rojo hasta confirmar la nueva red.
    set({ applying: true, probes: {} });
    try {
      const { nodes, edges } = get();
      const report = await ipc.applyTopology(toPayload(nodes, edges));
      set({ report });
      if (report.errors.length) get().toast('error', `${report.errors.length} error(es) al aplicar la red`);
      else get().toast('ok', report.networks.length ? `Red aplicada (${report.applied} nodos)` : 'Sin conexiones: nodos aislados');
      void get().refreshProbes();
    } catch (e) {
      get().toast('error', errText(e));
    } finally {
      set({ applying: false });
    }
  },

  async refreshProbes() {
    const { nodes, edges, snapshot } = get();
    const linked = new Set(edges.flatMap((e) => [e.source, e.target]));
    const targets = nodes.filter(
      (n) => linked.has(n.id) && resolveState(n.data, snapshot) === 'running',
    );
    if (targets.length === 0) {
      if (Object.keys(get().probes).length > 0) set({ probes: {} });
      return;
    }
    if (probeBusy) return;
    probeBusy = true;
    try {
      const list = await ipc.networkProbe(
        targets.map((n) => ({
          kind: n.data.kind,
          target: n.data.target,
          adapter: n.data.kind === 'vbox' ? (n.data.net?.adapter ?? 2) : null,
        })),
      );
      const live = new Set(get().nodes.map((n) => probeKey(n.data.kind, n.data.target)));
      const probes: Record<string, NodeProbe> = {};
      for (const p of list) {
        const key = probeKey(p.kind, p.target);
        if (live.has(key)) probes[key] = p;
      }
      set({ probes });
    } catch {
      // Un fallo puntual (VirtualBox bloqueado, Docker arrancando…) conserva la última lectura.
    } finally {
      probeBusy = false;
    }
  },

  toggleTheme() {
    const theme: Theme = get().theme === 'dark' ? 'light' : 'dark';
    applyTheme(theme);
    set({ theme });
  },

  toggleCanvasLock() {
    const canvasLocked = !get().canvasLocked;
    try { localStorage.setItem(CANVAS_LOCK_KEY, canvasLocked ? '1' : '0'); } catch { /* sin almacenamiento */ }
    set({ canvasLocked });
  },

  setOverSidebar: (overSidebar) => set({ overSidebar }),

  toast(kind, text) {
    const id = ++toastSeq;
    set((s) => ({ toasts: [...s.toasts.slice(-3), { id, kind, text }] }));
    setTimeout(() => get().dismissToast(id), kind === 'error' ? 8000 : 3500);
  },
  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));
