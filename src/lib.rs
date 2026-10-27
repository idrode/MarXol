//! marXol — núcleo de búsqueda y análisis on-chain de operadores de pump.fun:
//! decodificación, ingesta, almacenamiento y cálculo. La CLI (`src/main.rs`)
//! es una capa fina encima.

pub mod banco;
pub mod check;
pub mod concentracion;
pub mod collect;
pub mod entry;
pub mod entry2;
pub mod h1;
pub mod h1b;
pub mod historial;
pub mod idl;
pub mod ingest;
pub mod ledger;
pub mod pump;
pub mod replica;
pub mod report;
pub mod rpc;
pub mod store;
pub mod tx;
pub mod v3;
pub mod verify;
pub mod vivo;
