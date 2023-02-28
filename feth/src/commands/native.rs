use crate::Network;
use agent::{
    error::{Error, Result},
    native::{restore_fra_keypair, restore_xfr_pk_from_str, transfer, NativeOp},
};
use std::path::PathBuf;

pub fn execute_native(
    network: &Network,
    op: &NativeOp,
    secret: &PathBuf,
    target_addr: &str,
    amount: u64,
) -> Result<()> {
    let base = network.base_url();
    match *op {
        NativeOp::Transfer => {
            let kp = restore_fra_keypair(secret)?;
            let target = restore_xfr_pk_from_str(target_addr)?;
            transfer(base.as_str(), kp, target, amount)
        }
        _ => Err(Error::Native("Unsupported operation".to_string())),
    }
}
