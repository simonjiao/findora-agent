use super::common::{read_mnemonics, write_mnemonics, ETH_SECRET, ETH_SOURCE_FILE, UTXO_SECRET, UTXO_SOURCE_FILE};
use agent::{
    error::{Error, Result},
    native::{generate_mnemonic, restore_fra_keypair, restore_keypair_from_mnemonic_default, transfer_batch, FRA},
    one_eth_key, TestClient, TestClientOpts, BLOCK_TIME,
};
use std::{
    ops::{Mul, MulAssign},
    path::PathBuf,
    str::FromStr,
};
use tracing::{debug, info};
use web3::types::Address;

#[allow(clippy::too_many_arguments)]
pub fn fund_utxo_accounts(
    network: &str,
    source_keys_file: Option<PathBuf>,
    count: u64,
    amount: u64,
    load: bool,
) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();

    let amount = amount * FRA;
    let source_keys_file = source_keys_file.unwrap_or(PathBuf::from_str(UTXO_SOURCE_FILE).unwrap());
    let owner_kp = restore_fra_keypair(UTXO_SECRET)?;
    let mnemonics = if load {
        let mut mnemonics = runtime.block_on(async { read_mnemonics(&source_keys_file, vec![]).await })?;
        if count as usize > mnemonics.len() {
            mnemonics.append(&mut generate_mnemonic(count as usize - mnemonics.len(), 24, "en")?);
            //write new keys back
            runtime.block_on(async { write_mnemonics(&source_keys_file, mnemonics).await })?
        } else {
            mnemonics
        }
    } else {
        if source_keys_file.exists() {
            return Err(Error::Other("source keys file already existed".to_string()));
        }
        let mn = generate_mnemonic(count as usize, 24, "en")?;
        //write new keys back
        runtime.block_on(async { write_mnemonics(&source_keys_file, mn.clone()).await })?
    };
    info!("{} accounts loaded to be fund", mnemonics.len());

    let mut kps = vec![];
    for mn in mnemonics {
        let kp = restore_keypair_from_mnemonic_default(mn.trim()).map_err(|o| Error::Other(o.to_string()))?;
        kps.push(kp);
    }
    let target_list = kps.iter().map(|p| (&p.pub_key, amount)).collect::<Vec<_>>();

    transfer_batch(network, owner_kp, target_list)?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn fund_accounts(
    network: &str,
    source_keys_file: Option<PathBuf>,
    count: u64,
    am: u64,
    load: bool,
    redeposit: bool,
    seq: bool,
    delay_in_seconds: u64,
) {
    let source_keys_file = source_keys_file.unwrap_or(PathBuf::from_str(ETH_SOURCE_FILE).unwrap());
    let mut amount = web3::types::U256::exp10(17); // 0.1 eth
    amount.mul_assign(am);

    let opts = TestClientOpts {
        endpoint_url: Some(network.to_string()),
        secret_file: Some(ETH_SECRET.to_owned()),
        timeout: None,
    };
    let client = TestClient::setup_with_opts(opts);
    let balance = client.balance(client.root_addr, None);
    info!("Balance of {:?}: {}", client.root_addr, balance);

    let mut source_keys = if load {
        let keys: Vec<_> = serde_json::from_str(std::fs::read_to_string(&source_keys_file).unwrap().as_str()).unwrap();
        keys
    } else {
        // check if the key file exists
        debug!("generating new source keys");
        if std::fs::File::open(&source_keys_file).is_ok() {
            panic!("file \"{:?}\" already exists", source_keys_file.to_str());
        }
        if amount.mul(count + 1) >= balance {
            panic!("Too large source account number, maximum {}", balance / amount);
        }
        let source_keys = (0..count).map(|_| one_eth_key()).collect::<Vec<_>>();
        let data = serde_json::to_string(&source_keys).unwrap();
        std::fs::write(&source_keys_file, data).unwrap();

        source_keys
    };

    // add more source keys and save them to file
    if count as usize > source_keys.len() {
        let mut file_bak = source_keys_file.clone();
        file_bak.set_extension(".bak");

        source_keys.resize_with(count as usize, one_eth_key);

        let data = serde_json::to_string(&source_keys).unwrap();
        std::fs::write(file_bak.clone(), data).unwrap();

        // replace original file
        std::fs::rename(file_bak, source_keys_file).unwrap();
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
            .block_on(client.distribute(&client.root_sk, &source_accounts, delay_in_seconds))
            .unwrap();
    } else {
        let _metrics = client
            .distribution(1, None, &source_accounts, &Some(BLOCK_TIME), true, true)
            .unwrap();
    }
}
