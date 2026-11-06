/**
 * Puente con el backend. En Tauri usa `invoke`/eventos; en un navegador normal
 * (npm run dev) cae a un simulador para poder desarrollar la interfaz sin VirtualBox ni Docker.
 */
import type {
  Action,
  ApplyReport,
  DockerStartResult,
  GuestAdditionsPrep,
  GuestConfigRequest,
  GuestProfile,
  GuestReport,
  Inventory,
  MachineKind,
  NodeProbe,
  ProbeNode,
  Snapshot,
  ToolsInfo,
  TopologyPayload,
} from '../types';

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export const STATUS_EVENT = 'vmlab:status';

async function invoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args);
}

// ───────────────────────── API real ─────────────────────────

const real = {
  detectTools: () => invoke<ToolsInfo>('detect_tools'),
  listMachines: () => invoke<Inventory>('list_machines'),
  startDocker: () => invoke<DockerStartResult>('docker_start'),
  refreshStatus: () => invoke<Snapshot>('refresh_status'),
  machineAction: (kind: MachineKind, target: string, image: string | undefined, action: Action) =>
    invoke<void>('machine_action', { kind, target, image: image ?? null, action }),
  applyTopology: (topology: TopologyPayload) => invoke<ApplyReport>('apply_topology', { topology }),
  networkProbe: (nodes: ProbeNode[]) => invoke<NodeProbe[]>('network_probe', { nodes }),
  guestProbe: (target: string, username: string, password: string) =>
    invoke<GuestProfile>('guest_probe', { target, username, password }),
  guestApply: (request: GuestConfigRequest) => invoke<GuestReport>('guest_apply', { request }),
  prepareGuestAdditions: (target: string) =>
    invoke<GuestAdditionsPrep>('guest_additions_prepare', { target }),
  saveWorkspace: (json: string) => invoke<void>('save_workspace', { json }),
  loadWorkspace: () => invoke<string | null>('load_workspace'),
  async onStatus(cb: (s: Snapshot) => void): Promise<() => void> {
    const { listen } = await import('@tauri-apps/api/event');
    return listen<Snapshot>(STATUS_EVENT, (e) => cb(e.payload));
  },
};

// ───────────────────────── Simulador (solo navegador) ─────────────────────────

const mockStates: Snapshot['states'] = {
  'vbox:11111111-aaaa': 'running',
  'vbox:22222222-bbbb': 'stopped',
  'docker:web': 'running',
  'docker:db': 'stopped',
};
const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));
const mockKey = (kind: MachineKind, target: string) => `${kind}:${target}`;

const mock: typeof real = {
  detectTools: async () => ({ vboxmanage: 'C:/Program Files/Oracle/VirtualBox/VBoxManage.exe', docker: 'docker' }),
  listMachines: async () => ({
    vbox: [
      { kind: 'vbox', target: '11111111-aaaa', label: 'Kali Linux', state: mockStates['vbox:11111111-aaaa'], image: null, guestAdditions: { version: '7.2.22', known: true } },
      { kind: 'vbox', target: '22222222-bbbb', label: 'Windows Server 2022', state: mockStates['vbox:22222222-bbbb'], image: null, guestAdditions: { version: null, known: true } },
    ],
    docker: [
      { kind: 'docker', target: 'web', label: 'web', state: mockStates['docker:web'], image: 'nginx:latest', guestAdditions: null },
      { kind: 'docker', target: 'db', label: 'db', state: mockStates['docker:db'], image: 'postgres:16', guestAdditions: null },
    ],
    vboxError: null,
    dockerError: null,
    dockerEngineOff: false,
    dockerErrorDetail: null,
  }),
  startDocker: async () => {
    await delay(900);
    return {
      alreadyRunning: false,
      launched: true,
      engineReady: true,
      waitedSecs: 3,
      desktop: 'C:/Program Files/Docker/Docker/Docker Desktop.exe',
      message: 'Docker Desktop arrancó en segundo plano y el motor está listo.',
    };
  },
  refreshStatus: async () => ({ states: { ...mockStates }, vboxOk: true, dockerOk: true }),
  machineAction: async (kind, target, _image, action) => {
    await delay(700);
    const next = action === 'stop' || action === 'forceStop' ? 'stopped' : action === 'pause' ? 'paused' : 'running';
    mockStates[mockKey(kind, target)] = next;
  },
  applyTopology: async (t) => ({
    networks: t.edges.length ? ['vmlab-demo0001'] : [],
    applied: t.nodes.length,
    errors: [],
    warnings: [],
  }),
  networkProbe: async (nodes) => {
    await delay(250);
    return nodes.map((n) => {
      const host = n.target === '11111111-aaaa' || n.target === 'web' ? 11 : 12;
      return n.kind === 'vbox'
        ? {
            kind: n.kind,
            target: n.target,
            segment: 'intnet:vmlab-demo0001',
            ipv4: `10.77.9.${host}`,
            note: null,
          }
        : {
            kind: n.kind,
            target: n.target,
            segment: 'docker:vmlab-demo0001',
            ipv4: `172.18.0.${host}`,
            note: null,
          };
    });
  },
  guestProbe: async (_target, _username, _password) => {
    await delay(500);
    return {
      os: 'Ubuntu 22.04.4 LTS',
      osId: 'ubuntu',
      kernel: 'Linux 6.5.0 x86_64',
      isLinux: true,
      guestAdditions: true,
      isRoot: false,
      sudo: true,
      systems: [
        {
          system: 'netplan',
          label: 'Netplan',
          configPath: '/etc/netplan/90-vmlab.yaml',
          active: true,
          recommended: true,
          kind: 'client',
        },
        {
          system: 'networkManager',
          label: 'NetworkManager',
          configPath: '/etc/NetworkManager/system-connections/vmlab-eth0.nmconnection',
          active: true,
          recommended: false,
          kind: 'client',
        },
        {
          system: 'dhcpd',
          label: 'Servidor DHCP ISC (dhcpd.conf)',
          configPath: '/etc/dhcp/dhcpd.conf',
          active: false,
          recommended: true,
          kind: 'server',
        },
      ],
      interfaces: [
        { name: 'eth0', mac: '08:00:27:aa:bb:cc', state: 'UP', ipv4: '10.0.2.15/24' },
        { name: 'eth1', mac: '08:00:27:dd:ee:ff', state: 'DOWN', ipv4: null },
      ],
    };
  },
  guestApply: async (request) => {
    await delay(900);
    return {
      system: request.system === 'auto' ? 'netplan' : request.system,
      files: request.ipMode === 'server' ? ['/etc/dhcp/dhcpd.conf'] : [`/etc/netplan/90-vmlab.yaml`],
      messages: [`Configuración escrita para ${request.interface ?? 'la interfaz'}`],
      warnings: [],
    };
  },
  prepareGuestAdditions: async (target) => {
    await delay(700);
    return {
      iso: 'C:/Program Files/Oracle/VirtualBox/VBoxGuestAdditions.iso',
      attached: true,
      started: true,
      alreadyRunning: false,
      os: target === '22222222-bbbb' ? 'windows' : 'linux',
      warnings: [],
    };
  },
  saveWorkspace: async (json) => {
    try {
      localStorage.setItem('vmlab.workspace', json);
      localStorage.removeItem('lablab.workspace');
    } catch { /* sin almacenamiento */ }
  },
  loadWorkspace: async () => {
    try {
      const current = localStorage.getItem('vmlab.workspace');
      if (current !== null) return current;
      const old = localStorage.getItem('lablab.workspace');
      if (old !== null) {
        localStorage.setItem('vmlab.workspace', old);
        localStorage.removeItem('lablab.workspace');
      }
      return old;
    } catch { return null; }
  },
  async onStatus(cb) {
    const t = setInterval(() => cb({ states: { ...mockStates }, vboxOk: true, dockerOk: true }), 1500);
    return () => clearInterval(t);
  },
};

export const ipc = isTauri ? real : mock;

// ───────────────────────── Ventana ─────────────────────────

export const windowControls = {
  async minimize() { if (isTauri) (await import('@tauri-apps/api/window')).getCurrentWindow().minimize(); },
  async toggleMaximize() { if (isTauri) (await import('@tauri-apps/api/window')).getCurrentWindow().toggleMaximize(); },
  async close() { if (isTauri) (await import('@tauri-apps/api/window')).getCurrentWindow().close(); },
  async isMaximized(): Promise<boolean> {
    return isTauri ? (await import('@tauri-apps/api/window')).getCurrentWindow().isMaximized() : false;
  },
  async onResized(cb: () => void): Promise<() => void> {
    if (!isTauri) return () => {};
    return (await import('@tauri-apps/api/window')).getCurrentWindow().onResized(cb);
  },
};
