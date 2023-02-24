mod commands;
pub use agent::{db, profiler};

use std::{
    fmt::Formatter,
    ops::{Mul, MulAssign},
    path::PathBuf,
    str::FromStr,
    sync::{mpsc, Arc},
};

use agent::{one_eth_key, parse_call_json, parse_deploy_json, parse_query_json, utils::*, TestClient};
use commands::*;
use log::{debug, info};
use web3::types::{Address, BlockId, BlockNumber, TransactionId, H256, U256, U64};

fn eth_transaction(network: &str, timeout: Option<u64>, hash: H256) {
    let network = real_network(network);
    // use first endpoint to fund accounts
    let client = TestClient::setup(network[0].clone(), timeout);
    let tx = client.transaction(TransactionId::from(hash));
    log::info!("{:?}", tx);
}

fn eth_account(network: &str, timeout: Option<u64>, account: Address) {
    let network = real_network(network);
    // use first endpoint to fund accounts
    let client = TestClient::setup(network[0].clone(), timeout);
    let balance = client.balance(account, None);
    let nonce = client.nonce(account, None);
    log::info!("{:?}: {} {:?}", account, balance, nonce);
}
fn eth_contract(network: &str, timeout: Option<u64>, optype: &ContractOP, config: &PathBuf) -> anyhow::Result<()> {
    let network = real_network(network);
    let client = TestClient::setup(network[0].clone(), timeout);
    match optype {
        ContractOP::Deploy => {
            let deploy_json = parse_deploy_json(config)?;
            client.contract_deploy(deploy_json)?;
        }
        ContractOP::Call => {
            let call_json = parse_call_json(config)?;
            client.contract_call(call_json)?;
        }
        ContractOP::Query => {
            let query_json = parse_query_json(config)?;
            client.contract_query(query_json)?;
        }
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct BlockInfo {
    number: u64,
    timestamp: U256,
    count: usize,
    block_time: u64,
}

impl std::fmt::Display for BlockInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{}",
            self.number, self.timestamp, self.count, self.block_time
        )
    }
}

#[allow(unused)]
fn para_eth_blocks(client: Arc<TestClient>, start: u64, end: u64) {
    let pool = rayon::ThreadPoolBuilder::new().num_threads(8).build().unwrap();
    let (tx, rx) = mpsc::channel();
    (start..end).for_each(|n| {
        let tx = tx.clone();
        let client = client.clone();
        pool.install(move || {
            let id = BlockId::Number(BlockNumber::Number(U64::from(n)));
            let b = client.block_with_tx_hashes(id).map(|b| BlockInfo {
                number: b.number.unwrap().as_u64(),
                timestamp: b.timestamp,
                count: b.transactions.len(),
                block_time: 0u64,
            });
            tx.send((n, b)).unwrap();
        })
    });
    let mut blocks = vec![None; (end - start) as usize];
    for _ in start..end {
        let j = rx.recv().unwrap();
        *blocks.get_mut((j.0 - start) as usize).unwrap() = j.1
    }
    blocks.iter().for_each(|b| {
        if let Some(b) = b {
            info!("{},{},{},{}", b.number, b.timestamp, b.count, b.block_time);
        } else {
            info!("None");
        }
    })
}

fn eth_blocks(network: &str, timeout: Option<u64>, start: Option<u64>, count: u64, follow: bool) {
    let client = TestClient::setup(Some(network.to_string()), timeout);
    let start = start.unwrap_or_else(|| client.block_number().unwrap().as_u64());
    if !follow && count == 0 {
        panic!("Need a non-zero block count for a non-follow mode");
    }
    let mut fetched = {
        let id = if start == 0 {
            BlockId::Number(BlockNumber::Number(U64::zero()))
        } else {
            BlockId::Number(BlockNumber::Number(U64::from(start - 1)))
        };
        client
            .block_with_tx_hashes(id)
            .map(|b| BlockInfo {
                number: b.number.unwrap().as_u64(),
                timestamp: b.timestamp,
                count: b.transactions.len(),
                block_time: 0u64,
            })
            .unwrap()
    };

    let range = start..if follow { u64::MAX } else { start + count };
    for b in range {
        let id = BlockId::Number(BlockNumber::Number(U64::from(b)));
        let current = client
            .block_with_tx_hashes(id)
            .map(|b| BlockInfo {
                number: b.number.unwrap().as_u64(),
                timestamp: b.timestamp,
                count: b.transactions.len(),
                block_time: (b.timestamp - fetched.timestamp).as_u64(),
            })
            .unwrap();
        info!(
            "{},{},{},{}",
            current.number, current.timestamp, current.count, current.block_time,
        );
        fetched = current;
    }
}

#[allow(clippy::too_many_arguments)]
fn fund_accounts(
    network: &str,
    timeout: Option<u64>,
    block_time: u64,
    count: u64,
    am: u64,
    load: bool,
    redeposit: bool,
    seq: bool,
) {
    let mut amount = web3::types::U256::exp10(17); // 0.1 eth
    amount.mul_assign(am);

    let network = real_network(network);
    // use first endpoint to fund accounts
    let client = TestClient::setup(network[0].clone(), timeout);
    let balance = client.balance(client.root_addr, None);
    info!("Balance of {:?}: {}", client.root_addr, balance);

    let mut source_keys = if load {
        let keys: Vec<_> =
            serde_json::from_str(std::fs::read_to_string("../../source_keys.001").unwrap().as_str()).unwrap();
        keys
    } else {
        // check if the key file exists
        debug!("generating new source keys");
        if std::fs::File::open("../../source_keys.001").is_ok() {
            panic!("file \"source_keys.001\" already exists");
        }
        if amount.mul(count + 1) >= balance {
            panic!("Too large source account number, maximum {}", balance / amount);
        }
        let source_keys = (0..count).map(|_| one_eth_key()).collect::<Vec<_>>();
        let data = serde_json::to_string(&source_keys).unwrap();
        std::fs::write("../../source_keys.001", data).unwrap();

        source_keys
    };

    // add more source keys and save them to file
    if count as usize > source_keys.len() {
        source_keys.resize_with(count as usize, one_eth_key);

        std::fs::rename("../../source_keys.001", ".source_keys.001.bak").unwrap();
        let data = serde_json::to_string(&source_keys).unwrap();
        std::fs::write("../../source_keys.001", data).unwrap();
    }

    let total = source_keys.len();
    let source_accounts = source_keys
        .into_iter()
        .enumerate()
        .filter_map(|(idx, key)| {
            let from = Address::from_str(key.address.as_str()).unwrap();
            let account = if redeposit {
                let balance = client.balance(from, None);
                if balance < amount {
                    Some((from, amount))
                } else {
                    None
                }
            } else {
                Some((from, amount))
            };
            if let Some(a) = account.as_ref() {
                log::info!("{}/{} {:?}", idx + 1, total, a);
            }
            account
        })
        .collect::<Vec<_>>();
    // 1000 eth
    if seq {
        client
            .rt
            .block_on(client.distribute(&client.root_sk, &source_accounts))
            .unwrap();
    } else {
        let _metrics = client
            .distribution(1, None, &source_accounts, &Some(block_time), true, true)
            .unwrap();
    }

    // save metrics to file
    //let data = serde_json::to_string(&metrics).unwrap();
    //std::fs::write("metrics.001", &data).unwrap();
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let cli = Cli::parse_args();
    info!("{:?}", cli);
    info!("logical cpus {}, physical cpus {}", log_cpus(), phy_cpus());

    match &cli.command {
        Some(Commands::Fund {
            network,
            timeout,
            block_time,
            count,
            amount,
            load,
            redeposit,
            seq,
        }) => {
            fund_accounts(
                network.get_url().as_str(),
                *timeout,
                *block_time,
                *count,
                *amount,
                *load,
                *redeposit,
                *seq,
            );
            Ok(())
        }
        Some(Commands::Info {
            network,
            timeout,
            account,
        }) => {
            eth_account(network.get_url().as_str(), *timeout, *account);
            Ok(())
        }
        Some(Commands::Transaction { network, timeout, hash }) => {
            eth_transaction(network.get_url().as_str(), *timeout, *hash);
            Ok(())
        }
        Some(Commands::Block {
            network,
            timeout,
            start,
            count,
            follow,
        }) => {
            eth_blocks(network.get_url().as_str(), *timeout, *start, *count, *follow);
            Ok(())
        }
        Some(Commands::Etl {
            abcid,
            tendermint,
            redis,
            load,
        }) => {
            let _ = Cli::etl_cmd(abcid, tendermint, redis.as_str(), *load);
            Ok(())
        }
        Some(Commands::Profiler { network, enable }) => {
            let _ = Cli::profiler(network.as_str(), *enable);
            Ok(())
        }
        Some(Commands::Contract {
            network,
            optype,
            config,
            timeout,
        }) => {
            let rpc_url = network.get_url();
            eth_contract(&rpc_url, *timeout, optype, config)?;
            Ok(())
        }
        Some(Commands::Test {
            network,
            mode,
            delay: delay_in_blocks,
            max_threads,
            count,
            source_count,
            source,
            timeout,
            check_balance,
            wait_receipt: _need_wait_receipt,
            fetch_block: _need_fetch_block,
        }) => match mode {
            &TestMode::Long => long_run_test(
                network,
                max_threads,
                source,
                timeout,
                count,
                check_balance,
                source_count,
                delay_in_blocks,
            ),
            _ => panic!("unsupported test mode"),
        },
        Some(Commands::Prism { .. }) => Ok(()),
        None => Ok(()),
    }
}
