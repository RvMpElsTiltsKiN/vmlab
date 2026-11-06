use vmlab_core::tools::Tools;
use tokio::sync::RwLock;

/// Estado compartido de la app: rutas de los binarios detectados.
pub struct AppState {
    pub tools: RwLock<Tools>,
}

impl AppState {
    pub fn new() -> Self {
        Self { tools: RwLock::new(Tools::detect()) }
    }

    /// Copia barata (dos rutas) para no mantener el lock durante la ejecución de un comando.
    pub async fn tools(&self) -> Tools {
        self.tools.read().await.clone()
    }
}
