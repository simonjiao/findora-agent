use agent::{one_eth_key, utils::real_network, TestClient};
use std::ops::{Mul, MulAssign};
use std::str::FromStr;
use tracing::{debug, info};
use web3::types::Address;

#[allow(clippy::too_many_arguments)]
pub fn fund_accounts(
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
    let source_keys_file = "source_keys.001";
    let source_keys_file_bak = ".source_keys.001_bak";

    let mut source_keys = if load {
        let keys: Vec<_> = serde_json::from_str(std::fs::read_to_string(source_keys_file).unwrap().as_str()).unwrap();
        keys
    } else {
        // check if the key file exists
        debug!("generating new source keys");
        if std::fs::File::open(source_keys_file).is_ok() {
            panic!("file \"{source_keys_file}\" already exists");
        }
        if amount.mul(count + 1) >= balance {
            panic!("Too large source account number, maximum {}", balance / amount);
        }
        let source_keys = (0..count).map(|_| one_eth_key()).collect::<Vec<_>>();
        let data = serde_json::to_string(&source_keys).unwrap();
        std::fs::write(source_keys_file, data).unwrap();

        source_keys
    };

    // add more source keys and save them to file
    if count as usize > source_keys.len() {
        source_keys.resize_with(count as usize, one_eth_key);

        let data = serde_json::to_string(&source_keys).unwrap();
        std::fs::write(source_keys_file_bak, data).unwrap();

        // replace original file
        std::fs::rename(source_keys_file_bak, source_keys_file).unwrap();
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
                info!("{}/{} {:?}", idx + 1, total, a);
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
}
