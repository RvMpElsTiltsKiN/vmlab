//! Núcleo de VMLab. Todo lo que toca procesos del sistema vive aquí, aislado de Tauri,
//! de modo que se puede probar con `cargo test` sin entorno gráfico.
//!
//! Reglas de seguridad (ver `runner` y `validate`):
//! 1. Nunca se invoca un shell: cada comando es `programa + argv[]`.
//! 2. Toda cadena que viene de la UI se valida antes de convertirse en argumento.
//! 3. Todo proceso tiene timeout y se mata si se cancela (`kill_on_drop`).
pub mod connectivity;
pub mod docker;
pub mod docker_desktop;
pub mod engine;
pub mod error;
pub mod guest;
pub mod model;
pub mod runner;
pub mod tools;
pub mod topology;
pub mod validate;
pub mod vbox;
