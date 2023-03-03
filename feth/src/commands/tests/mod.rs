mod basic;
mod long_run;

pub use basic::*;
pub use long_run::*;
use std::fmt::Formatter;

#[derive(Debug)]
pub enum TxnsType {
    Eth,
    Utxo,
    Prism,
    Mixed(u64, u64, u64),
}

impl std::fmt::Display for TxnsType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Eth => write!(f, "eth"),
            Self::Utxo => write!(f, "utxo"),
            Self::Prism => write!(f, "prism"),
            Self::Mixed(x, y, z) => write!(f, "mixed,{x},{y},{z}"),
        }
    }
}

impl std::str::FromStr for TxnsType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().trim() {
            "eth" => Ok(Self::Eth),
            "utxo" => Ok(Self::Utxo),
            "prism" => Ok(Self::Prism),
            n => {
                let segs = n.splitn(4, ',').collect::<Vec<&str>>();
                if segs.len() != 4 || unsafe { segs.get_unchecked(0) != &"mixed" } {
                    return Err("Invalid Type".to_string());
                }
                let mut params = [0u64; 3];
                for (i, n) in segs.iter().skip(1).enumerate() {
                    unsafe {
                        *params.get_unchecked_mut(i) = n.parse::<u64>().map_err(|o| o.to_string())?;
                    }
                }
                Ok(Self::Mixed(
                    unsafe { *params.get_unchecked(0) },
                    unsafe { *params.get_unchecked(1) },
                    unsafe { *params.get_unchecked(2) },
                ))
            }
        }
    }
}
