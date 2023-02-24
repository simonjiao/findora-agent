use super::Network;
use agent::{
    utils::{build_source_keys, display_info},
    TestClient,
};
use log::{error, info};
use rayon::prelude::*;
use std::{
    ops::Add,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering::Relaxed},
        Arc,
    },
    time::Duration,
};
use web3::types::U64;

pub fn long_run_test<P>(
    network: &Network,
    max_threads: &u64,
    source: P,
    timeout: &u64,
    count: &u64,
    check_balance: &bool,
    source_count: &Option<u64>,
    delay: &u64,
) -> anyhow::Result<()>
where
    P: AsRef<Path>,
{
    let max_par = *max_threads;
    let source_file = source;
    let timeout = Some(*timeout);
    let count = *count;

    let target_amount = web3::types::U256::exp10(14); // 0.0001 eth

    let url = network.get_url();
    let client = Arc::new(TestClient::setup(Some(url), timeout));

    let (chain_id, gas_price) = display_info(client.clone());

    info!("preparing test data, it could take several minutes...");
    let source_keys = build_source_keys(
        client.clone(),
        source_file,
        *check_balance,
        target_amount,
        *source_count,
        count,
        max_par,
    );
    if count == 0 || source_keys.is_empty() {
        error!("Not enough sufficient source accounts or target accounts, skipped.");
        return Ok(());
    }

    let total_succeed = AtomicU64::new(0);

    info!("starting tests...");
    let start_height = client.block_number().unwrap();
    let mut last_height = start_height;
    for round in 0..u64::MAX {
        let now = std::time::Instant::now();
        source_keys.par_iter().for_each(|(source, address, targets)| {
            let target = targets.get((round % count) as usize).unwrap();
            if let Some(nonce) = client.pending_nonce(*address) {
                if let Ok(_hash) =
                    client.distribution_simple(source, target, Some(chain_id), Some(gas_price), Some(nonce))
                {
                    total_succeed.fetch_add(1, Relaxed);
                }
            }
        });

        let elapsed = now.elapsed().as_secs();
        info!("round {} time {}", round, elapsed);

        loop {
            let current = client.block_number().unwrap();
            if current >= last_height.add(U64::from(*delay)) {
                last_height = current;
                break;
            } else {
                std::thread::sleep(Duration::from_millis(1000));
            }
        }
    }
    // we'll never reach here, just to silence the compiler
    Ok(())
}
