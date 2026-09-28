//! Lectura de transacciones y extracción de eventos Anchor `emit_cpi!`.
//!
//! Flujo (CLAUDE.md 5.3): `getTransaction` → inner instructions dirigidas al
//! programa pump.fun cuyo primer account es el `event_authority` PDA → datos
//! con prefijo `ANCHOR_EVENT_IX_TAG` → Borsh contra el IDL. No se parsean logs.

use crate::idl::{Decoded, Idl};
use crate::pump;
use anyhow::{bail, Context, Result};
use solana_commitment_config::CommitmentConfig;
use solana_rpc_client::rpc_client::{GetConfirmedSignaturesForAddress2Config, RpcClient};
use solana_rpc_client_api::config::RpcTransactionConfig;
use solana_signature::Signature;
use solana_transaction_status_client_types::option_serializer::OptionSerializer;
use solana_transaction_status_client_types::{
    EncodedTransaction, UiInstruction, UiMessage, UiTransactionEncoding,
};
use std::str::FromStr;

#[derive(Debug)]
pub struct TxEvents {
    pub signature: String,
    pub slot: u64,
    pub block_time: Option<i64>,
    pub failed: bool,
    /// Instrucciones pump.fun de nivel superior (por nombre del IDL).
    pub instructions: Vec<String>,
    pub events: Vec<Decoded>,
    /// Inner instructions de evento que no se pudieron decodificar.
    pub decode_errors: Vec<String>,
}

pub fn fetch_events(rpc: &RpcClient, idl: &Idl, sig: &str) -> Result<TxEvents> {
    let signature = Signature::from_str(sig).context("firma inválida")?;
    let tx = rpc
        .get_transaction_with_config(
            &signature,
            RpcTransactionConfig {
                encoding: Some(UiTransactionEncoding::Json),
                commitment: Some(CommitmentConfig::confirmed()),
                max_supported_transaction_version: Some(1),
            },
        )
        .with_context(|| format!("getTransaction {sig}"))?;

    let EncodedTransaction::Json(ui_tx) = &tx.transaction.transaction else {
        bail!("codificación de transacción inesperada");
    };
    let UiMessage::Raw(msg) = &ui_tx.message else {
        bail!("mensaje no es Raw (¿se pidió jsonParsed?)");
    };
    let meta = tx.transaction.meta.as_ref().context("transacción sin meta")?;

    // Claves completas: estáticas + cargadas desde lookup tables (tx v0).
    let mut keys = msg.account_keys.clone();
    if let OptionSerializer::Some(loaded) = &meta.loaded_addresses {
        keys.extend(loaded.writable.iter().cloned());
        keys.extend(loaded.readonly.iter().cloned());
    }

    let program = pump::PROGRAM_ID;
    let event_authority = pump::event_authority_pda().to_string();
    let key = |i: u8| keys.get(i as usize).map(String::as_str);

    let instructions = msg
        .instructions
        .iter()
        .filter(|ix| key(ix.program_id_index) == Some(program))
        .map(|ix| {
            bs58::decode(&ix.data)
                .into_vec()
                .ok()
                .and_then(|d| idl.instruction_by_disc(&d).map(|i| i.name.clone()))
                .unwrap_or_else(|| "<desconocida>".into())
        })
        .collect();

    let mut events = Vec::new();
    let mut decode_errors = Vec::new();
    if let OptionSerializer::Some(inner) = &meta.inner_instructions {
        for group in inner {
            for ix in &group.instructions {
                let UiInstruction::Compiled(c) = ix else { continue };
                if key(c.program_id_index) != Some(program)
                    || c.accounts.first().and_then(|&a| key(a)) != Some(event_authority.as_str())
                {
                    continue;
                }
                let data = bs58::decode(&c.data).into_vec().context("inner ix no es base58")?;
                match idl.decode_event_cpi(&data) {
                    Some(Ok(ev)) => events.push(ev),
                    Some(Err(e)) => decode_errors.push(format!("{e:#}")),
                    None => {}
                }
            }
        }
    }

    Ok(TxEvents {
        signature: sig.to_string(),
        slot: tx.slot,
        block_time: tx.block_time,
        failed: meta.err.is_some(),
        instructions,
        events,
        decode_errors,
    })
}

pub struct SigInfo {
    pub signature: String,
    pub failed: bool,
    pub slot: u64,
    pub block_time: Option<i64>,
}

/// Firmas recientes que tocan `address` (más recientes primero).
pub fn recent_signatures(
    rpc: &RpcClient,
    address: &str,
    limit: usize,
    before: Option<&str>,
) -> Result<Vec<SigInfo>> {
    let addr = solana_pubkey::Pubkey::from_str(address).context("dirección inválida")?;
    let sigs = rpc.get_signatures_for_address_with_config(
        &addr,
        GetConfirmedSignaturesForAddress2Config {
            before: before.map(Signature::from_str).transpose()?,
            until: None,
            limit: Some(limit),
            commitment: Some(CommitmentConfig::confirmed()),
        },
    )?;
    Ok(sigs
        .into_iter()
        .map(|s| SigInfo {
            failed: s.err.is_some(),
            slot: s.slot,
            block_time: s.block_time,
            signature: s.signature,
        })
        .collect())
}
