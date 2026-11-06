import type { Node } from '@xyflow/react';

export type MachineKind = 'vbox' | 'docker';
export type PowerState = 'running' | 'stopped' | 'paused' | 'missing' | 'unknown';
export type Action = 'start' | 'startGui' | 'stop' | 'forceStop' | 'restart' | 'pause' | 'resume';

/** Modos de adaptador que entiende VirtualBox (los que ofrece VMLab). */
export type NetMode = 'nat' | 'bridged' | 'intnet' | 'hostonly' | 'natnetwork' | 'none';

export interface NodeNet {
  mode: NetMode;
  /** Adaptador 1..4. VMLab usa el 2 por convención. */
  adapter: number;
  /** Red interna, red NAT o adaptador solo-anfitrión. */
  name?: string;
  /** Interfaz física del host para el modo puente. */
  bridge?: string;
}

/** Datos que guarda cada nodo del lienzo. */
export interface MachineData extends Record<string, unknown> {
  kind: MachineKind;
  label: string;
  /** VirtualBox: UUID de la VM. Docker: nombre del contenedor. */
  target: string;
  /** Solo Docker: imagen con la que crear el contenedor si no existe. */
  image?: string;
  /** Solo VirtualBox: modo de red elegido en el panel derecho. */
  net?: NodeNet;
  /** Solo VirtualBox/Linux: usuario para Guest Additions (la contraseña nunca se guarda). */
  guestUser?: string;
  guestSystem?: GuestSystem;
  guestIface?: string;
  guestMode?: GuestIpMode;
  guestAddress?: string;
  guestPrefix?: number;
  guestGateway?: string;
  guestDns?: string;
  guestSubnet?: string;
  guestRangeStart?: string;
  guestRangeEnd?: string;
}
export type MachineNode = Node<MachineData, 'machine'>;

// ---- Contrato con el backend Rust (camelCase, ver crates/vmlab-core/src/model.rs) ----

export interface ToolsInfo {
  vboxmanage: string | null;
  docker: string | null;
}

/** Resultado de arrancar Docker Desktop en segundo plano. */
export interface DockerStartResult {
  alreadyRunning: boolean;
  launched: boolean;
  engineReady: boolean;
  waitedSecs: number;
  desktop: string;
  message: string;
}

export interface MachineInfo {
  kind: MachineKind;
  target: string;
  label: string;
  state: PowerState;
  image: string | null;
  /** Solo VirtualBox: Guest Additions detectadas (la propiedad persiste con la VM apagada). */
  guestAdditions: GuestAdditionsInfo | null;
}

export interface GuestAdditionsInfo {
  /** Versión instalada, o null si no se detectan. */
  version: string | null;
  /** false si VirtualBox no pudo consultar (VM bloqueada, etc.). */
  known: boolean;
}

export interface GuestAdditionsPrep {
  iso: string;
  attached: boolean;
  started: boolean;
  alreadyRunning: boolean;
  /** windows | linux | other */
  os: 'windows' | 'linux' | 'other';
  warnings: string[];
}

export interface Inventory {
  vbox: MachineInfo[];
  docker: MachineInfo[];
  vboxError: string | null;
  /** Mensaje listo para mostrar cuando falla Docker. */
  dockerError: string | null;
  /** El motor no está escuchando: se puede arrancar Docker Desktop desde la app. */
  dockerEngineOff: boolean;
  /** Error técnico original, para el desplegable de detalles. */
  dockerErrorDetail: string | null;
}

export interface Snapshot {
  states: Record<string, PowerState>;
  vboxOk: boolean;
  dockerOk: boolean;
}

export interface TopologyPayload {
  nodes: { id: string; kind: MachineKind; target: string; image?: string; label?: string; net?: NodeNet }[];
  edges: { from: string; to: string }[];
}

export interface ApplyReport {
  networks: string[];
  applied: number;
  errors: string[];
  warnings: string[];
}

/** Nodo del lienzo cuya comunicación se quiere comprobar. */
export interface ProbeNode {
  kind: MachineKind;
  target: string;
  /** Solo VirtualBox: adaptador que gestiona VMLab. */
  adapter: number | null;
}

/** Enlace observado dentro de una máquina o contenedor. */
export interface NodeProbe {
  kind: MachineKind;
  target: string;
  /** Red concreta: `intnet:<nombre>`, `hostonly:<interfaz>`, `docker:<red>`… */
  segment: string | null;
  ipv4: string | null;
  /** Motivo cuando todavía no hay comunicación. */
  note: string | null;
}

// ---- Configuración dentro de la VM Linux (Guest Additions) ----

export type GuestSystem =
  | 'auto'
  | 'netplan'
  | 'networkManager'
  | 'systemdNetworkd'
  | 'ifupdown'
  | 'wicked'
  | 'dhcpcd'
  | 'connman'
  | 'networkScripts'
  | 'dhcpd';

export type GuestSystemKind = 'client' | 'server';
export type GuestIpMode = 'dhcp' | 'static' | 'server';

export interface GuestSystemInfo {
  system: GuestSystem;
  label: string;
  /** Ruta exacta que usaría VMLab en esa VM. */
  configPath: string;
  active: boolean;
  recommended: boolean;
  kind: GuestSystemKind;
}

export interface GuestInterface {
  name: string;
  mac: string | null;
  state: string | null;
  ipv4: string | null;
}

export interface GuestProfile {
  os: string;
  osId: string;
  kernel: string;
  isLinux: boolean;
  guestAdditions: boolean;
  isRoot: boolean;
  sudo: boolean;
  systems: GuestSystemInfo[];
  interfaces: GuestInterface[];
}

export interface GuestConfigRequest {
  target: string;
  username: string;
  password: string;
  system: GuestSystem;
  interface: string;
  ipMode: GuestIpMode;
  address?: string;
  prefix?: number;
  gateway?: string;
  dns?: string[];
  subnet?: string;
  rangeStart?: string;
  rangeEnd?: string;
}

export interface GuestReport {
  system: GuestSystem;
  files: string[];
  messages: string[];
  warnings: string[];
}

/** Elemento arrastrable de la paleta. */
export interface PaletteItem {
  kind: MachineKind;
  label: string;
  target: string;
  image?: string;
}
