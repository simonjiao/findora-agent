use crate::commands::{common::read_mnemonics, Network, TxnsType};
use agent::{
    error::{Error, Result},
    native::{gen_one_mnemonic_default, restore_keypair_from_mnemonic_default, transfer, TX_FEE_MIN},
    one_eth_key, TestClient,
};
use rayon::prelude::*;
use std::{
    ops::Mul,
    path::PathBuf,
    str::FromStr,
    sync::{
        atomic::{AtomicU64, Ordering::Relaxed},
        Arc,
    },
    time::Duration,
};
use tendermint_rpc::Client;
use tracing::{debug, error, info};
use web3::types::{Address, U256};

fn calc_pool_size(keys: usize, max_threads: usize) -> usize {
    if keys > max_threads {
        max_threads
    } else {
        keys
    }
}

#[allow(clippy::too_many_arguments)]
pub fn basic_test(
    network: &Network,
    txns_type: &TxnsType,
    delay: u64,
    max_par: u64,
    count: u64,
    source_file: &PathBuf,
    timeout: Option<u64>,
    check_balance: bool,
) -> Result<()> {
    match *txns_type {
        TxnsType::Eth => basic_eth_test(network, delay, max_par, count, source_file, timeout, check_balance),
        TxnsType::Utxo => basic_utxo_test(network, max_par, count, source_file),
        _ => {
            todo!();
        }
    }
}

fn basic_utxo_test(network: &Network, _max_threads: u64, count: u64, source_file: &PathBuf) -> Result<()> {
    // 1. load accounts from source_file
    // 2. generate `count` targets per source  account
    // 3. send them in parallel
    // 4. wait for a block and send again
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()
        .unwrap();

    let source_kps = runtime
        .block_on(async { read_mnemonics(source_file, vec![]).await })?
        .par_iter()
        .filter_map(|o| restore_keypair_from_mnemonic_default(o).ok())
        .collect::<Vec<_>>();
    info!("{} source keys for testing", source_kps.len());

    let targets = (0..count as usize * source_kps.len())
        .into_par_iter()
        .filter_map(|_| {
            gen_one_mnemonic_default()
                .ok()
                .and_then(|o| restore_keypair_from_mnemonic_default(o.as_str()).ok())
                .map(|o| o.pub_key)
        })
        .collect::<Vec<_>>();

    if targets.len() != count as usize * source_kps.len() {
        return Err(Error::Other("Not enough targets generated".to_owned()));
    }

    let base = network.base_url();
    let tm_client = tendermint_rpc::HttpClient::new(format!("{base}:26657").as_str()).unwrap();
    let mut last = runtime
        .block_on(async { tm_client.latest_block().await })
        .map_err(|o| Error::Native(o.to_string()))?
        .block
        .header
        .height;

    for chunk in targets.chunks(count as usize) {
        source_kps
            .par_iter()
            .zip(chunk)
            .for_each(|(kp, target)| transfer(base.as_str(), kp.clone(), *target, TX_FEE_MIN).unwrap());

        loop {
            let current = runtime
                .block_on(async { tm_client.latest_block().await })
                .map_err(|o| Error::Native(o.to_string()))?
                .block
                .header
                .height;
            if current > last {
                last = current;
                break;
            } else {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn basic_eth_test(
    network: &Network,
    delay: u64,
    max_par: u64,
    count: u64,
    source_file: &PathBuf,
    timeout: Option<u64>,
    check_balance: bool,
) -> Result<()> {
    let source_keys: Vec<agent::KeyPair> =
        serde_json::from_str(std::fs::read_to_string(source_file).unwrap().as_str()).unwrap();
    let target_amount = web3::types::U256::exp10(16); // 0.01 eth

    let max_pool_size = calc_pool_size(source_keys.len(), max_par as usize);
    rayon::ThreadPoolBuilder::new()
        .num_threads(max_pool_size)
        .build_global()
        .unwrap();
    info!("thread pool size {}", max_pool_size);

    let url = network.eth_url();
    let client = Arc::new(TestClient::setup(Some(url), timeout));

    let chain_id = client.chain_id().unwrap().as_u64();
    let gas_price = client.gas_price().unwrap();
    info!("chain_id:     {}", chain_id);
    info!("gas_price:    {}", gas_price);
    info!("block_number: {}", client.block_number().unwrap());
    info!("frc20 code:   {:?}", client.frc20_code().unwrap());

    info!("preparing test data, it could take several minutes...");
    let source_keys = source_keys
        .par_iter()
        .filter_map(|kp| {
            let (secret, address) = (
                secp256k1::SecretKey::from_str(kp.private.as_str()).unwrap(),
                Address::from_str(kp.address.as_str()).unwrap(),
            );
            let balance = if check_balance {
                client.balance(address, None)
            } else {
                U256::MAX
            };
            if balance > target_amount.mul(count) {
                let target = (0..count)
                    .map(|_| {
                        (
                            Address::from_str(one_eth_key().address.as_str()).unwrap(),
                            target_amount,
                        )
                    })
                    .collect::<Vec<_>>();
                debug!("account {:?} added to source pool", address);
                Some((secret, address, target))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    if count == 0 || source_keys.is_empty() {
        error!("Not enough sufficient source accounts or target accounts, skipped.");
        return Ok(());
    }

    let total_succeed = AtomicU64::new(0);
    let concurrences = if source_keys.len() > max_pool_size {
        max_pool_size
    } else {
        source_keys.len()
    };

    // one-thread per source key
    info!("starting tests...");
    let start_height = client.block_number().unwrap();
    let mut last_height = start_height;
    let total = source_keys.len() * count as usize;
    let now = std::time::Instant::now();
    for r in 0..count {
        loop {
            let current = client.block_number().unwrap();
            if current > last_height {
                last_height = current;
                break;
            } else {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        let now = std::time::Instant::now();
        source_keys.par_iter().for_each(|(source, address, targets)| {
            let target = targets.get(r as usize).unwrap();
            if let Some(nonce) = client.pending_nonce(*address) {
                if client
                    .distribution_simple(source, target, Some(chain_id), Some(gas_price), Some(nonce))
                    .is_ok()
                {
                    total_succeed.fetch_add(1, Relaxed);
                }
            }
        });
        let elapsed = now.elapsed().as_secs();
        info!("round {}/{} time {}", r + 1, count, elapsed);
        std::thread::sleep(Duration::from_secs(delay));
    }

    let elapsed = now.elapsed().as_secs();
    let end_height = client.block_number().unwrap();

    let avg = total as f64 / elapsed as f64;
    info!(
        "Test result summary: total,{:?}/{},concurrency,{},TPS,{:.3},seconds,{},height,{},{}",
        total_succeed, total, concurrences, avg, elapsed, start_height, end_height,
    );
    Ok(())
}
