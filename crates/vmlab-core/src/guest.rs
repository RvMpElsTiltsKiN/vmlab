//! Configuración de red **dentro** de una VM Linux sin abrirla.
//!
//! Mecanismo: Guest Additions + `VBoxManage guestcontrol` con un usuario de la VM.
//! VMLab detecta el gestor de red real (Netplan, NetworkManager, systemd-networkd,
//! ifupdown, wicked, dhcpcd, ConnMan, network-scripts) y escribe el fichero con la
//! ruta exacta que existe en esa VM. También puede escribir `dhcpd.conf` para que la
//! VM actúe como servidor DHCP del laboratorio.
//!
//! Seguridad:
//! - El host nunca usa un shell; el script se copia a la VM y se ejecuta allí.
//! - Todo valor que viene de la UI se valida (interfaces, IPs, prefijos, usuario).
//! - La contraseña se pasa con `--passwordfile` en un fichero temporal del host y
//!   se borra al terminar. Solo se usa `--password` como último recurso si la
//!   versión de VirtualBox no acepta el fichero.
//! - Los ficheros con bloques gestionados (`dhcpcd.conf`, `interfaces`, `dhcpd.conf`)
//!   se editan entre marcadores `# >>> vmlab` / `# <<< vmlab`, sin tocar el resto.
use crate::error::{CoreError, Result};
use crate::model::*;
use crate::runner::{run, Output};
use crate::tools::Tools;
use crate::validate;
use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SHORT: Duration = Duration::from_secs(20);
const COPY: Duration = Duration::from_secs(60);
const GUEST: Duration = Duration::from_secs(180);
const BLOCK_START: &str = "# >>> vmlab";
const BLOCK_END: &str = "# <<< vmlab";

fn now_nanos() -> u128 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
}

fn io_err(e: std::io::Error) -> CoreError {
    CoreError::Io(e.to_string())
}

/// Contraseña en un fichero temporal del host. Se borra en `Drop`, aunque haya error.
struct Auth {
    user: String,
    password: String,
    file: PathBuf,
    inline: bool,
}

impl Auth {
    fn new(user: &str, password: &str) -> Result<Self> {
        let user = validate::guest_user(user)?.to_string();
        let password = validate::guest_password(password)?.to_string();
        let mut file = std::env::temp_dir();
        file.push(format!("vmlab-{}-{}.pwd", std::process::id(), now_nanos()));
        std::fs::write(&file, password.as_bytes()).map_err(io_err)?;
        Ok(Self { user, password, file, inline: false })
    }

    fn args(&self) -> Vec<String> {
        if self.inline {
            vec!["--username".into(), self.user.clone(), "--password".into(), self.password.clone()]
        } else {
            vec![
                "--username".into(),
                self.user.clone(),
                "--passwordfile".into(),
                self.file.to_string_lossy().into_owned(),
            ]
        }
    }

    fn switch_to_inline(&mut self) {
        self.inline = true;
    }
}

impl Drop for Auth {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
    }
}

/// Solo se reintenta con `--password` si el problema es que esta versión no
/// entiende `--passwordfile`. Un fallo de credenciales NO debe reintentarse:
/// expondría la contraseña en la lista de procesos del host.
fn auth_file_error(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    s.contains("--passwordfile")
        || s.contains("unknown option")
        || s.contains("unrecognized option")
        || s.contains("invalid option")
        || s.contains("invalid parameter")
        || s.contains("syntax error")
}

fn explain_guest_error(stderr: &str) -> String {
    let s = stderr.to_lowercase();
    // `VBOX_E_IPRT_ERROR` es genérico: también lo devuelve un fallo de copia de
    // ficheros. No se debe confundir con que falten las Guest Additions.
    if s.contains("guest additions") || s.contains("guest execution service") || s.contains("vbox_e_not_supported") {
        return format!(
            "la VM no tiene las Guest Additions instaladas o el servicio no está en marcha. \
             Instálalas y reinicia la VM. Detalle: {}",
            stderr.trim().chars().take(300).collect::<String>()
        );
    }
    if s.contains("a password is required") || s.contains("sudo:") || s.contains("not in the sudoers") {
        return format!(
            "la VM necesita root o sudo sin contraseña (NOPASSWD) para escribir la configuración. \
             Usa un usuario root o añade el usuario a sudoers. Detalle: {}",
            stderr.trim().chars().take(300).collect::<String>()
        );
    }
    stderr.trim().chars().take(500).collect()
}

/// Ejecuta `VBoxManage guestcontrol …` y devuelve la salida aunque el código sea ≠ 0.
async fn guest_cmd(tools: &Tools, vm: &str, auth: &mut Auth, sub: &str, tail: &[&str], limit: Duration) -> Result<Output> {
    validate::vbox_target(vm)?;
    let bin = tools.vbox()?;
    loop {
        let mut argv: Vec<String> = vec!["guestcontrol".into(), vm.into(), sub.into()];
        argv.extend(auth.args());
        argv.extend(tail.iter().map(|s| (*s).to_string()));
        let strs: Vec<&str> = argv.iter().map(String::as_str).collect();
        let out = run(bin, &strs, limit).await?;
        if out.code != 0 && !auth.inline && auth_file_error(&out.stderr) {
            // Algunas versiones de VirtualBox no aceptan --passwordfile: reintento inline.
            auth.switch_to_inline();
            continue;
        }
        return Ok(out);
    }
}

async fn guest_run(tools: &Tools, vm: &str, auth: &mut Auth, exe: &str, args: &[&str], limit: Duration) -> Result<Output> {
    let mut tail: Vec<&str> = vec!["--wait-stdout", "--wait-stderr", "--exe", exe, "--"];
    tail.extend_from_slice(args);
    let out = guest_cmd(tools, vm, auth, "run", &tail, limit).await?;
    if out.code != 0 && !out.stderr.trim().is_empty() {
        // Se conserva la salida para que el llamador decida; solo se enriquece el mensaje.
        return Ok(Output { stdout: out.stdout, stderr: explain_guest_error(&out.stderr), code: out.code });
    }
    Ok(out)
}

/// `guestcontrol copyto --target-directory <destino> <origen-host>`.
///
/// Algunas versiones de VBoxManage interpretan `--target-directory` como la ruta
/// completa del fichero de destino y devuelven «Destination … already exists and
/// is a directory» si se pasa el directorio. Se prueban las dos formas.
async fn copy_to(tools: &Tools, vm: &str, auth: &mut Auth, host: &Path, guest_path: &str) -> Result<()> {
    let host = host.to_string_lossy().into_owned();
    let guest_dir = guest_path.rsplit_once('/').map(|(dir, _)| dir).unwrap_or("/tmp");
    let mut first_error: Option<Output> = None;

    for target in [guest_path, guest_dir] {
        let out = guest_cmd(
            tools,
            vm,
            auth,
            "copyto",
            &["--target-directory", target, host.as_str()],
            COPY,
        )
        .await?;
        if out.code == 0 {
            return Ok(());
        }
        // El primer intento puede haber dejado un fichero (o un directorio) parcial:
        // se limpia antes de reintentar con la otra variante.
        let _ = guest_run(tools, vm, auth, "/bin/rm", &["-rf", guest_path], SHORT).await;
        if first_error.is_none() {
            first_error = Some(out);
        }
    }

    let out = first_error.expect("se ha intentado al menos una copia");
    Err(CoreError::Failed {
        program: "VBoxManage".into(),
        code: out.code,
        stderr: explain_guest_error(&out.stderr),
    })
}

/// Copia un script al guest, lo ejecuta con `/bin/sh` y lo borra.
async fn run_script(tools: &Tools, vm: &str, auth: &mut Auth, script: &str) -> Result<Output> {
    let name = format!("vmlab-{}-{}.sh", std::process::id(), now_nanos());
    let host = std::env::temp_dir().join(&name);
    std::fs::write(&host, script.as_bytes()).map_err(io_err)?;
    let guest = format!("/tmp/{name}");
    let copied = copy_to(tools, vm, auth, &host, &guest).await;
    let _ = std::fs::remove_file(&host);
    copied?;
    let out = guest_run(tools, vm, auth, "/bin/sh", &[guest.as_str()], GUEST).await;
    // La limpieza no debe ocultar el error real.
    let _ = guest_run(tools, vm, auth, "/bin/rm", &["-f", guest.as_str()], SHORT).await;
    out
}

// ───────────────────────── Detección ─────────────────────────

/// Script fijo de sondeo. No contiene datos de la UI.
const PROBE: &str = r#"#!/bin/sh
echo "VMLAB_BEGIN"
echo "OS:$(cat /etc/os-release 2>/dev/null | tr '\n' '|')"
echo "UNAME:$(uname -srm 2>/dev/null)"
echo "UID:$(id -u 2>/dev/null)"
echo "LINKS:"
ip -o link show 2>/dev/null
echo "ADDRS:"
ip -o -4 addr show 2>/dev/null
echo "ROUTE:"
ip -4 route show default 2>/dev/null
echo "CMDS:"
for c in nmcli netplan wicked wickedd dhcpcd connmanctl ifup ifdown systemctl dhcpd VBoxControl VBoxService sudo; do
  if command -v "$c" >/dev/null 2>&1; then echo "CMD:$c"; fi
done
echo "SVCS:"
for s in NetworkManager systemd-networkd dhcpcd connman wickedd isc-dhcp-server dhcpd network networking; do
  st=$(systemctl is-active "$s" 2>/dev/null)
  echo "SVC:$s:$st"
done
echo "FILES:"
for f in /etc/netplan /etc/netplan/*.yaml /etc/netplan/*.yml /etc/network/interfaces /etc/network/interfaces.d /etc/dhcpcd.conf /etc/systemd/network /etc/NetworkManager/system-connections /etc/sysconfig/network /etc/sysconfig/network/ifcfg-* /etc/sysconfig/network-scripts /etc/sysconfig/network-scripts/ifcfg-* /var/lib/connman /etc/dhcp /etc/dhcp/dhcpd.conf /etc/dhcpd.conf /etc/default/isc-dhcp-server /etc/sysconfig/dhcpd; do
  [ -e "$f" ] && echo "FILE:$f"
done
echo "GUESTADD:$(ls -d /opt/VBoxGuestAdditions* /usr/lib/VBoxGuestAdditions 2>/dev/null | head -n 1)"
echo "VMLAB_END"
"#;

#[derive(Debug, Default)]
struct RawProbe {
    os_release: HashMap<String, String>,
    uname: String,
    uid: Option<u32>,
    links: Vec<(String, Option<String>, Option<String>)>,
    addrs: HashMap<String, String>,
    default_iface: Option<String>,
    cmds: BTreeSet<String>,
    svcs: HashMap<String, String>,
    files: BTreeSet<String>,
    guest_additions: bool,
}

fn parse_os_release(raw: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for kv in raw.split('|') {
        let Some((k, v)) = kv.split_once('=') else { continue };
        let k = k.trim();
        if k.is_empty() || k.starts_with(|c: char| !c.is_ascii_alphabetic()) {
            continue;
        }
        let v = v.trim().trim_matches('"').trim_matches('\'').to_string();
        out.insert(k.to_string(), v);
    }
    out
}

fn parse_link_line(line: &str) -> Option<(String, Option<String>, Option<String>)> {
    let line = line.trim();
    let rest = line.split_once(": ")?.1;
    // `ip -o link` usa `2: eth0: <…>` y a veces `eth0@if3`.
    let name = rest.split_once(": ")?.0.trim();
    let name = name.split('@').next().unwrap_or(name);
    if name.is_empty() || name == "lo" {
        return None;
    }
    let mac = line.split_once("link/ether ").map(|(_, r)| r.split_whitespace().next().unwrap_or("").to_string());
    let state = line.split_once("state ").map(|(_, r)| r.split_whitespace().next().unwrap_or("").to_string());
    Some((name.to_string(), mac, state))
}

fn parse_addr_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    // `ip -o -4 addr` usa `2: eth0    inet 10.0.2.15/24 …` (un solo `: `).
    let rest = line.split_once(": ")?.1;
    let name = rest.split_whitespace().next()?;
    let (_, after) = line.split_once(" inet ")?;
    let addr = after.split_whitespace().next()?.to_string();
    Some((name.to_string(), addr))
}

fn parse_probe(out: &str) -> RawProbe {
    let mut raw = RawProbe::default();
    let mut section = "";
    for line in out.lines() {
        let line = line.trim_end();
        match line {
            "LINKS:" => {
                section = "links";
                continue;
            }
            "ADDRS:" => {
                section = "addrs";
                continue;
            }
            "ROUTE:" => {
                section = "route";
                continue;
            }
            "CMDS:" => {
                section = "cmds";
                continue;
            }
            "SVCS:" => {
                section = "svcs";
                continue;
            }
            "FILES:" => {
                section = "files";
                continue;
            }
            _ => {}
        }
        if let Some(v) = line.strip_prefix("OS:") {
            raw.os_release = parse_os_release(v);
        } else if let Some(v) = line.strip_prefix("UNAME:") {
            raw.uname = v.to_string();
        } else if let Some(v) = line.strip_prefix("UID:") {
            raw.uid = v.trim().parse().ok();
        } else if let Some(v) = line.strip_prefix("CMD:") {
            raw.cmds.insert(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("SVC:") {
            if let Some((name, state)) = v.split_once(':') {
                raw.svcs.insert(name.trim().to_string(), state.trim().to_string());
            }
        } else if let Some(v) = line.strip_prefix("FILE:") {
            raw.files.insert(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("GUESTADD:") {
            raw.guest_additions = !v.trim().is_empty();
        } else {
            match section {
                "links" => {
                    if let Some(x) = parse_link_line(line) {
                        raw.links.push(x);
                    }
                }
                "addrs" => {
                    if let Some((name, ip)) = parse_addr_line(line) {
                        raw.addrs.insert(name, ip);
                    }
                }
                "route" => {
                    if line.starts_with("default ") {
                        if let Some((_, after)) = line.split_once(" dev ") {
                            raw.default_iface = after.split_whitespace().next().map(str::to_string);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    raw
}

fn any_file_starting(raw: &RawProbe, prefix: &str) -> bool {
    raw.files.iter().any(|f| f.starts_with(prefix))
}

fn svc_active(raw: &RawProbe, name: &str) -> bool {
    raw.svcs.get(name).is_some_and(|s| s == "active")
}

fn system_info(raw: &RawProbe) -> Vec<GuestSystemInfo> {
    let os_id = raw.os_release.get("ID").map(|s| s.to_lowercase()).unwrap_or_default();
    let suse = os_id.contains("opensuse") || os_id == "sles" || os_id == "sled";

    let mut clients: Vec<GuestSystemInfo> = Vec::new();
    let mut add = |system: GuestSystem, path: &str, active: bool| {
        clients.push(GuestSystemInfo {
            system,
            label: system.label().to_string(),
            config_path: path.to_string(),
            active,
            recommended: false,
            kind: system.kind(),
        });
    };

    let netplan = raw.files.contains("/etc/netplan")
        || any_file_starting(raw, "/etc/netplan/")
        || raw.cmds.contains("netplan");
    let nm = raw.cmds.contains("nmcli") || svc_active(raw, "NetworkManager") || raw.files.contains("/etc/NetworkManager/system-connections");
    let networkd = svc_active(raw, "systemd-networkd") || raw.files.contains("/etc/systemd/network");
    let ifupdown = raw.files.contains("/etc/network/interfaces")
        || raw.files.contains("/etc/network/interfaces.d")
        || raw.cmds.contains("ifup");
    let wicked = raw.cmds.contains("wicked") || (suse && raw.files.contains("/etc/sysconfig/network"));
    let dhcpcd = raw.files.contains("/etc/dhcpcd.conf") || raw.cmds.contains("dhcpcd");
    let connman = raw.cmds.contains("connmanctl") || raw.files.contains("/var/lib/connman") || svc_active(raw, "connman");
    let scripts = any_file_starting(raw, "/etc/sysconfig/network-scripts/ifcfg-")
        || raw.files.contains("/etc/sysconfig/network-scripts");

    if netplan {
        add(GuestSystem::Netplan, "/etc/netplan/90-vmlab.yaml", raw.cmds.contains("netplan"));
    }
    if suse && wicked {
        add(GuestSystem::Wicked, "/etc/sysconfig/network/ifcfg-<interfaz>", svc_active(raw, "wickedd"));
    }
    if nm {
        add(
            GuestSystem::NetworkManager,
            "/etc/NetworkManager/system-connections/vmlab-<interfaz>.nmconnection",
            svc_active(raw, "NetworkManager"),
        );
    }
    if networkd {
        add(
            GuestSystem::SystemdNetworkd,
            "/etc/systemd/network/90-vmlab-<interfaz>.network",
            svc_active(raw, "systemd-networkd"),
        );
    }
    if ifupdown {
        add(
            GuestSystem::Ifupdown,
            if raw.files.contains("/etc/network/interfaces.d") {
                "/etc/network/interfaces.d/vmlab-<interfaz>"
            } else {
                "/etc/network/interfaces"
            },
            raw.cmds.contains("ifup"),
        );
    }
    if !suse && wicked {
        add(GuestSystem::Wicked, "/etc/sysconfig/network/ifcfg-<interfaz>", svc_active(raw, "wickedd"));
    }
    if dhcpcd {
        add(GuestSystem::Dhcpcd, "/etc/dhcpcd.conf", svc_active(raw, "dhcpcd"));
    }
    if connman {
        add(GuestSystem::Connman, "/var/lib/connman/vmlab-<interfaz>.config", svc_active(raw, "connman"));
    }
    if scripts {
        add(
            GuestSystem::NetworkScripts,
            "/etc/sysconfig/network-scripts/ifcfg-<interfaz>",
            svc_active(raw, "network") || svc_active(raw, "NetworkManager"),
        );
    }

    // El primero se marca como recomendado para el modo cliente.
    if let Some(first) = clients.first_mut() {
        first.recommended = true;
    }

    // Si aún no existe ninguno, el script crea el de Debian; la ruta mostrada debe coincidir.
    let dhcpd_path = if raw.files.contains("/etc/dhcpd.conf") {
        "/etc/dhcpd.conf"
    } else {
        "/etc/dhcp/dhcpd.conf"
    };
    let dhcpd = raw.files.contains("/etc/dhcp/dhcpd.conf")
        || raw.files.contains("/etc/dhcpd.conf")
        || raw.cmds.contains("dhcpd");
    if dhcpd {
        clients.push(GuestSystemInfo {
            system: GuestSystem::Dhcpd,
            label: GuestSystem::Dhcpd.label().to_string(),
            config_path: dhcpd_path.to_string(),
            active: svc_active(raw, "isc-dhcp-server") || svc_active(raw, "dhcpd"),
            recommended: true,
            kind: GuestSystemKind::Server,
        });
    }

    clients
}

fn interfaces(raw: &RawProbe) -> Vec<GuestInterface> {
    let mut out: Vec<GuestInterface> = raw
        .links
        .iter()
        .map(|(name, mac, state)| GuestInterface {
            name: name.clone(),
            mac: mac.clone(),
            state: state.clone(),
            ipv4: raw.addrs.get(name).cloned(),
        })
        .collect();
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out.sort_by_key(|i| raw.default_iface.as_deref() != Some(i.name.as_str()));
    out
}

fn raw_profile(raw: &RawProbe) -> GuestProfile {
    let name = raw.os_release.get("PRETTY_NAME").cloned().unwrap_or_else(|| "Linux".into());
    let os_id = raw.os_release.get("ID").cloned().unwrap_or_default().to_lowercase();
    let is_linux = raw.uname.to_lowercase().starts_with("linux");
    GuestProfile {
        os: name,
        os_id,
        kernel: raw.uname.clone(),
        is_linux,
        // Si el sondeo ha llegado hasta aquí es que guestcontrol funciona, así que
        // las Guest Additions están instaladas y el servicio está en marcha.
        guest_additions: raw.guest_additions || !raw.uname.is_empty(),
        is_root: raw.uid == Some(0),
        sudo: raw.cmds.contains("sudo"),
        systems: system_info(raw),
        interfaces: interfaces(raw),
    }
}

async fn probe_with_auth(tools: &Tools, vm: &str, auth: &mut Auth) -> Result<GuestProfile> {
    let out = run_script(tools, vm, auth, PROBE).await?;
    if out.code != 0 || !out.stdout.contains("VMLAB_END") {
        return Err(CoreError::Failed {
            program: "VBoxManage".into(),
            code: out.code,
            stderr: explain_guest_error(if out.stderr.trim().is_empty() { &out.stdout } else { &out.stderr }),
        });
    }
    let profile = raw_profile(&parse_probe(&out.stdout));
    if !profile.is_linux {
        return Err(CoreError::Invalid("la detección solo admite máquinas Linux".into()));
    }
    Ok(profile)
}

/// Detecta la distribución, los gestores de red y las interfaces dentro de la VM.
pub async fn probe(tools: &Tools, vm: &str, username: &str, password: &str) -> Result<GuestProfile> {
    validate::vbox_target(vm)?;
    let mut auth = Auth::new(username, password)?;
    probe_with_auth(tools, vm, &mut auth).await
}

// ───────────────────────── Parámetros validados ─────────────────────────

#[derive(Debug, Clone)]
struct Params {
    iface: String,
    mode: GuestIpMode,
    address: Option<Ipv4Addr>,
    prefix: u8,
    gateway: Option<Ipv4Addr>,
    dns: Vec<Ipv4Addr>,
    subnet: Option<(Ipv4Addr, u8)>,
    range_start: Option<Ipv4Addr>,
    range_end: Option<Ipv4Addr>,
    renderer: &'static str,
}

fn ip(s: &str, what: &str) -> Result<Ipv4Addr> {
    s.trim().parse::<Ipv4Addr>().map_err(|_| CoreError::Invalid(format!("{what} no es una IPv4 válida: «{s}»")))
}

fn cidr(s: &str) -> Result<(Ipv4Addr, u8)> {
    let (addr, prefix) = s.split_once('/').ok_or_else(|| CoreError::Invalid(format!("subred no válida: «{s}» (usa CIDR, p. ej. 10.77.5.0/24)")))?;
    let addr = ip(addr, "la subred")?;
    let prefix: u8 = prefix.parse().map_err(|_| CoreError::Invalid(format!("prefijo no válido: «{s}»")))?;
    if !(1..=32).contains(&prefix) {
        return Err(CoreError::Invalid("el prefijo debe estar entre 1 y 32".into()));
    }
    Ok((addr, prefix))
}

fn mask(prefix: u8) -> u32 {
    if prefix == 0 { 0 } else { u32::MAX << (32 - u32::from(prefix)) }
}

fn network(addr: Ipv4Addr, prefix: u8) -> u32 {
    u32::from(addr) & mask(prefix)
}

fn in_subnet(addr: Ipv4Addr, net: Ipv4Addr, prefix: u8) -> bool {
    u32::from(addr) & mask(prefix) == network(net, prefix)
}

fn netmask(prefix: u8) -> Ipv4Addr {
    Ipv4Addr::from(mask(prefix))
}

fn net_from_u32(n: u32) -> Ipv4Addr {
    Ipv4Addr::from(n)
}

impl Params {
    fn from_request(req: &GuestConfigRequest, renderer: &'static str) -> Result<Self> {
        let iface = validate::linux_iface(&req.interface)?.to_string();
        let mut dns = Vec::new();
        for d in &req.dns {
            if !d.trim().is_empty() {
                dns.push(ip(d, "el DNS")?);
            }
        }

        let mut p = Params {
            iface,
            mode: req.ip_mode,
            address: None,
            prefix: 24,
            gateway: req.gateway.as_deref().filter(|g| !g.trim().is_empty()).map(|g| ip(g, "la puerta de enlace")).transpose()?,
            dns,
            subnet: None,
            range_start: None,
            range_end: None,
            renderer,
        };

        match req.ip_mode {
            GuestIpMode::Dhcp => {}
            GuestIpMode::Static => {
                let addr = req
                    .address
                    .as_deref()
                    .filter(|a| !a.trim().is_empty())
                    .ok_or_else(|| CoreError::Invalid("la IP estática es obligatoria".into()))?;
                p.address = Some(ip(addr, "la IP")?);
                let prefix = req.prefix.ok_or_else(|| CoreError::Invalid("el prefijo es obligatorio".into()))?;
                if !(1..=32).contains(&prefix) {
                    return Err(CoreError::Invalid("el prefijo debe estar entre 1 y 32".into()));
                }
                p.prefix = prefix;
            }
            GuestIpMode::Server => {
                let subnet = req
                    .subnet
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| CoreError::Invalid("la subred del servidor DHCP es obligatoria".into()))?;
                let (net, prefix) = cidr(subnet)?;
                if !(8..=30).contains(&prefix) {
                    return Err(CoreError::Invalid("la subred del servidor DHCP debe tener entre /8 y /30".into()));
                }
                p.subnet = Some((net_from_u32(network(net, prefix)), prefix));
                p.prefix = prefix;
                let start = req
                    .range_start
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| CoreError::Invalid("el inicio del rango DHCP es obligatorio".into()))?;
                let end = req
                    .range_end
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .ok_or_else(|| CoreError::Invalid("el fin del rango DHCP es obligatorio".into()))?;
                let start = ip(start, "el inicio del rango")?;
                let end = ip(end, "el fin del rango")?;
                if !in_subnet(start, net, prefix) || !in_subnet(end, net, prefix) {
                    return Err(CoreError::Invalid("el rango DHCP debe estar dentro de la subred".into()));
                }
                if u32::from(start) > u32::from(end) {
                    return Err(CoreError::Invalid("el inicio del rango no puede ser mayor que el fin".into()));
                }
                p.range_start = Some(start);
                p.range_end = Some(end);
                if p.gateway.is_none() {
                    p.gateway = Some(net_from_u32(network(net, prefix) + 1));
                }
                if let Some(g) = p.gateway {
                    if !in_subnet(g, net, prefix) {
                        return Err(CoreError::Invalid("la puerta de enlace debe estar dentro de la subred".into()));
                    }
                }
            }
        }
        Ok(p)
    }

    fn dns_line(&self) -> String {
        self.dns.iter().map(ToString::to_string).collect::<Vec<_>>().join(" ")
    }
}

// ───────────────────────── Render de ficheros ─────────────────────────

fn netplan_yaml(p: &Params) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "network:");
    let _ = writeln!(s, "  version: 2");
    let _ = writeln!(s, "  renderer: {}", p.renderer);
    let _ = writeln!(s, "  ethernets:");
    let _ = writeln!(s, "    {}:", p.iface);
    match p.mode {
        GuestIpMode::Dhcp => {
            let _ = writeln!(s, "      dhcp4: true");
        }
        GuestIpMode::Static => {
            let _ = writeln!(s, "      dhcp4: false");
            let _ = writeln!(s, "      addresses:");
            let _ = writeln!(s, "        - {}/{}", p.address.expect("validada"), p.prefix);
            if let Some(gw) = p.gateway {
                let _ = writeln!(s, "      routes:");
                let _ = writeln!(s, "        - to: default");
                let _ = writeln!(s, "          via: {gw}");
            }
            if !p.dns.is_empty() {
                let _ = writeln!(s, "      nameservers:");
                let _ = writeln!(s, "        addresses: [{}]", p.dns.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "));
            }
        }
        GuestIpMode::Server => {}
    }
    s
}

fn pseudo_uuid(seed: &str) -> String {
    let mut h1: u64 = 0xcbf2_9ce4_8422_2325;
    let mut h2: u64 = 0x8422_2325_cbf2_9ce4;
    for b in seed.bytes() {
        h1 ^= u64::from(b);
        h1 = h1.wrapping_mul(0x0000_0100_0000_01b3);
        h2 ^= u64::from(b).wrapping_add(0x9e37_79b9_7f4a_7c15);
        h2 = h2.rotate_left(7).wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!(
        "{:08x}-{:04x}-4{:03x}-8{:03x}-{:012x}",
        (h1 >> 32) as u32,
        (h1 >> 16) as u16,
        (h1 & 0xfff) as u16,
        ((h2 >> 48) & 0xfff) as u16,
        h2 & 0xffff_ffff_ffff
    )
}

fn nm_keyfile(p: &Params) -> String {
    let id = format!("vmlab-{}", p.iface);
    let mut s = String::new();
    let _ = writeln!(s, "[connection]");
    let _ = writeln!(s, "id={id}");
    let _ = writeln!(s, "uuid={}", pseudo_uuid(&id));
    let _ = writeln!(s, "type=ethernet");
    let _ = writeln!(s, "interface-name={}", p.iface);
    let _ = writeln!(s, "autoconnect=true");
    let _ = writeln!(s, "autoconnect-priority=100");
    let _ = writeln!(s);
    let _ = writeln!(s, "[ethernet]");
    let _ = writeln!(s, "mac-address-blacklist=");
    let _ = writeln!(s);
    let _ = writeln!(s, "[ipv4]");
    match p.mode {
        GuestIpMode::Dhcp => {
            let _ = writeln!(s, "method=auto");
        }
        GuestIpMode::Static => {
            let _ = writeln!(s, "method=manual");
            match p.gateway {
                Some(gw) => {
                    let _ = writeln!(s, "address1={}/{},{}", p.address.expect("validada"), p.prefix, gw);
                }
                None => {
                    let _ = writeln!(s, "address1={}/{}", p.address.expect("validada"), p.prefix);
                }
            }
            if !p.dns.is_empty() {
                let _ = writeln!(s, "dns={};", p.dns.iter().map(ToString::to_string).collect::<Vec<_>>().join(";"));
            }
        }
        GuestIpMode::Server => {}
    }
    let _ = writeln!(s, "may-fail=false");
    let _ = writeln!(s);
    let _ = writeln!(s, "[ipv6]");
    let _ = writeln!(s, "method=ignore");
    s
}

fn networkd_unit(p: &Params) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "[Match]");
    let _ = writeln!(s, "Name={}", p.iface);
    let _ = writeln!(s);
    let _ = writeln!(s, "[Network]");
    match p.mode {
        GuestIpMode::Dhcp => {
            let _ = writeln!(s, "DHCP=yes");
        }
        GuestIpMode::Static => {
            let _ = writeln!(s, "Address={}/{}", p.address.expect("validada"), p.prefix);
            if let Some(gw) = p.gateway {
                let _ = writeln!(s, "Gateway={gw}");
            }
            for d in &p.dns {
                let _ = writeln!(s, "DNS={d}");
            }
        }
        GuestIpMode::Server => {}
    }
    s
}

fn ifupdown_stanza(p: &Params) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "auto {}", p.iface);
    match p.mode {
        GuestIpMode::Dhcp => {
            let _ = writeln!(s, "iface {} inet dhcp", p.iface);
        }
        GuestIpMode::Static => {
            let _ = writeln!(s, "iface {} inet static", p.iface);
            let _ = writeln!(s, "    address {}/{}", p.address.expect("validada"), p.prefix);
            if let Some(gw) = p.gateway {
                let _ = writeln!(s, "    gateway {gw}");
            }
            if !p.dns.is_empty() {
                let _ = writeln!(s, "    dns-nameservers {}", p.dns_line());
            }
        }
        GuestIpMode::Server => {}
    }
    s
}

fn wicked_ifcfg(p: &Params) -> String {
    let mut s = String::new();
    match p.mode {
        GuestIpMode::Dhcp => {
            let _ = writeln!(s, "BOOTPROTO='dhcp'");
            let _ = writeln!(s, "STARTMODE='auto'");
        }
        GuestIpMode::Static => {
            let _ = writeln!(s, "BOOTPROTO='static'");
            let _ = writeln!(s, "STARTMODE='auto'");
            let _ = writeln!(s, "IPADDR='{}'", p.address.expect("validada"));
            let _ = writeln!(s, "PREFIXLEN='{}'", p.prefix);
            if let Some(gw) = p.gateway {
                let _ = writeln!(s, "GATEWAY='{gw}'");
            }
        }
        GuestIpMode::Server => {}
    }
    s
}

fn dhcpcd_block(p: &Params) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "{BLOCK_START}");
    let _ = writeln!(s, "interface {}", p.iface);
    if p.mode == GuestIpMode::Static {
        let _ = writeln!(s, "static ip_address={}/{}", p.address.expect("validada"), p.prefix);
        if let Some(gw) = p.gateway {
            let _ = writeln!(s, "static routers={gw}");
        }
        if !p.dns.is_empty() {
            let _ = writeln!(s, "static domain_name_servers={}", p.dns_line());
        }
    }
    let _ = writeln!(s, "{BLOCK_END}");
    s
}

fn connman_config(p: &Params) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "[global]");
    let _ = writeln!(s, "Name = vmlab");
    let _ = writeln!(s);
    let _ = writeln!(s, "[service_vmlab_{}]", p.iface);
    let _ = writeln!(s, "Type = ethernet");
    let _ = writeln!(s, "Interface = {}", p.iface);
    match p.mode {
        GuestIpMode::Dhcp => {
            let _ = writeln!(s, "IPv4 = dhcp");
        }
        GuestIpMode::Static => {
            match p.gateway {
                Some(gw) => {
                    let _ = writeln!(s, "IPv4 = {}/{}/{}", p.address.expect("validada"), p.prefix, gw);
                }
                None => {
                    let _ = writeln!(s, "IPv4 = {}/{}", p.address.expect("validada"), p.prefix);
                }
            }
            if !p.dns.is_empty() {
                let _ = writeln!(s, "Nameservers = {}", p.dns.iter().map(ToString::to_string).collect::<Vec<_>>().join(","));
            }
        }
        GuestIpMode::Server => {}
    }
    s
}

fn network_scripts(p: &Params) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "TYPE=Ethernet");
    let _ = writeln!(s, "NAME=vmlab-{}", p.iface);
    let _ = writeln!(s, "DEVICE={}", p.iface);
    let _ = writeln!(s, "ONBOOT=yes");
    let _ = writeln!(s, "DEFROUTE=yes");
    match p.mode {
        GuestIpMode::Dhcp => {
            let _ = writeln!(s, "BOOTPROTO=dhcp");
        }
        GuestIpMode::Static => {
            let _ = writeln!(s, "BOOTPROTO=none");
            let _ = writeln!(s, "IPADDR={}", p.address.expect("validada"));
            let _ = writeln!(s, "PREFIX={}", p.prefix);
            if let Some(gw) = p.gateway {
                let _ = writeln!(s, "GATEWAY={gw}");
            }
            for (i, d) in p.dns.iter().enumerate() {
                let _ = writeln!(s, "DNS{}={d}", i + 1);
            }
        }
        GuestIpMode::Server => {}
    }
    s
}

fn dhcpd_block(p: &Params) -> String {
    let (net, prefix) = p.subnet.expect("validada");
    let mut s = String::new();
    let _ = writeln!(s, "{BLOCK_START}");
    let _ = writeln!(s, "subnet {net} netmask {} {{", netmask(prefix));
    let _ = writeln!(s, "  range {} {};", p.range_start.expect("validado"), p.range_end.expect("validado"));
    if let Some(gw) = p.gateway {
        let _ = writeln!(s, "  option routers {gw};");
    }
    if !p.dns.is_empty() {
        let _ = writeln!(
            s,
            "  option domain-name-servers {};",
            p.dns.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
        );
    }
    let _ = writeln!(s, "  default-lease-time 600;");
    let _ = writeln!(s, "  max-lease-time 7200;");
    let _ = writeln!(s, "}}");
    let _ = writeln!(s, "{BLOCK_END}");
    s
}

// ───────────────────────── Script de escritura ─────────────────────────

fn q(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// Escribe un fichero completo (VMLab es su dueño) y lo marca en la salida.
fn write_owned(path: &str, content: &str, mode: &str) -> String {
    format!(
        "mkdir -p \"$(dirname {p})\"\ncat > {p} <<'VMLAB_EOF'\n{c}VMLAB_EOF\nchmod {mode} {p}\necho {marker}\n",
        p = q(path),
        c = content,
        mode = mode,
        marker = q(&format!("VMLAB_FILE:{path}"))
    )
}

/// Borra el fichero que escribía la versión anterior (`lablab-*`), para que no
/// queden dos configuraciones activas en la misma interfaz.
fn remove_legacy(path: &str) -> String {
    format!("rm -f {p}\n", p = q(path))
}

/// Sustituye el bloque `# >>> vmlab … # <<< vmlab` de un fichero existente.
/// También elimina bloques antiguos `# >>> lablab … # <<< lablab`.
fn replace_block(path: &str, content: &str) -> String {
    format!(
        "f={p}\nmkdir -p \"$(dirname \"$f\")\"\n[ -f \"$f\" ] || : > \"$f\"\ntmp=\"$f.vmlab.tmp\"\n\
         awk 'BEGIN{{skip=0}} /^# >>> (vmlab|lablab)/ {{skip=1; next}} /^# <<< (vmlab|lablab)/ {{skip=0; next}} !skip' \"$f\" > \"$tmp\"\n\
         cat \"$tmp\" > \"$f\"\nrm -f \"$tmp\"\ncat >> \"$f\" <<'VMLAB_EOF'\n{c}VMLAB_EOF\necho {marker}\n",
        p = q(path),
        c = content,
        marker = q(&format!("VMLAB_FILE:{path}"))
    )
}

fn replace_block_var(content: &str) -> String {
    format!(
        "mkdir -p \"$(dirname \"$f\")\"\n[ -f \"$f\" ] || : > \"$f\"\ntmp=\"$f.vmlab.tmp\"\n\
         awk 'BEGIN{{skip=0}} /^# >>> (vmlab|lablab)/ {{skip=1; next}} /^# <<< (vmlab|lablab)/ {{skip=0; next}} !skip' \"$f\" > \"$tmp\"\n\
         cat \"$tmp\" > \"$f\"\nrm -f \"$tmp\"\ncat >> \"$f\" <<'VMLAB_EOF'\n{c}VMLAB_EOF\necho \"VMLAB_FILE:$f\"\n",
        c = content
    )
}

fn reload_nm(iface: &str) -> String {
    format!(
        "if command -v nmcli >/dev/null 2>&1; then\n  nmcli connection delete {old} >/dev/null 2>&1 || true\n  nmcli connection reload || true\n  nmcli connection up {id} >/dev/null 2>&1 || echo 'VMLAB_WARN:NetworkManager no pudo activar {id}; revisa «nmcli connection show».'\nelse\n  systemctl restart NetworkManager 2>/dev/null || true\nfi\n",
        old = q(&format!("lablab-{iface}")),
        id = q(&format!("vmlab-{iface}"))
    )
}

fn build_script(system: GuestSystem, p: &Params) -> Result<String> {
    let mut s = String::from("#!/bin/sh\nset -u\n");
    s.push_str(
        "if [ \"$(id -u)\" -ne 0 ]; then\n  if command -v sudo >/dev/null 2>&1; then\n    exec sudo -n /bin/sh \"$0\" \"$@\"\n  fi\n  echo 'VMLAB_ERROR:se necesita root y sudo no está instalado. Usa un usuario root o instala sudo.' >&2\n  exit 2\nfi\n",
    );

    match system {
        GuestSystem::Netplan => {
            let path = "/etc/netplan/90-vmlab.yaml";
            s.push_str(&remove_legacy("/etc/netplan/90-lablab.yaml"));
            s.push_str(&write_owned(path, &netplan_yaml(p), "600"));
            s.push_str("if command -v netplan >/dev/null 2>&1; then\n  netplan generate || { echo 'VMLAB_ERROR:netplan generate falló'; exit 3; }\n  netplan apply || echo 'VMLAB_WARN:netplan apply devolvió un error; revisa la configuración.'\nelse\n  echo 'VMLAB_WARN:netplan no está instalado; el fichero se escribió pero no se aplicó.'\nfi\n");
        }
        GuestSystem::NetworkManager => {
            let path = format!("/etc/NetworkManager/system-connections/vmlab-{}.nmconnection", p.iface);
            s.push_str(&remove_legacy(&format!(
                "/etc/NetworkManager/system-connections/lablab-{}.nmconnection",
                p.iface
            )));
            s.push_str(&write_owned(&path, &nm_keyfile(p), "600"));
            s.push_str(&reload_nm(&p.iface));
        }
        GuestSystem::SystemdNetworkd => {
            let path = format!("/etc/systemd/network/90-vmlab-{}.network", p.iface);
            s.push_str(&remove_legacy(&format!("/etc/systemd/network/90-lablab-{}.network", p.iface)));
            s.push_str(&write_owned(&path, &networkd_unit(p), "644"));
            s.push_str("systemctl restart systemd-networkd 2>/dev/null || networkctl reload 2>/dev/null || echo 'VMLAB_WARN:no se pudo recargar systemd-networkd.'\n");
        }
        GuestSystem::Ifupdown => {
            let path = format!("/etc/network/interfaces.d/vmlab-{}", p.iface);
            let stanza = ifupdown_stanza(p);
            s.push_str(&remove_legacy(&format!("/etc/network/interfaces.d/lablab-{}", p.iface)));
            s.push_str(&format!(
                "if [ -d /etc/network/interfaces.d ]; then\n{owned}else\n{block}fi\n",
                owned = write_owned(&path, &stanza, "644"),
                block = replace_block("/etc/network/interfaces", &stanza)
            ));
            s.push_str(&format!(
                "if command -v ifup >/dev/null 2>&1; then\n  ifdown {iface} 2>/dev/null || true\n  ifup {iface} || echo 'VMLAB_WARN:ifup devolvió un error; revisa la configuración.'\nelse\n  systemctl restart networking 2>/dev/null || true\nfi\n",
                iface = q(&p.iface)
            ));
        }
        GuestSystem::Wicked => {
            let path = format!("/etc/sysconfig/network/ifcfg-{}", p.iface);
            s.push_str(&write_owned(&path, &wicked_ifcfg(p), "644"));
            s.push_str(&format!(
                "if command -v wicked >/dev/null 2>&1; then\n  wicked ifreload {iface} || echo 'VMLAB_WARN:wicked ifreload devolvió un error.'\nelse\n  systemctl restart wicked 2>/dev/null || true\nfi\n",
                iface = q(&p.iface)
            ));
        }
        GuestSystem::Dhcpcd => {
            s.push_str(&replace_block("/etc/dhcpcd.conf", &dhcpcd_block(p)));
            s.push_str("systemctl restart dhcpcd 2>/dev/null || dhcpcd -n 2>/dev/null || echo 'VMLAB_WARN:no se pudo reiniciar dhcpcd.'\n");
        }
        GuestSystem::Connman => {
            let path = format!("/var/lib/connman/vmlab-{}.config", p.iface);
            s.push_str(&remove_legacy(&format!("/var/lib/connman/lablab-{}.config", p.iface)));
            s.push_str(&write_owned(&path, &connman_config(p), "600"));
            s.push_str("systemctl restart connman 2>/dev/null || echo 'VMLAB_WARN:no se pudo reiniciar ConnMan.'\n");
        }
        GuestSystem::NetworkScripts => {
            let path = format!("/etc/sysconfig/network-scripts/ifcfg-{}", p.iface);
            s.push_str(&write_owned(&path, &network_scripts(p), "644"));
            s.push_str("nmcli connection reload 2>/dev/null || systemctl restart network 2>/dev/null || echo 'VMLAB_WARN:no se pudo recargar network-scripts.'\n");
        }
        GuestSystem::Dhcpd => {
            // Se busca la ruta exacta entre las habituales; si no existe, se crea la de Debian.
            s.push_str("f=/etc/dhcp/dhcpd.conf\n");
            s.push_str("for c in /etc/dhcp/dhcpd.conf /etc/dhcpd.conf; do [ -e \"$c\" ] && f=\"$c\" && break; done\n");
            s.push_str(&replace_block_var(&dhcpd_block(p)));
            // Interfaz del servicio en Debian/Ubuntu y RHEL.
            s.push_str(&format!(
                "if [ -f /etc/default/isc-dhcp-server ]; then\n  sed -i 's|^INTERFACESv4=.*|INTERFACESv4=\"{iface}\"|' /etc/default/isc-dhcp-server && echo 'VMLAB_MSG:/etc/default/isc-dhcp-server actualizado'\nfi\n",
                iface = p.iface
            ));
            s.push_str(&format!(
                "if [ -f /etc/sysconfig/dhcpd ]; then\n  sed -i 's|^DHCPDARGS=.*|DHCPDARGS={iface}|' /etc/sysconfig/dhcpd && echo 'VMLAB_MSG:/etc/sysconfig/dhcpd actualizado'\nfi\n",
                iface = p.iface
            ));
            s.push_str(
                "if command -v dhcpd >/dev/null 2>&1; then\n  dhcpd -t -cf \"$f\" >/dev/null 2>&1 || { echo 'VMLAB_ERROR:dhcpd -t ha rechazado la configuración'; exit 4; }\nfi\n",
            );
            s.push_str("systemctl restart isc-dhcp-server 2>/dev/null || systemctl restart dhcpd 2>/dev/null || echo 'VMLAB_WARN:instala isc-dhcp-server o reinicia el servicio DHCP a mano.'\n");
            s.push_str("echo 'VMLAB_WARN:recuerda desactivar el DHCP integrado de VirtualBox en esta red para que no haya dos servidores.'\n");
        }
        GuestSystem::Auto => return Err(CoreError::Invalid("el sistema de red no se pudo resolver".into())),
    }

    s.push_str("echo 'VMLAB_OK'\n");
    Ok(s)
}

// ───────────────────────── Aplicación ─────────────────────────

fn choose_system(req: &GuestConfigRequest, profile: &GuestProfile) -> Result<GuestSystem> {
    if req.ip_mode == GuestIpMode::Server {
        return match req.system {
            GuestSystem::Auto | GuestSystem::Dhcpd => Ok(profile
                .systems
                .iter()
                .find(|s| s.kind == GuestSystemKind::Server)
                .map(|s| s.system)
                .unwrap_or(GuestSystem::Dhcpd)),
            _ => Err(CoreError::Invalid(
                "el modo «servidor DHCP» escribe dhcpd.conf; elige «Servidor DHCP ISC» o cambia a DHCP/estática".into(),
            )),
        };
    }
    match req.system {
        GuestSystem::Auto => profile
            .systems
            .iter()
            .find(|s| s.kind == GuestSystemKind::Client && s.recommended)
            .or_else(|| profile.systems.iter().find(|s| s.kind == GuestSystemKind::Client))
            .map(|s| s.system)
            .ok_or_else(|| {
                CoreError::Invalid(
                    "no se reconoció ningún gestor de red Linux en la VM. Instala uno de los soportados o elige uno manualmente.".into(),
                )
            }),
        GuestSystem::Dhcpd => Err(CoreError::Invalid(
            "dhcpd.conf es un servidor DHCP; elige el modo «Servidor DHCP» para escribirlo".into(),
        )),
        s => Ok(s),
    }
}

fn parse_markers(out: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let mut files = Vec::new();
    let mut messages = Vec::new();
    let mut warnings = Vec::new();
    for line in out.lines() {
        if let Some(v) = line.strip_prefix("VMLAB_FILE:") {
            if !v.trim().is_empty() {
                files.push(v.trim().to_string());
            }
        } else if let Some(v) = line.strip_prefix("VMLAB_MSG:") {
            messages.push(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("VMLAB_WARN:") {
            warnings.push(v.trim().to_string());
        }
    }
    (files, messages, warnings)
}

/// Detecta el gestor, escribe su configuración y la aplica dentro de la VM.
pub async fn apply(tools: &Tools, req: GuestConfigRequest) -> Result<GuestReport> {
    validate::vbox_target(&req.target)?;
    let mut auth = Auth::new(&req.username, &req.password)?;

    // Se sondea siempre: hace falta para elegir rutas y el renderer de Netplan.
    let profile = probe_with_auth(tools, &req.target, &mut auth).await?;
    let system = choose_system(&req, &profile)?;

    if req.ip_mode == GuestIpMode::Server && system != GuestSystem::Dhcpd {
        return Err(CoreError::Invalid(
            "el modo servidor DHCP solo escribe dhcpd.conf; elige el servidor DHCP ISC".into(),
        ));
    }
    if req.ip_mode != GuestIpMode::Server && system == GuestSystem::Dhcpd {
        return Err(CoreError::Invalid(
            "dhcpd.conf es un servidor DHCP; cambia el modo a «Servidor DHCP»".into(),
        ));
    }

    let renderer = if profile.systems.iter().any(|s| s.system == GuestSystem::NetworkManager && s.active) {
        "NetworkManager"
    } else {
        "networkd"
    };
    let params = Params::from_request(&req, renderer)?;
    let script = build_script(system, &params)?;
    let out = run_script(tools, &req.target, &mut auth, &script).await?;

    let (files, messages, mut warnings) = parse_markers(&out.stdout);
    if out.code != 0 || !out.stdout.contains("VMLAB_OK") {
        let detail = out
            .stdout
            .lines()
            .chain(out.stderr.lines())
            .find(|l| l.contains("VMLAB_ERROR"))
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .unwrap_or_else(|| {
                if out.stderr.trim().is_empty() {
                    out.stdout.trim().chars().take(400).collect()
                } else {
                    out.stderr.trim().chars().take(400).collect()
                }
            });
        return Err(CoreError::Failed {
            program: "VBoxManage".into(),
            code: out.code,
            stderr: detail,
        });
    }

    if !profile.is_root && !profile.sudo {
        warnings.push("la VM no tiene sudo; si la escritura falló, usa un usuario root.".into());
    }
    for w in out.stdout.lines().filter_map(|l| l.strip_prefix("VMLAB_WARN:")) {
        let w = w.trim();
        if !w.is_empty() && !warnings.iter().any(|x| x == w) {
            warnings.push(w.to_string());
        }
    }

    Ok(GuestReport { system, files, messages, warnings })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req_static() -> GuestConfigRequest {
        GuestConfigRequest {
            target: "vm".into(),
            username: "ubuntu".into(),
            password: "x".into(),
            system: GuestSystem::Netplan,
            interface: "eth0".into(),
            ip_mode: GuestIpMode::Static,
            address: Some("10.0.0.5".into()),
            prefix: Some(24),
            gateway: Some("10.0.0.1".into()),
            dns: vec!["1.1.1.1".into(), "8.8.8.8".into()],
            subnet: None,
            range_start: None,
            range_end: None,
        }
    }

    fn params(mode: GuestIpMode) -> Params {
        let mut req = req_static();
        req.ip_mode = mode;
        if mode == GuestIpMode::Server {
            req.subnet = Some("10.77.5.0/24".into());
            req.range_start = Some("10.77.5.100".into());
            req.range_end = Some("10.77.5.200".into());
            req.gateway = Some("10.77.5.1".into());
        }
        Params::from_request(&req, "networkd").unwrap()
    }

    #[test]
    fn parses_os_release() {
        let kv = parse_os_release(r#"NAME="Ubuntu"|VERSION="22.04.4 LTS"|ID=ubuntu|PRETTY_NAME="Ubuntu 22.04.4 LTS"||"#);
        assert_eq!(kv.get("ID").unwrap(), "ubuntu");
        assert_eq!(kv.get("PRETTY_NAME").unwrap(), "Ubuntu 22.04.4 LTS");
    }

    #[test]
    fn password_fallback_only_for_option_errors() {
        assert!(auth_file_error("VBoxManage: error: Unknown option: --passwordfile"));
        assert!(auth_file_error("VBoxManage: error: invalid parameter --passwordfile"));
        // Un fallo de credenciales no debe reintentarse con la contraseña en argv.
        assert!(!auth_file_error("VBoxManage: error: Invalid password"));
        assert!(!auth_file_error("VBoxManage: error: The specified user was not found"));
    }

    #[test]
    fn explains_missing_sudo() {
        let e = explain_guest_error("sudo: a password is required");
        assert!(e.contains("sudo sin contraseña"));
    }

    #[test]
    fn copy_errors_do_not_blame_guest_additions() {
        let e = explain_guest_error(
            "VBoxManage.exe: error: File copy failed\nVBoxManage.exe: error: Destination \"/tmp\" on the guest already exists and is a directory\nDetails: code VBOX_E_IPRT_ERROR (0x80bb0005)",
        );
        assert!(!e.to_lowercase().contains("guest additions"), "{e}");
        assert!(e.contains("already exists"));
    }

    #[test]
    fn parses_probe_sections() {
        let out = "VMLAB_BEGIN\nOS:ID=ubuntu|PRETTY_NAME=\"Ubuntu 22.04\"|\nUNAME:Linux 6.5 x86_64\nUID:1000\nCMDS:\nCMD:nmcli\nSVC:NetworkManager:active\nFILES:\nFILE:/etc/netplan\nLINKS:\n2: eth0: <BROADCAST> mtu 1500 state UP\n    link/ether 08:00:27:aa:bb:cc brd ff:ff:ff:ff:ff:ff\nADDRS:\n2: eth0    inet 10.0.2.15/24 brd 10.0.2.255 scope global eth0\nROUTE:\ndefault via 10.0.2.2 dev eth0 proto dhcp\nVMLAB_END\n";
        let raw = parse_probe(out);
        assert_eq!(raw.uid, Some(1000));
        assert!(raw.cmds.contains("nmcli"));
        assert_eq!(raw.default_iface.as_deref(), Some("eth0"));
        let p = raw_profile(&raw);
        assert_eq!(p.os, "Ubuntu 22.04");
        assert_eq!(p.interfaces.len(), 1);
        assert_eq!(p.interfaces[0].ipv4.as_deref(), Some("10.0.2.15/24"));
        assert!(p.systems.iter().any(|s| s.system == GuestSystem::Netplan));
    }

    #[test]
    fn dhcpd_defaults_to_path_created_by_the_script() {
        let out = "VMLAB_BEGIN\nOS:ID=ubuntu|PRETTY_NAME=\"Ubuntu\"|\nUNAME:Linux 6.5 x86_64\nUID:1000\nCMDS:\nCMD:dhcpd\nFILES:\nVMLAB_END\n";
        let p = raw_profile(&parse_probe(out));
        let s = p.systems.iter().find(|s| s.system == GuestSystem::Dhcpd).unwrap();
        assert_eq!(s.config_path, "/etc/dhcp/dhcpd.conf");
    }

    #[test]
    fn netplan_static_has_address_and_route() {
        let y = netplan_yaml(&params(GuestIpMode::Static));
        assert!(y.contains("eth0:"));
        assert!(y.contains("10.0.0.5/24"));
        assert!(y.contains("via: 10.0.0.1"));
        assert!(y.contains("1.1.1.1, 8.8.8.8"));
    }

    #[test]
    fn renders_all_client_systems() {
        let p = params(GuestIpMode::Static);
        assert!(nm_keyfile(&p).contains("address1=10.0.0.5/24,10.0.0.1"));
        assert!(networkd_unit(&p).contains("Gateway=10.0.0.1"));
        assert!(ifupdown_stanza(&p).contains("iface eth0 inet static"));
        assert!(wicked_ifcfg(&p).contains("PREFIXLEN='24'"));
        assert!(dhcpcd_block(&p).contains("static routers=10.0.0.1"));
        assert!(connman_config(&p).contains("IPv4 = 10.0.0.5/24/10.0.0.1"));
        assert!(network_scripts(&p).contains("PREFIX=24"));
    }

    #[test]
    fn renders_dhcpd_block() {
        let b = dhcpd_block(&params(GuestIpMode::Server));
        assert!(b.contains("subnet 10.77.5.0 netmask 255.255.255.0"));
        assert!(b.contains("range 10.77.5.100 10.77.5.200"));
        assert!(b.contains("option routers 10.77.5.1"));
    }

    #[test]
    fn rejects_out_of_subnet_range() {
        let mut req = req_static();
        req.ip_mode = GuestIpMode::Server;
        req.subnet = Some("10.77.5.0/24".into());
        req.range_start = Some("10.77.6.10".into());
        req.range_end = Some("10.77.6.20".into());
        assert!(Params::from_request(&req, "networkd").is_err());
    }

    #[test]
    fn script_writes_markers_and_reloads() {
        let p = params(GuestIpMode::Static);
        let s = build_script(GuestSystem::Netplan, &p).unwrap();
        assert!(s.contains("VMLAB_EOF"));
        assert!(s.contains("netplan apply"));
        assert!(s.contains("echo 'VMLAB_OK'"));
        // Migración desde la versión anterior: se borra su fichero y sus bloques.
        assert!(s.contains("rm -f '/etc/netplan/90-lablab.yaml'"));
        let dhcpcd = build_script(GuestSystem::Dhcpcd, &p).unwrap();
        assert!(dhcpcd.contains("(vmlab|lablab)"));
        let nm = build_script(GuestSystem::NetworkManager, &p).unwrap();
        assert!(nm.contains("nmconnection"));
        assert!(nm.contains("nmcli connection delete 'lablab-eth0'"));
        assert!(nm.contains("nmcli connection up 'vmlab-eth0'"));
    }

    #[test]
    fn pseudo_uuid_is_stable() {
        let a = pseudo_uuid("vmlab-eth0");
        assert_eq!(a, pseudo_uuid("vmlab-eth0"));
        assert_eq!(a.len(), 36);
    }
}
