use bip0039::{Count, Language, Mnemonic};
use bip32::{DerivationPath, XPrv};
use libsecp256k1::{PublicKey, SecretKey};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::ops::Mul;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{env, thread};
use tokio::runtime::Runtime;
use web3::{
    transports::Http,
    types::{
        Address, BlockNumber, Bytes, Transaction, TransactionId, TransactionParameters, TransactionReceipt, H160, H256,
        U256, U64,
    },
};

const FRC20_ADDRESS: u64 = 0x1000;
const BLOCK_TIME: u64 = 16;

//const WEB3_SRV: &str = "http://127.0.0.1:8545";
//const WEB3_SRV: &str = "http://18.236.205.22:8545";
const WEB3_SRV: &str = "https://prod-testnet.prod.findora.org:8545";
//const WEB3_SRV: &str = "https://dev-mainnetmock.dev.findora.org:8545";

const ROOT_SK: &str = "b8836c243a1ff93a63b12384176f102345123050c9f3d3febbb82e3acd6dd1cb";
const ROOT_ADDR: &str = "0xBb4a0755b740a55Bf18Ac4404628A1a6ae8B6F8F";

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeyPair {
    address: String,
    private: String,
}

fn one_eth_key() -> KeyPair {
    let mnemonic = Mnemonic::generate_in(Language::English, Count::Words12);
    let bs = mnemonic.to_seed("");
    let ext = XPrv::derive_from_path(&bs, &DerivationPath::from_str("m/44'/60'/0'/0/0").unwrap()).unwrap();

    let secret = SecretKey::parse_slice(&ext.to_bytes()).unwrap();
    let public = PublicKey::from_secret_key(&secret);

    let mut res = [0u8; 64];
    res.copy_from_slice(&public.serialize()[1..65]);
    let public = H160::from(H256::from_slice(Keccak256::digest(&res).as_slice()));

    KeyPair {
        address: eth_checksum::checksum(&format!("{:?}", public)),
        private: hex::encode(secret.serialize()),
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
struct TransferMetrics {
    from: Address,
    to: Address,
    amount: U256,
    hash: Option<H256>, // Tx hash
    status: u64,        // 1 - success, 0 - fail
    wait: u64,          // seconds for waiting tx receipt
}

struct TestClient {
    web3: Arc<web3::Web3<Http>>,
    root_sk: secp256k1::SecretKey,
    root_addr: Address,
    rt: Runtime,
}

impl TestClient {
    pub fn setup(url: Option<&str>, root_sk: Option<&str>, root_addr: Option<&str>) -> Self {
        let transport = web3::transports::Http::new(url.unwrap_or(WEB3_SRV)).unwrap();
        let web3 = Arc::new(web3::Web3::new(transport));
        let root_sk = secp256k1::SecretKey::from_str(root_sk.unwrap_or(ROOT_SK)).unwrap();
        let root_addr = Address::from_str(root_addr.unwrap_or(ROOT_ADDR)).unwrap();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        Self {
            web3,
            root_sk,
            root_addr,
            rt,
        }
    }

    pub fn chain_id(&self) -> Option<U256> {
        self.rt.block_on(self.web3.eth().chain_id()).ok()
    }

    pub fn block_number(&self) -> Option<U64> {
        self.rt.block_on(self.web3.eth().block_number()).ok()
    }

    pub fn gas_price(&self) -> Option<U256> {
        self.rt.block_on(self.web3.eth().gas_price()).ok()
    }

    pub fn frc20_code(&self) -> Option<Bytes> {
        self.rt
            .block_on(self.web3.eth().code(H160::from_low_u64_be(FRC20_ADDRESS), None))
            .ok()
    }

    #[allow(unused)]
    pub fn transaction(&self, id: TransactionId) -> Option<Transaction> {
        self.rt.block_on(self.web3.eth().transaction(id)).unwrap_or_default()
    }

    pub fn transaction_receipt(&self, hash: H256) -> Option<TransactionReceipt> {
        self.rt
            .block_on(self.web3.eth().transaction_receipt(hash))
            .unwrap_or_default()
    }

    #[allow(unused)]
    pub fn accounts(&self) -> Vec<Address> {
        self.rt.block_on(self.web3.eth().accounts()).unwrap_or_default()
    }

    pub fn balance(&self, address: Address, number: Option<BlockNumber>) -> U256 {
        self.rt
            .block_on(self.web3.eth().balance(address, number))
            .unwrap_or_default()
    }

    pub fn distribution(
        &self,
        source: Option<(secp256k1::SecretKey, Address)>,
        accounts: &[&str],
        amounts: &[U256],
    ) -> web3::Result<(Vec<TransferMetrics>, u64)> {
        let mut results = vec![];
        let mut succeed = 0u64;
        let mut idx = 1u64;
        let total = accounts.len();
        let source_address = source.unwrap_or((self.root_sk, self.root_addr)).1;
        let source_sk = source.unwrap_or((self.root_sk, self.root_addr)).0;
        let wait_time = BLOCK_TIME * 3 + 1;
        accounts
            .iter()
            .zip(amounts)
            .map(|(&account, &am)| {
                let to = Some(Address::from_str(account).unwrap());
                let tm = TransferMetrics {
                    from: source_address,
                    to: to.unwrap(),
                    amount: am,
                    ..Default::default()
                };
                let tp = TransactionParameters {
                    to,
                    value: am,
                    ..Default::default()
                };
                (tp, tm)
            })
            // Sign the txs (can be done offline)
            .for_each(|(tx_object, mut metric)| {
                if let Ok(signed) = self
                    .rt
                    .block_on(self.web3.accounts().sign_transaction(tx_object, &source_sk))
                {
                    if let Ok(hash) = self
                        .rt
                        .block_on(self.web3.eth().send_raw_transaction(signed.raw_transaction))
                    {
                        metric.hash = Some(hash);
                        let mut retry = wait_time;
                        loop {
                            if let Some(receipt) = self.transaction_receipt(hash) {
                                if let Some(status) = receipt.status {
                                    if status == U64::from(1u64) {
                                        succeed += 1;
                                        metric.status = 1;
                                    }
                                }
                                metric.wait = wait_time + 1 - retry;
                                break;
                            } else {
                                std::thread::sleep(Duration::from_secs(1));
                                retry -= 1;
                                if retry == 0 {
                                    metric.wait = wait_time;
                                    break;
                                }
                            }
                        }
                    }
                }
                println!("{}/{} {:?} {}", idx, total, metric.to, metric.status == 1);
                idx += 1;
                results.push(metric);
            });

        println!("Tx succeeded: {}/{}", succeed, total);

        Ok((results, succeed))
    }
}

fn show_usage(prog: &str) {
    println!("{} help", prog);
    println!("{} load_source NumberPerAccount", prog);
    println!("{} SourceAccountNumber NumberPerAccount", prog);
}

fn main() -> web3::Result<()> {
    let mut per_count = 10;
    let mut source_count = 5;
    let mut prog = "feth".to_owned();
    let mut source_keys = None;
    let mut metrics = None;
    for (i, arg) in env::args().enumerate() {
        if i == 0 {
            prog = arg;
        } else if i == 1 {
            if arg.as_str() == "help" {
                show_usage(prog.as_str());
                return Ok(());
            } else if arg.as_str() == "load_source" {
                println!("loading from \"source_keys.001\"");
                let keys: Vec<KeyPair> =
                    serde_json::from_str(std::fs::read_to_string("source_keys.001").unwrap().as_str()).unwrap();
                source_count = keys.len();
                source_keys = Some(keys);
            } else {
                source_count = arg.parse::<usize>().unwrap_or(source_count);
            }
        } else if i == 2 {
            per_count = arg.parse::<usize>().unwrap_or(per_count);
        }
    }
    let source_amount = U256::exp10(18 + 3); // 1000 eth
    let target_amount = U256::exp10(17); // 0.1 eth

    let client = TestClient::setup(None, None, None);

    println!("chain_id:     {}", client.chain_id().unwrap());
    println!("gas_price:    {}", client.gas_price().unwrap());
    println!("block_number: {}", client.block_number().unwrap());
    println!("frc20 code:   {:?}", client.frc20_code().unwrap());
    let balance = client.balance(ROOT_ADDR[2..].parse().unwrap(), None);
    println!("Root Balance: {}", balance);

    let source_keys = source_keys.unwrap_or_else(|| {
        if std::fs::File::open("source_keys.001").is_ok() {
            panic!("file \"source_keys.001\" already exists");
        }
        if source_amount.mul(source_count + 1) >= balance {
            panic!("Too large source account number, maximum {}", balance / source_amount);
        }
        let source_keys = (0..source_count).map(|_| one_eth_key()).collect::<Vec<_>>();
        let data = serde_json::to_string(&source_keys).unwrap();
        std::fs::write("source_keys.001", &data).unwrap();

        let source_accounts = source_keys.iter().map(|key| key.address.as_str()).collect::<Vec<_>>();
        // 1000 eth
        let amounts = vec![source_amount; source_count];
        metrics = Some(client.distribution(None, &source_accounts, &amounts).unwrap().0);
        // save metrics to file
        let data = serde_json::to_string(&metrics).unwrap();
        std::fs::write("metrics.001", &data).unwrap();

        source_keys
    });
    let metrics = metrics.unwrap_or_else(|| {
        source_keys
            .iter()
            .map(|kp| {
                let balance = client.balance(kp.address[2..].parse().unwrap(), None);
                let status = if balance <= target_amount.mul(per_count) { 0 } else { 1 };
                TransferMetrics {
                    from: client.root_addr,
                    to: Default::default(),
                    amount: balance,
                    hash: None,
                    status,
                    wait: 0,
                }
            })
            .collect::<Vec<_>>()
    });

    if source_count == 0 || per_count == 0 {
        return Ok(());
    }

    let client = Arc::new(client);
    let mut handles = vec![];
    let total_succeed = Arc::new(Mutex::new(0u64));
    let now = std::time::Instant::now();

    metrics.into_iter().enumerate().for_each(|(i, m)| {
        if m.status == 1 {
            let client = client.clone();
            let target_count = per_count;
            let keys = (0..target_count).map(|_| one_eth_key()).collect::<Vec<_>>();
            let am = target_amount;
            let source = source_keys.get(i).map(|s| {
                (
                    secp256k1::SecretKey::from_str(s.private.as_str()).unwrap(),
                    Address::from_str(s.address.as_str()).unwrap(),
                )
            });
            let total_succeed = total_succeed.clone();

            let handle = thread::spawn(move || {
                let amounts = vec![am; target_count];
                let accounts = keys.iter().map(|key| key.address.as_str()).collect::<Vec<_>>();
                let (metrics, succeed) = client.distribution(source, &accounts, &amounts).unwrap();
                let file = format!("metrics.target.{}", i);
                let data = serde_json::to_string(&metrics).unwrap();
                std::fs::write(file, data).unwrap();

                let mut num = total_succeed.lock().unwrap();
                *num += succeed;
            });
            handles.push(handle);
        }
    });

    source_count = handles.len();
    for h in handles {
        h.join().unwrap();
    }

    let elapsed = now.elapsed().as_secs();
    let avg = source_count as f64 * per_count as f64 / elapsed as f64;
    println!(
        "Transfer from {} accounts to {} accounts concurrently, succeed {}, {:.3} Transfer/s, total {} seconds",
        source_count,
        per_count,
        total_succeed.lock().unwrap(),
        avg,
        elapsed,
    );

    Ok(())
}
