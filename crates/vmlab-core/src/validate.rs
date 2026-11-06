//! Validación estricta de todo texto que acabará como argumento de un comando.
//! Como no hay shell, el riesgo real es la *inyección de opciones* (un valor que
//! empieza por `-` y se interpreta como flag), por eso todos los validadores lo prohíben.
use crate::error::{CoreError, Result};

fn bad<T>(what: &str, v: &str) -> Result<T> {
    Err(CoreError::Invalid(format!("{what} «{}»", v.chars().take(60).collect::<String>())))
}

/// Nombre de contenedor Docker: `[A-Za-z0-9][A-Za-z0-9_.-]{0,63}`.
pub fn container_name(s: &str) -> Result<&str> {
    let mut chars = s.chars();
    let ok = matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric())
        && s.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if ok { Ok(s) } else { bad("nombre de contenedor no válido", s) }
}

/// Referencia de imagen Docker (`repo/nombre:tag`, `nombre@sha256:...`).
pub fn docker_image(s: &str) -> Result<&str> {
    let mut chars = s.chars();
    let ok = matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric())
        && s.len() <= 200
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '/' | ':' | '@' | '-'));
    if ok { Ok(s) } else { bad("imagen Docker no válida", s) }
}

/// Nombre o UUID de una VM de VirtualBox. Los nombres pueden llevar espacios y acentos.
pub fn vbox_target(s: &str) -> Result<&str> {
    let ok = !s.is_empty()
        && s.chars().count() <= 128
        && !s.starts_with('-')
        && !s.chars().any(|c| c.is_control());
    if ok { Ok(s) } else { bad("máquina virtual no válida", s) }
}

/// Interfaz de red del host o nombre de una red de VirtualBox. Puede llevar espacios
/// (p. ej. `VirtualBox Host-Only Ethernet Adapter`), pero no empezar por `-` ni llevar controles.
pub fn host_iface(s: &str) -> Result<&str> {
    let ok = !s.is_empty()
        && s.chars().count() <= 128
        && !s.starts_with('-')
        && !s.chars().any(|c| c.is_control());
    if ok { Ok(s) } else { bad("interfaz de red no válida", s) }
}

/// Nombre de interfaz dentro de Linux (`IFNAMSIZ` = 16, incluido el NUL).
pub fn linux_iface(s: &str) -> Result<&str> {
    let mut chars = s.chars();
    let ok = matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric())
        && s.len() <= 15
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if ok { Ok(s) } else { bad("interfaz Linux no válida", s) }
}

/// Usuario Linux para `VBoxManage guestcontrol`.
pub fn guest_user(s: &str) -> Result<&str> {
    let mut chars = s.chars();
    let ok = matches!(chars.next(), Some(c) if c.is_ascii_alphanumeric() || c == '_')
        && s.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'));
    if ok { Ok(s) } else { bad("usuario de la VM no válido", s) }
}

/// Contraseña: no puede llevar controles (romperían el fichero temporal) ni ser enorme.
pub fn guest_password(s: &str) -> Result<&str> {
    let ok = s.chars().count() <= 1024 && !s.chars().any(|c| c.is_control());
    if ok { Ok(s) } else { bad("contraseña no válida", s) }
}

/// Redes gestionadas por VMLab: `vmlab-` + [a-z0-9-].
pub fn network_name(s: &str) -> Result<&str> {
    let ok = s.len() <= 40
        && s.strip_prefix("vmlab-").is_some_and(|r| {
            !r.is_empty() && r.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        });
    if ok { Ok(s) } else { bad("nombre de red no válido", s) }
}

/// `true` para redes nuevas (`vmlab-*`) y para las antiguas (`lablab-*`), que se
/// desconectan y limpian durante la migración. No se aceptan para crear redes.
pub fn managed_network(s: &str) -> bool {
    let valid = |r: &str| {
        !r.is_empty() && r.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    s.len() <= 40
        && (s.strip_prefix("vmlab-").is_some_and(valid) || s.strip_prefix("lablab-").is_some_and(valid))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_option_injection() {
        assert!(container_name("--privileged").is_err());
        assert!(docker_image("-v /:/host").is_err());
        assert!(vbox_target("--type").is_err());
    }

    #[test]
    fn rejects_shell_metacharacters() {
        for s in ["a;rm -rf /", "a b", "a$(id)", "a`id`", "a|b", "a&b", "a\nb"] {
            assert!(container_name(s).is_err(), "{s}");
        }
        assert!(docker_image("nginx; id").is_err());
    }

    #[test]
    fn accepts_normal_values() {
        assert!(container_name("web-01_a.b").is_ok());
        assert!(docker_image("ghcr.io/org/app:1.2@sha256").is_ok());
        assert!(vbox_target("Ubuntu Server 22.04 (lab)").is_ok());
        assert!(vbox_target("c0ffee00-1111-2222-3333-444455556666").is_ok());
        assert!(network_name("vmlab-0a1b2c3d").is_ok());
        assert!(network_name("bridge").is_err());
        assert!(network_name("vmlab-").is_err());
        assert!(network_name("lablab-0a1b2c3d").is_err(), "las redes antiguas no se crean");
        assert!(managed_network("vmlab-0a1b2c3d"));
        assert!(managed_network("lablab-0a1b2c3d"));
        assert!(!managed_network("bridge"));
    }

    #[test]
    fn validates_guest_inputs() {
        assert!(linux_iface("eth0").is_ok());
        assert!(linux_iface("enp0s3.100").is_ok());
        assert!(linux_iface("eth 0").is_err());
        assert!(linux_iface("-eth0").is_err());
        assert!(guest_user("ubuntu").is_ok());
        assert!(guest_user("--root").is_err());
        assert!(guest_password("una contraseña con espacios").is_ok());
        assert!(guest_password("a\nb").is_err());
        assert!(host_iface("VirtualBox Host-Only Ethernet Adapter").is_ok());
        assert!(host_iface("--evil").is_err());
    }
}
