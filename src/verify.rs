//! `marxol verify` y lectura de cuentas del programa.

use crate::idl::{Decoded, Idl};
use crate::pump::{self, Graduation};
use crate::report::print_graduation;
use crate::rpc;
use anyhow::{bail, Context, Result};
use solana_pubkey::Pubkey;
use solana_rpc_client::rpc_client::RpcClient;
use std::str::FromStr;

pub fn read_account(rpc: &RpcClient, budget: &rpc::Budget, idl: &Idl, address: &str) -> Result<Decoded> {
    let pk = Pubkey::from_str(address)?;
    budget.acquire(rpc::Method::GetAccountInfo)?;
    let acc = rpc.get_account(&pk).with_context(|| format!("getAccountInfo {address}"))?;
    anyhow::ensure!(
        acc.owner == pump::program_id(),
        "la cuenta {address} no pertenece al programa pump.fun (owner {})",
        acc.owner
    );
    idl.decode_account(&acc.data)
}

pub fn verify(rpc: &RpcClient, budget: &rpc::Budget, idl: &Idl) -> Result<()> {
    let ok = |b: bool| if b { "OK  " } else { "FALLO" };

    budget.acquire(rpc::Method::GetAccountInfo)?;
    let prog = rpc.get_account(&pump::program_id())?;
    println!(
        "[{}] program {} ejecutable={} owner={}",
        ok(prog.executable),
        pump::PROGRAM_ID,
        prog.executable,
        prog.owner
    );

    println!(
        "[{}] IDL embebido: address={} ({} instrucciones, {} cuentas, {} eventos, {} tipos)",
        ok(idl.address == pump::PROGRAM_ID),
        idl.address,
        idl.instructions.len(),
        idl.accounts.len(),
        idl.events.len(),
        idl.types.len()
    );

    let derived = pump::global_pda();
    println!("[{}] Global PDA derivada = {derived}", ok(derived.to_string() == pump::GLOBAL_PDA));
    println!("      mint-authority PDA = {} (para `index --address`)", pump::mint_authority_pda());

    let g = read_account(rpc, budget, idl, pump::GLOBAL_PDA)?;
    let exact = global_exact(&g);
    println!(
        "[{}] Global decodificado ({} campos, faltan {:?}, {} bytes sobrantes)",
        ok(exact.is_ok()),
        g.fields.len(),
        g.missing,
        g.trailing_bytes
    );
    print_graduation(&Graduation::from_global(&g)?);
    exact
}

/// `Global` tiene que decodificar exactamente con el IDL embebido. Si sobran
/// bytes, el programa desplegado tiene campos que el IDL no describe (IDL
/// desactualizado); si faltan campos, el IDL es más nuevo que la cuenta.
fn global_exact(g: &Decoded) -> Result<()> {
    if g.trailing_bytes > 0 {
        bail!(
            "Global tiene {} bytes que el IDL embebido no describe: el programa desplegado es más nuevo que \
             idl/pump.json. Actualizar el IDL (y verificarlo) antes de fiarse de la decodificación",
            g.trailing_bytes
        );
    }
    if !g.missing.is_empty() {
        bail!(
            "a Global le faltan campos del IDL embebido {:?}: el IDL es más nuevo que la cuenta desplegada",
            g.missing
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn global(trailing_bytes: usize, missing: &[&str]) -> Decoded {
        let mut fields = serde_json::Map::new();
        fields.insert("initialized".into(), true.into());
        Decoded {
            name: "Global".into(),
            fields,
            missing: missing.iter().map(|s| s.to_string()).collect(),
            trailing_bytes,
        }
    }

    #[test]
    fn verify_exige_que_global_decodifique_exacto() {
        assert!(global_exact(&global(0, &[])).is_ok());
        // Lo que pasó con el IDL del 2026-09-28: 1 byte sobrante (max_curve_depth).
        let e = format!("{:#}", global_exact(&global(1, &[])).unwrap_err());
        assert!(e.contains("1 bytes que el IDL embebido no describe") && e.contains("idl/pump.json"), "{e}");
        let e = format!("{:#}", global_exact(&global(0, &["max_curve_depth"])).unwrap_err());
        assert!(e.contains("max_curve_depth"), "{e}");
    }
}

