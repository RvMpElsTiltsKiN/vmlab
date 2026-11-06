//! Tipos que cruzan el puente IPC (JSON en camelCase).
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    Vbox,
    Docker,
}

impl NodeKind {
    pub fn key(self, target: &str) -> String {
        match self {
            NodeKind::Vbox => format!("vbox:{target}"),
            NodeKind::Docker => format!("docker:{target}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerState {
    Running,
    Stopped,
    Paused,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Start,
    StartGui,
    Stop,
    ForceStop,
    Restart,
    Pause,
    Resume,
}

/// Modo de red de un adaptador de VirtualBox.
///
/// `Intnet` es el modo que gestiona VMLab: el adaptador se conecta a la red interna
/// del segmento del lienzo. El resto de modos los elige el usuario en el panel derecho.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NetMode {
    Nat,
    Bridged,
    #[default]
    Intnet,
    Hostonly,
    Natnetwork,
    None,
}

impl NetMode {
    pub fn label(self) -> &'static str {
        match self {
            NetMode::Nat => "NAT",
            NetMode::Bridged => "adaptador puente",
            NetMode::Intnet => "red interna",
            NetMode::Hostonly => "solo anfitrión",
            NetMode::Natnetwork => "red NAT",
            NetMode::None => "sin conectar",
        }
    }
}

/// Configuración de un adaptador de VirtualBox elegida en el panel derecho.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeNet {
    #[serde(default)]
    pub mode: NetMode,
    /// Número de adaptador (1..=4). VMLab usa el 2 por convención.
    #[serde(default = "default_adapter")]
    pub adapter: u8,
    /// Nombre de la red interna, red NAT o adaptador solo-anfitrión.
    #[serde(default)]
    pub name: Option<String>,
    /// Interfaz física del host para el modo puente (p. ej. `Wi-Fi` o `eth0`).
    #[serde(default)]
    pub bridge: Option<String>,
}

pub fn default_adapter() -> u8 {
    2
}

impl Default for NodeNet {
    fn default() -> Self {
        Self { mode: NetMode::Intnet, adapter: default_adapter(), name: None, bridge: None }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeSpec {
    pub id: String,
    pub kind: NodeKind,
    /// VirtualBox: UUID o nombre de la VM. Docker: nombre del contenedor.
    pub target: String,
    /// Solo Docker: imagen para crear el contenedor si todavía no existe.
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    /// Solo VirtualBox: modo y parámetros del adaptador de red.
    #[serde(default)]
    pub net: Option<NodeNet>,
}

impl NodeSpec {
    pub fn display(&self) -> &str {
        self.label.as_deref().filter(|l| !l.is_empty()).unwrap_or(&self.target)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Topology {
    pub nodes: Vec<NodeSpec>,
    pub edges: Vec<Edge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineInfo {
    pub kind: NodeKind,
    pub target: String,
    pub label: String,
    pub state: PowerState,
    pub image: Option<String>,
    /// Solo VirtualBox: estado de las Guest Additions.
    pub guest_additions: Option<GuestAdditionsInfo>,
}

/// Guest Additions detectadas por la propiedad persistente que escribe VirtualBox
/// (`/VirtualBox/GuestAdd/Version`). Funciona también con la VM apagada, siempre que
/// las Guest Additions se hayan ejecutado al menos una vez.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuestAdditionsInfo {
    /// Versión instalada, o `None` si la VM no tiene Guest Additions.
    pub version: Option<String>,
    /// `false` si VirtualBox no pudo consultar (por ejemplo, VM bloqueada por otra sesión).
    pub known: bool,
}

/// Resultado de preparar la instalación guiada de las Guest Additions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuestAdditionsPrep {
    /// Ruta de la ISO de Guest Additions usada.
    pub iso: String,
    /// La ISO quedó montada en una unidad óptica de la VM.
    pub attached: bool,
    /// Se abrió la VM con ventana para poder ejecutar el instalador.
    pub started: bool,
    /// La VM ya estaba en marcha: se dejó como estaba.
    pub already_running: bool,
    /// `windows`, `linux` u `other`, para mostrar las instrucciones correctas.
    pub os: String,
    pub warnings: Vec<String>,
}


#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub vbox: Vec<MachineInfo>,
    pub docker: Vec<MachineInfo>,
    pub vbox_error: Option<String>,
    /// Mensaje listo para mostrar (en español) cuando falla Docker.
    pub docker_error: Option<String>,
    /// `true` si el fallo es que el motor no está escuchando.
    pub docker_engine_off: bool,
    /// Error técnico original, para el desplegable de detalles.
    pub docker_error_detail: Option<String>,
}

/// Resultado de arrancar Docker Desktop en segundo plano.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DockerStartInfo {
    /// Docker Desktop ya estaba en marcha antes de la llamada.
    pub already_running: bool,
    /// VMLab lanzó el proceso en esta llamada.
    pub launched: bool,
    /// El motor responde y acepta comandos.
    pub engine_ready: bool,
    pub waited_secs: u64,
    /// Ruta del ejecutable usado.
    pub desktop: String,
    /// Resumen en español para mostrar tal cual.
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Claves `vbox:<uuid|nombre>` y `docker:<nombre>`.
    pub states: HashMap<String, PowerState>,
    pub vbox_ok: bool,
    pub docker_ok: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyReport {
    pub networks: Vec<String>,
    pub applied: usize,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

// ───────── Sonda de comunicación del lienzo ─────────

/// Nodo del lienzo cuya conexión se quiere comprobar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeNode {
    pub kind: NodeKind,
    pub target: String,
    /// Solo VirtualBox: adaptador que gestiona VMLab (2 por defecto).
    pub adapter: Option<u8>,
}

/// Enlace observado dentro de una máquina o contenedor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkInfo {
    /// Red concreta: `intnet:<nombre>`, `bridged:<interfaz>`, `docker:<red>`…
    pub segment: Option<String>,
    pub ipv4: Option<String>,
    /// Motivo cuando todavía no se puede afirmar que haya comunicación.
    pub note: Option<String>,
}

/// Resultado de la sonda para un nodo del lienzo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeProbe {
    pub kind: NodeKind,
    pub target: String,
    pub segment: Option<String>,
    pub ipv4: Option<String>,
    pub note: Option<String>,
}

// ───────── Configuración dentro de una VM Linux (Guest Additions) ─────────

/// Gestor de red Linux que VMLab sabe escribir.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum GuestSystem {
    /// Elegir automáticamente el gestor recomendado por la detección.
    #[default]
    Auto,
    Netplan,
    NetworkManager,
    SystemdNetworkd,
    Ifupdown,
    Wicked,
    Dhcpcd,
    Connman,
    NetworkScripts,
    /// Servidor DHCP ISC (`dhcpd.conf`), no cliente.
    Dhcpd,
}

impl GuestSystem {
    pub fn label(self) -> &'static str {
        match self {
            GuestSystem::Auto => "Automático",
            GuestSystem::Netplan => "Netplan",
            GuestSystem::NetworkManager => "NetworkManager",
            GuestSystem::SystemdNetworkd => "systemd-networkd",
            GuestSystem::Ifupdown => "ifupdown (/etc/network/interfaces)",
            GuestSystem::Wicked => "wicked (SUSE)",
            GuestSystem::Dhcpcd => "dhcpcd",
            GuestSystem::Connman => "ConnMan",
            GuestSystem::NetworkScripts => "network-scripts (RHEL/CentOS)",
            GuestSystem::Dhcpd => "Servidor DHCP ISC (dhcpd.conf)",
        }
    }

    pub fn kind(self) -> GuestSystemKind {
        match self {
            GuestSystem::Dhcpd => GuestSystemKind::Server,
            _ => GuestSystemKind::Client,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuestSystemKind {
    Client,
    Server,
}

/// Resultado de una detección dentro de la VM, con la ruta exacta que usaría VMLab.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuestSystemInfo {
    pub system: GuestSystem,
    pub label: String,
    pub config_path: String,
    pub active: bool,
    pub recommended: bool,
    pub kind: GuestSystemKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuestInterface {
    pub name: String,
    pub mac: Option<String>,
    pub state: Option<String>,
    pub ipv4: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuestProfile {
    pub os: String,
    pub os_id: String,
    pub kernel: String,
    pub is_linux: bool,
    pub guest_additions: bool,
    pub is_root: bool,
    pub sudo: bool,
    pub systems: Vec<GuestSystemInfo>,
    pub interfaces: Vec<GuestInterface>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GuestIpMode {
    Dhcp,
    Static,
    /// La VM actúa como servidor DHCP de la red (escribe `dhcpd.conf`).
    Server,
}

/// Petición cerrada: todos los campos se validan antes de tocar la VM.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuestConfigRequest {
    pub target: String,
    pub username: String,
    /// Solo vive en memoria durante la operación; nunca se guarda en el lienzo.
    pub password: String,
    #[serde(default)]
    pub system: GuestSystem,
    pub interface: String,
    pub ip_mode: GuestIpMode,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub prefix: Option<u8>,
    #[serde(default)]
    pub gateway: Option<String>,
    #[serde(default)]
    pub dns: Vec<String>,
    /// Solo para `ipMode = server`: subred en CIDR (`10.77.5.0/24`).
    #[serde(default)]
    pub subnet: Option<String>,
    #[serde(default)]
    pub range_start: Option<String>,
    #[serde(default)]
    pub range_end: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GuestReport {
    pub system: GuestSystem,
    pub files: Vec<String>,
    pub messages: Vec<String>,
    pub warnings: Vec<String>,
}
