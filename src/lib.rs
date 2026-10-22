//! marXol — núcleo de búsqueda y análisis on-chain de operadores de pump.fun:
//! decodificación, ingesta, almacenamiento y cálculo. La CLI (`src/main.rs`)
//! es una capa fina encima.

pub mod check;
pub mod entry;
pub mod entry2;
pub mod h1;
pub mod h1b;
pub mod idl;
pub mod ingest;
pub mod pump;
pub mod replica;
pub mod report;
pub mod rpc;
pub mod store;
pub mod tx;
pub mod v3;
pub mod verify;
