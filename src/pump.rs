//! Constantes, PDAs y matemática de la bonding curve de pump.fun.
//!
//! Los parámetros económicos NO se hardcodean: se leen de la cuenta `Global`
//! (ver CLAUDE.md sección 4). Aquí solo viven direcciones verificadas y
//! fórmulas que operan sobre esos parámetros leídos.

use crate::idl::Decoded;
use anyhow::{Context, Result};
use solana_pubkey::Pubkey;
use std::str::FromStr;

pub const PROGRAM_ID: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";
pub const GLOBAL_PDA: &str = "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf";
pub const LAMPORTS_PER_SOL: f64 = 1_000_000_000.0;

/// Los tres eventos que definen la identidad de "operador" de un token a lo
/// largo del tiempo (CLAUDE.md 5.2). Nunca se colapsan en un único campo.
pub const IDENTITY_EVENTS: &[&str] =
    &["CreateEvent", "SetCreatorEvent", "MigrateBondingCurveCreatorEvent"];

/// Eventos de ciclo de vida útiles para medir "éxito" (H1).
pub const LIFECYCLE_EVENTS: &[&str] = &["CompleteEvent", "CompletePumpAmmMigrationEvent"];

pub fn program_id() -> Pubkey {
    Pubkey::from_str(PROGRAM_ID).unwrap()
}

pub fn global_pda() -> Pubkey {
    Pubkey::find_program_address(&[b"global"], &program_id()).0
}

pub fn event_authority_pda() -> Pubkey {
    Pubkey::find_program_address(&[b"__event_authority"], &program_id()).0
}

/// Autoridad del mint durante la bonding curve. Solo la tocan las
/// instrucciones de creación: sus firmas son una vía barata para listar
/// creaciones sin recorrer todo el tráfico del programa.
pub fn mint_authority_pda() -> Pubkey {
    Pubkey::find_program_address(&[b"mint-authority"], &program_id()).0
}

pub fn bonding_curve_pda(mint: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[b"bonding-curve", mint.as_ref()], &program_id()).0
}

/// Parámetros de curva leídos de `Global`, y lo que implican para la
/// graduación. Curva de producto constante sobre reservas virtuales:
/// la curva se completa cuando se han vendido `initial_real_token_reserves`.
#[derive(Debug, Clone)]
pub struct Graduation {
    /// SOL real (lamports) que acumula la curva al completarse, sin fees.
    pub real_sol_at_completion: u64,
    /// Precio (SOL por token entero, 6 decimales) inicial y al completarse.
    pub start_price_sol: f64,
    pub final_price_sol: f64,
    /// Market cap en SOL (precio × supply total) inicial y al completarse.
    pub start_mcap_sol: f64,
    pub final_mcap_sol: f64,
}

pub const TOKEN_DECIMALS: u32 = 6;

impl Graduation {
    pub fn from_global(g: &Decoded) -> Result<Self> {
        let f = |k: &str| g.u64(k).with_context(|| format!("Global.{k} ausente"));
        let vt0 = f("initial_virtual_token_reserves")?;
        let vs0 = f("initial_virtual_sol_reserves")?;
        let rt0 = f("initial_real_token_reserves")?;
        let supply = f("token_total_supply")?;
        Ok(Self::compute(vt0, vs0, rt0, supply))
    }

    pub fn compute(vt0: u64, vs0: u64, rt0: u64, supply: u64) -> Self {
        let k = vt0 as u128 * vs0 as u128;
        let vt_end = (vt0 - rt0) as u128;
        // El programa redondea hacia arriba lo que paga el comprador.
        let vs_end = k.div_ceil(vt_end);
        let real_sol = (vs_end - vs0 as u128) as u64;
        let unit = 10f64.powi(TOKEN_DECIMALS as i32);
        let price = |vs: f64, vt: f64| (vs / LAMPORTS_PER_SOL) / (vt / unit);
        let start_price = price(vs0 as f64, vt0 as f64);
        let final_price = price(vs_end as f64, vt_end as f64);
        let supply_tokens = supply as f64 / unit;
        Graduation {
            real_sol_at_completion: real_sol,
            start_price_sol: start_price,
            final_price_sol: final_price,
            start_mcap_sol: start_price * supply_tokens,
            final_mcap_sol: final_price * supply_tokens,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdas_derivadas_coinciden_con_las_documentadas() {
        assert_eq!(global_pda().to_string(), GLOBAL_PDA);
    }

    #[test]
    fn graduacion_con_parametros_historicos_conocidos() {
        // Parámetros clásicos de pump.fun (1.073B virtual, 30 SOL virtual,
        // 793.1M reales, 1B supply): la curva se completa con ~85 SOL.
        let g = Graduation::compute(
            1_073_000_000_000_000,
            30_000_000_000,
            793_100_000_000_000,
            1_000_000_000_000_000,
        );
        let sol = g.real_sol_at_completion as f64 / LAMPORTS_PER_SOL;
        assert!((84.0..86.0).contains(&sol), "{sol}");
    }
}
