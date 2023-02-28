use crate::Network;
use agent::error::{Error, Result};
use agent::native::*;
use std::path::PathBuf;
use web3::types::H160;

pub fn execute_prism(network: &Network, op: &PrismOp, secret: &PathBuf, target: &str, amount: &u64) -> Result<()> {
    let base = network.base_url();
    match *op {
        PrismOp::Deposit => {
            let kp = restore_fra_keypair(secret)?;
            let target = target.parse::<H160>().map_err(|o| Error::Prism(o.to_string()))?;
            deposit(base.as_str(), kp, target, *amount)?;
        }
        PrismOp::WithDraw => {
            let kp = restore_eth_keypair(secret)?;
            let target = restore_xfr_pk_from_str(target)?;
            withdraw(base.as_str(), kp, target, *amount)?;
        }
    }
    Ok(())
}
