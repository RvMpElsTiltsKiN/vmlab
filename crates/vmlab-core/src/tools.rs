//! Localiza `VBoxManage` y `docker` sin pasar por un shell.
//! En Windows solo se aceptan `.exe` (nunca `.cmd`/`.bat`, que reintroducen un intérprete).
use crate::error::{CoreError, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tools {
    pub vboxmanage: Option<PathBuf>,
    pub docker: Option<PathBuf>,
}

impl Tools {
    pub fn detect() -> Self {
        Self {
            vboxmanage: find("VBoxManage", &vbox_candidates()),
            docker: find("docker", &docker_candidates()),
        }
    }

    pub fn vbox(&self) -> Result<&Path> {
        self.vboxmanage.as_deref().ok_or(CoreError::ToolMissing("VBoxManage"))
    }

    pub fn docker(&self) -> Result<&Path> {
        self.docker.as_deref().ok_or(CoreError::ToolMissing("docker"))
    }
}

fn exe(name: &str) -> String {
    if cfg!(windows) { format!("{name}.exe") } else { name.to_string() }
}

fn find(name: &str, candidates: &[PathBuf]) -> Option<PathBuf> {
    find_in_path(name).or_else(|| candidates.iter().find(|p| p.is_file()).cloned())
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let file = exe(name);
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(&file))
        .find(|p| p.is_file())
}

fn vbox_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(dir) = std::env::var_os("VBOX_MSI_INSTALL_PATH") {
        v.push(PathBuf::from(dir).join("VBoxManage.exe"));
    }
    if let Some(pf) = std::env::var_os("ProgramFiles") {
        v.push(PathBuf::from(pf).join("Oracle").join("VirtualBox").join("VBoxManage.exe"));
    }
    v.push("/usr/local/bin/VBoxManage".into());
    v.push("/Applications/VirtualBox.app/Contents/MacOS/VBoxManage".into());
    v
}

fn docker_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    // Docker Desktop 4.9x se instala por usuario en %LOCALAPPDATA%\Programs\DockerDesktop.
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        v.push(
            PathBuf::from(local)
                .join("Programs")
                .join("DockerDesktop")
                .join("resources")
                .join("bin")
                .join("docker.exe"),
        );
    }
    if let Some(pf) = std::env::var_os("ProgramFiles") {
        v.push(PathBuf::from(pf).join("Docker").join("Docker").join("resources").join("bin").join("docker.exe"));
    }
    v.push("/usr/local/bin/docker".into());
    v.push("/Applications/Docker.app/Contents/Resources/bin/docker".into());
    v
}
