use crate::{one_eth_key, KeyPair, TestClient};
use log::{debug, info};
use rayon::prelude::*;
use sha3::{Digest, Keccak256};
use std::time::Duration;
use std::{ops::Mul, path::Path, str::FromStr, sync::Arc};
use url::Url;
use web3::types::{Address, H256, U256};

pub fn log_cpus() -> u64 {
    num_cpus::get() as u64
}

pub fn phy_cpus() -> u64 {
    num_cpus::get_physical() as u64
}

pub fn real_network(network: &str) -> Vec<Option<String>> {
    match network {
        "local" => vec![Some("http://localhost:8545".to_string())],
        "anvil" => vec![Some("https://prod-testnet.prod.findora.org:8545".to_string())],
        "main" => vec![Some("https://prod-mainnet.prod.findora.org:8545".to_string())],
        "mock" => vec![Some("https://dev-mainnetmock.dev.findora.org:8545".to_string())],
        "test" => vec![Some("http://34.211.109.216:8545".to_string())],
        "qa01" => vec![Some("https://dev-qa01.dev.findora.org:8545".to_string())],
        "qa02" => vec![Some("https://dev-qa02.dev.findora.org:8545".to_string())],
        n => {
            // comma seperated network endpoints
            n.split(',')
                .filter_map(|s| {
                    let ns = s.trim();
                    if ns.is_empty() || Url::parse(ns).is_err() {
                        None
                    } else {
                        Some(Some(ns.to_string()))
                    }
                })
                .collect::<Vec<_>>()
        }
    }
}

#[inline(always)]
pub fn extract_keypair_from_file<P>(secret: P) -> (secp256k1::SecretKey, Address)
where
    P: AsRef<Path>,
{
    let sk_str = std::fs::read_to_string(secret).unwrap();
    let root_sk = secp256k1::SecretKey::from_str(sk_str.trim()).unwrap();
    let s = secp256k1::Secp256k1::signing_only();
    let root_pk = secp256k1::PublicKey::from_secret_key(&s, &root_sk);
    let mut res = [0u8; 64];
    res.copy_from_slice(&root_pk.serialize_uncompressed()[1..65]);
    let root_addr = Address::from(H256::from_slice(Keccak256::digest(res).as_slice()));

    (root_sk, root_addr)
}

pub fn check_parallel_args(max_par: u64) {
    if max_par > log_cpus() * 1000 {
        panic!(
            "Two much working thread, maybe overload the system {}/{}",
            max_par,
            log_cpus(),
        )
    }
    if max_par == 0 {
        panic!("Invalid parallel parameters: max {}", max_par);
    }
}

pub fn calc_pool_size(keys: usize, max_par: usize) -> usize {
    let mut max_pool_size = keys * 2;
    if max_pool_size > max_par {
        max_pool_size = max_par;
    }
    max_pool_size
}

pub fn build_source_keys<P>(
    client: Arc<TestClient>,
    source_file: P,
    check_balance: bool,
    target_amount: U256,
    count: u64,
    max_par: u64,
) -> Vec<(secp256k1::SecretKey, Address, Vec<(Address, U256)>)>
where
    P: AsRef<Path>,
{
    let source_keys: Vec<KeyPair> =
        serde_json::from_str(std::fs::read_to_string(source_file).unwrap().as_str()).unwrap();

    let max_pool_size = calc_pool_size(source_keys.len(), max_par as usize);
    rayon::ThreadPoolBuilder::new()
        .num_threads(max_pool_size)
        .build_global()
        .unwrap();
    info!("thread pool size {}", max_pool_size);

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

    source_keys
}

pub fn display_info(client: Arc<TestClient>) -> (u64, U256) {
    let chain_id = client.chain_id().unwrap().as_u64();
    let gas_price = client.gas_price().unwrap();
    info!("chain_id:     {}", chain_id);
    info!("gas_price:    {}", gas_price);
    info!("block_number: {}", client.block_number().unwrap());
    info!("frc20 code:   {:?}", client.frc20_code().unwrap());

    (chain_id, gas_price)
}

pub fn wait_receipt(client: Arc<TestClient>, hash: H256) -> bool {
    let (_, receipt) = client.wait_for_tx_receipt(hash, Duration::from_secs(1), 3);

    receipt.is_some()
}
