use std::sync::atomic::{AtomicBool, Ordering};
use vmlab_core::tools::Tools;
use tokio::sync::RwLock;

/// Estado compartido de la app: rutas de los binarios detectados.
pub struct AppState {
    pub tools: RwLock<Tools>,
    /// `true` si VMLab arrancó Docker Desktop y, por tanto, debe pararlo al salir.
    docker_owned: AtomicBool,
}

impl AppState {
    pub fn new() -> Self {
        Self { tools: RwLock::new(Tools::detect()), docker_owned: AtomicBool::new(false) }
    }

    /// Copia barata (dos rutas) para no mantener el lock durante la ejecución de un comando.
    pub async fn tools(&self) -> Tools {
        self.tools.read().await.clone()
    }

    /// Registra que Docker Desktop lo arrancó VMLab (no que ya estuviera en marcha).
    pub fn own_docker(&self) {
        self.docker_owned.store(true, Ordering::SeqCst);
    }

    /// Devuelve `true` una sola vez si VMLab arrancó Docker, para no pararlo dos veces.
    pub fn take_docker_ownership(&self) -> bool {
        self.docker_owned.swap(false, Ordering::SeqCst)
    }
}
