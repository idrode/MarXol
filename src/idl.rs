//! Decodificador Borsh genérico dirigido por el IDL Anchor de pump.fun.
//!
//! No se escriben structs a mano por evento: el IDL (`idl/pump.json`) es la
//! fuente de verdad y cambia con frecuencia. El decodificador es tolerante a
//! datos más cortos que el IDL actual (eventos/cuentas históricos emitidos
//! antes de que se añadieran campos al final): los campos que faltan se
//! reportan en `missing`, nunca se inventan valores por defecto.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{Map, Value};
use std::collections::HashMap;

pub const PUMP_IDL_JSON: &str = include_str!("../idl/pump.json");

/// Prefijo de las inner instructions de `emit_cpi!`: sha256("anchor:event")[..8].
pub const ANCHOR_EVENT_IX_TAG: [u8; 8] = [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];

#[derive(Debug, Deserialize)]
struct RawIdl {
    address: String,
    instructions: Vec<RawNamed>,
    accounts: Vec<RawNamed>,
    events: Vec<RawNamed>,
    types: Vec<RawTypeDef>,
}

#[derive(Debug, Deserialize)]
struct RawNamed {
    name: String,
    discriminator: Vec<u8>,
}

#[derive(Debug, Deserialize)]
struct RawTypeDef {
    name: String,
    #[serde(rename = "type")]
    ty: RawTypeBody,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum RawTypeBody {
    Struct {
        #[serde(default)]
        fields: Vec<RawStructField>,
    },
    Enum { variants: Vec<RawVariant> },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawStructField {
    Named(RawField),
    Tuple(Value),
}

#[derive(Debug, Deserialize)]
struct RawField {
    name: String,
    #[serde(rename = "type")]
    ty: Value,
}

#[derive(Debug, Deserialize)]
struct RawVariant {
    name: String,
    #[serde(default)]
    fields: Vec<RawStructField>,
}

#[derive(Debug, Clone)]
pub enum Ty {
    Bool,
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
    Pubkey,
    String,
    Bytes,
    Vec(Box<Ty>),
    Option(Box<Ty>),
    Array(Box<Ty>, usize),
    Defined(String),
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub ty: Ty,
}

#[derive(Debug, Clone)]
pub enum TypeDef {
    Struct(Vec<Field>),
    Enum(Vec<(String, Vec<Field>)>),
}

#[derive(Debug, Clone)]
pub struct Discriminated {
    pub name: String,
    pub discriminator: [u8; 8],
}

#[derive(Debug)]
pub struct Idl {
    pub address: String,
    pub instructions: Vec<Discriminated>,
    pub accounts: Vec<Discriminated>,
    pub events: Vec<Discriminated>,
    pub types: HashMap<String, TypeDef>,
}

/// Resultado de decodificar un struct. `missing` lista campos del IDL actual
/// que no estaban en los bytes (datos anteriores a la versión del IDL).
#[derive(Debug, Clone)]
pub struct Decoded {
    pub name: String,
    pub fields: Map<String, Value>,
    pub missing: Vec<String>,
    pub trailing_bytes: usize,
}

impl Decoded {
    pub fn get(&self, k: &str) -> Option<&Value> {
        self.fields.get(k)
    }
    pub fn str(&self, k: &str) -> Option<&str> {
        self.get(k).and_then(Value::as_str)
    }
    pub fn u64(&self, k: &str) -> Option<u64> {
        self.get(k).and_then(Value::as_u64)
    }
    pub fn i64(&self, k: &str) -> Option<i64> {
        self.get(k).and_then(Value::as_i64)
    }
    pub fn bool(&self, k: &str) -> Option<bool> {
        self.get(k).and_then(Value::as_bool)
    }
    pub fn to_json(&self) -> Value {
        let mut m = Map::new();
        m.insert("name".into(), Value::String(self.name.clone()));
        m.insert("fields".into(), Value::Object(self.fields.clone()));
        if !self.missing.is_empty() {
            m.insert("missing".into(), self.missing.clone().into());
        }
        Value::Object(m)
    }
}

fn parse_ty(v: &Value) -> Result<Ty> {
    Ok(match v {
        Value::String(s) => match s.as_str() {
            "bool" => Ty::Bool,
            "u8" => Ty::U8,
            "u16" => Ty::U16,
            "u32" => Ty::U32,
            "u64" => Ty::U64,
            "u128" => Ty::U128,
            "i8" => Ty::I8,
            "i16" => Ty::I16,
            "i32" => Ty::I32,
            "i64" => Ty::I64,
            "i128" => Ty::I128,
            "pubkey" | "publicKey" => Ty::Pubkey,
            "string" => Ty::String,
            "bytes" => Ty::Bytes,
            other => bail!("tipo primitivo desconocido en IDL: {other}"),
        },
        Value::Object(o) => {
            if let Some(inner) = o.get("vec") {
                Ty::Vec(Box::new(parse_ty(inner)?))
            } else if let Some(inner) = o.get("option") {
                Ty::Option(Box::new(parse_ty(inner)?))
            } else if let Some(arr) = o.get("array") {
                let inner = parse_ty(&arr[0])?;
                let n = arr[1].as_u64().context("longitud de array no numérica")? as usize;
                Ty::Array(Box::new(inner), n)
            } else if let Some(d) = o.get("defined") {
                let name = d
                    .get("name")
                    .and_then(Value::as_str)
                    .or_else(|| d.as_str())
                    .context("defined sin nombre")?;
                Ty::Defined(name.to_string())
            } else {
                bail!("tipo compuesto desconocido en IDL: {v}")
            }
        }
        _ => bail!("tipo inválido en IDL: {v}"),
    })
}

fn parse_fields(raw: &[RawStructField]) -> Result<Vec<Field>> {
    raw.iter()
        .enumerate()
        .map(|(i, f)| match f {
            RawStructField::Named(n) => Ok(Field { name: n.name.clone(), ty: parse_ty(&n.ty)? }),
            RawStructField::Tuple(t) => Ok(Field { name: i.to_string(), ty: parse_ty(t)? }),
        })
        .collect()
}

fn disc(v: &[u8], name: &str) -> Result<[u8; 8]> {
    v.try_into().with_context(|| format!("discriminador de {name} no tiene 8 bytes"))
}

impl Idl {
    pub fn pump() -> Result<Self> {
        Self::parse(PUMP_IDL_JSON)
    }

    pub fn parse(json: &str) -> Result<Self> {
        let raw: RawIdl = serde_json::from_str(json).context("IDL JSON inválido")?;
        let mut types = HashMap::new();
        for t in &raw.types {
            let def = match &t.ty {
                RawTypeBody::Struct { fields } => TypeDef::Struct(parse_fields(fields)?),
                RawTypeBody::Enum { variants } => TypeDef::Enum(
                    variants
                        .iter()
                        .map(|v| Ok((v.name.clone(), parse_fields(&v.fields)?)))
                        .collect::<Result<_>>()?,
                ),
            };
            types.insert(t.name.clone(), def);
        }
        let conv = |v: &[RawNamed]| -> Result<Vec<Discriminated>> {
            v.iter()
                .map(|n| {
                    Ok(Discriminated {
                        name: n.name.clone(),
                        discriminator: disc(&n.discriminator, &n.name)?,
                    })
                })
                .collect()
        };
        Ok(Idl {
            address: raw.address,
            instructions: conv(&raw.instructions)?,
            accounts: conv(&raw.accounts)?,
            events: conv(&raw.events)?,
            types,
        })
    }

    pub fn struct_fields(&self, name: &str) -> Option<&[Field]> {
        match self.types.get(name)? {
            TypeDef::Struct(f) => Some(f),
            TypeDef::Enum(_) => None,
        }
    }

    /// Decodifica `data` (sin discriminador) como el struct `type_name`.
    pub fn decode_struct(&self, type_name: &str, data: &[u8]) -> Result<Decoded> {
        let fields = self
            .struct_fields(type_name)
            .with_context(|| format!("tipo {type_name} no es un struct del IDL"))?;
        let mut r = Reader { buf: data, pos: 0 };
        let mut out = Map::new();
        let mut missing = Vec::new();
        for (i, f) in fields.iter().enumerate() {
            if r.remaining() == 0 {
                missing.extend(fields[i..].iter().map(|f| f.name.clone()));
                break;
            }
            let v = self
                .decode_value(&f.ty, &mut r)
                .with_context(|| format!("{type_name}.{} (offset {})", f.name, r.pos))?;
            out.insert(f.name.clone(), v);
        }
        Ok(Decoded {
            name: type_name.to_string(),
            fields: out,
            missing,
            trailing_bytes: r.remaining(),
        })
    }

    /// Decodifica una cuenta completa (8 bytes de discriminador + Borsh).
    pub fn decode_account(&self, data: &[u8]) -> Result<Decoded> {
        let d: [u8; 8] = data.get(..8).context("cuenta de menos de 8 bytes")?.try_into()?;
        let acc = self
            .accounts
            .iter()
            .find(|a| a.discriminator == d)
            .context("discriminador de cuenta no reconocido en el IDL")?;
        self.decode_struct(&acc.name, &data[8..])
    }

    /// Si `data` es una inner instruction de `emit_cpi!`, decodifica el evento.
    pub fn decode_event_cpi(&self, data: &[u8]) -> Option<Result<Decoded>> {
        if data.len() < 16 || data[..8] != ANCHOR_EVENT_IX_TAG {
            return None;
        }
        let d: [u8; 8] = data[8..16].try_into().ok()?;
        let Some(ev) = self.events.iter().find(|e| e.discriminator == d) else {
            return Some(Err(anyhow::anyhow!(
                "evento con discriminador desconocido {:02x?} (¿IDL desactualizado?)",
                d
            )));
        };
        Some(self.decode_struct(&ev.name, &data[16..]))
    }

    /// Identifica la instrucción de nivel superior por su discriminador.
    pub fn instruction_by_disc(&self, data: &[u8]) -> Option<&Discriminated> {
        let d: [u8; 8] = data.get(..8)?.try_into().ok()?;
        self.instructions.iter().find(|i| i.discriminator == d)
    }

    fn decode_value(&self, ty: &Ty, r: &mut Reader) -> Result<Value> {
        Ok(match ty {
            Ty::Bool => match r.take(1)?[0] {
                0 => Value::Bool(false),
                1 => Value::Bool(true),
                b => bail!("bool inválido: {b}"),
            },
            Ty::U8 => r.take(1)?[0].into(),
            Ty::I8 => (r.take(1)?[0] as i8).into(),
            Ty::U16 => u16::from_le_bytes(r.arr()?).into(),
            Ty::I16 => i16::from_le_bytes(r.arr()?).into(),
            Ty::U32 => u32::from_le_bytes(r.arr()?).into(),
            Ty::I32 => i32::from_le_bytes(r.arr()?).into(),
            Ty::U64 => u64::from_le_bytes(r.arr()?).into(),
            Ty::I64 => i64::from_le_bytes(r.arr()?).into(),
            // u128/i128 como string: serde_json no los representa sin pérdida.
            Ty::U128 => u128::from_le_bytes(r.arr()?).to_string().into(),
            Ty::I128 => i128::from_le_bytes(r.arr()?).to_string().into(),
            Ty::Pubkey => bs58::encode(r.take(32)?).into_string().into(),
            Ty::String => {
                let n = u32::from_le_bytes(r.arr()?) as usize;
                String::from_utf8_lossy(r.take(n)?).into_owned().into()
            }
            Ty::Bytes => {
                let n = u32::from_le_bytes(r.arr()?) as usize;
                bs58::encode(r.take(n)?).into_string().into()
            }
            Ty::Vec(inner) => {
                let n = u32::from_le_bytes(r.arr()?) as usize;
                if n > r.remaining() {
                    bail!("vec con longitud {n} mayor que los bytes restantes");
                }
                Value::Array((0..n).map(|_| self.decode_value(inner, r)).collect::<Result<_>>()?)
            }
            Ty::Array(inner, n) => {
                Value::Array((0..*n).map(|_| self.decode_value(inner, r)).collect::<Result<_>>()?)
            }
            Ty::Option(inner) => match r.take(1)?[0] {
                0 => Value::Null,
                1 => self.decode_value(inner, r)?,
                b => bail!("tag de option inválido: {b}"),
            },
            Ty::Defined(name) => match self.types.get(name) {
                Some(TypeDef::Struct(fields)) => self.decode_fields(fields, r)?,
                Some(TypeDef::Enum(variants)) => {
                    let idx = r.take(1)?[0] as usize;
                    let (vname, vfields) =
                        variants.get(idx).with_context(|| format!("variante {idx} de {name}"))?;
                    if vfields.is_empty() {
                        Value::String(vname.clone())
                    } else {
                        let mut m = Map::new();
                        m.insert(vname.clone(), self.decode_fields(vfields, r)?);
                        Value::Object(m)
                    }
                }
                None => bail!("tipo definido {name} no existe en el IDL"),
            },
        })
    }

    fn decode_fields(&self, fields: &[Field], r: &mut Reader) -> Result<Value> {
        let mut m = Map::new();
        for f in fields {
            m.insert(f.name.clone(), self.decode_value(&f.ty, r)?);
        }
        Ok(Value::Object(m))
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        if n > self.remaining() {
            bail!("fin de datos: se piden {n} bytes, quedan {}", self.remaining());
        }
        let s = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn arr<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idl_parsea_y_tiene_program_id_esperado() {
        let idl = Idl::pump().unwrap();
        assert_eq!(idl.address, crate::pump::PROGRAM_ID);
        for ev in crate::pump::IDENTITY_EVENTS {
            assert!(idl.events.iter().any(|e| e.name == *ev), "falta evento {ev}");
        }
    }

    #[test]
    fn decodifica_set_creator_event_y_tolera_campos_finales_ausentes() {
        let idl = Idl::pump().unwrap();
        let ev = idl.events.iter().find(|e| e.name == "SetCreatorEvent").unwrap();
        let mut data = ANCHOR_EVENT_IX_TAG.to_vec();
        data.extend_from_slice(&ev.discriminator);
        data.extend_from_slice(&1_700_000_000i64.to_le_bytes());
        data.extend_from_slice(&[1u8; 32]); // mint
        // bonding_curve y creator ausentes: simula un evento histórico truncado
        let d = idl.decode_event_cpi(&data).unwrap().unwrap();
        assert_eq!(d.name, "SetCreatorEvent");
        assert_eq!(d.i64("timestamp"), Some(1_700_000_000));
        assert_eq!(d.missing, vec!["bonding_curve", "creator"]);
    }

    #[test]
    fn datos_que_no_son_evento_se_ignoran() {
        let idl = Idl::pump().unwrap();
        assert!(idl.decode_event_cpi(&[0u8; 40]).is_none());
    }
}
