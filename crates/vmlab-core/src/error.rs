use thiserror::Error;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("entrada no válida: {0}")]
    Invalid(String),
    #[error("{0} no está instalado o no se encuentra en el PATH")]
    ToolMissing(&'static str),
    #[error("no se pudo ejecutar {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} tardó más de {secs}s y fue cancelado")]
    Timeout { program: String, secs: u64 },
    #[error("{program} terminó con error ({code}): {stderr}")]
    Failed {
        program: String,
        code: i32,
        stderr: String,
    },
    #[error("error de E/S: {0}")]
    Io(String),
}

pub type Result<T> = std::result::Result<T, CoreError>;
