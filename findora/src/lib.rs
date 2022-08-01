pub mod error;
pub mod utils;
use crate::{
    error::{Error, InternalError, Result},
    utils::extract_keypair_from_file,
};
use bip0039::{Count, Language, Mnemonic};
use bip32::{DerivationPath, XPrv};
use lazy_static::lazy_static;
use libsecp256k1::{PublicKey, SecretKey};
use log::{debug, error, info, warn};
use reqwest::{Client, Url};
use secp256k1::SecretKey as SecretKey2;

use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::time;
use std::{
    cell::RefCell,
    error::Error as StdError,
    fs,
    ops::AddAssign,
    path::PathBuf,
    str::FromStr,
    sync::{
        atomic::{AtomicU32, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use tokio::sync::Mutex;
use tokio::{runtime::Runtime, sync::mpsc::Receiver};
use web3::{
    self,
    api::Eth,
    contract::{Contract, Options},
    types::H160,
};
use web3::{
    transports::Http,
    types::{
        Address, Block, BlockId, BlockNumber, Bytes, Transaction, TransactionId, TransactionParameters,
        TransactionReceipt, H256, U256, U64,
    },
};

const FRC20_ADDRESS: u64 = 0x1000;
pub const BLOCK_TIME: u64 = 16;

//const WEB3_SRV: &str = "http://127.0.0.1:8545";
//const WEB3_SRV: &str = "http://18.236.205.22:8545";
const WEB3_SRV: &str = "https://prod-testnet.prod.findora.org:8545";
//const WEB3_SRV: &str = "https://dev-mainnetmock.dev.findora.org:8545";

lazy_static! {
    pub(crate) static ref CUR_TASKS: Arc<AtomicU32> = Arc::new(AtomicU32::new(0));
    pub(crate) static ref MAX_TASKS: Arc<AtomicU32> = Arc::new(AtomicU32::new(2));
    // total success tasks、total tasks cost time、average tasks cost time queue
    pub(crate) static ref RES_QUEUE_SECS: Arc<Mutex<(u32, u128, Vec::<u128>)>> = Arc::new(Mutex::new((0, 0, Vec::new())));

}

const PULL_INTERVAL: u64 = 50;
const RES_QUEUE_MAX_LEN: usize = 10;
const UPDATE_INTERVAL: u64 = 300;
const DELTA_RANGE: u128 = 100;

#[derive(Debug, Clone, Eq, PartialEq, Serialize, Deserialize)]
pub struct KeyPair {
    pub address: String,
    pub private: String,
}

#[inline(always)]
pub fn one_eth_key() -> KeyPair {
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

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct TxMetric {
    pub to: Address,
    pub amount: U256,
    pub hash: Option<H256>, // Tx hash
    pub status: u64,        // 1 - success, other - fail
    pub wait: u64,          // seconds for waiting tx receipt
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct TransferMetrics {
    pub from: Address,
    pub total: u64,
    pub succeed: u64,
    pub txs: Vec<TxMetric>,
}

#[derive(Debug)]
pub struct TestClient {
    pub web3: Arc<web3::Web3<Http>>,
    pub eth: Arc<web3::api::Eth<Http>>,
    pub accounts: Arc<web3::api::Accounts<Http>>,
    pub root_sk: secp256k1::SecretKey,
    pub root_addr: Address,
    pub overflow_flag: AtomicUsize,
    rt: Runtime,
}

#[derive(Debug)]
pub struct NetworkInfo {
    pub chain_id: U256,
    pub block_number: U64,
    pub gas_price: U256,
    pub frc20_code: Option<Bytes>,
}

impl TestClient {
    pub fn setup(url: Option<String>, timeout: Option<u64>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout.unwrap_or(3)))
            .build()
            .unwrap();
        let url = Url::parse(url.as_deref().unwrap_or(WEB3_SRV)).unwrap();
        let transport = Http::with_client(client, url);
        let web3 = Arc::new(web3::Web3::new(transport));
        let eth = Arc::new(web3.eth());
        let accounts = Arc::new(web3.accounts());
        let (root_sk, root_addr) = extract_keypair_from_file(".secret");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        Self {
            web3,
            eth,
            accounts,
            root_sk,
            root_addr,
            rt,
            overflow_flag: AtomicUsize::from(0),
        }
    }

    pub fn chain_id(&self) -> Option<U256> {
        self.rt.block_on(self.eth.chain_id()).ok()
    }

    pub fn block_number(&self) -> Option<U64> {
        self.rt.block_on(self.eth.block_number()).ok()
    }

    pub fn current_block(&self) -> Option<Block<H256>> {
        self.rt
            .block_on(self.eth.block(BlockId::Number(BlockNumber::Latest)))
            .unwrap_or_default()
    }

    pub fn block_with_tx_hashes(&self, id: BlockId) -> Option<Block<H256>> {
        self.block_with_tx_hashes_inner(id, None, None)
    }

    pub fn block_with_tx_hashes_inner(
        &self,
        id: BlockId,
        interval: Option<u64>,
        times: Option<u64>,
    ) -> Option<Block<H256>> {
        let interval = interval.unwrap_or(1);
        let mut retries = 1u64;
        loop {
            if let Ok(Some(block)) = self.rt.block_on(self.eth.block(id)) {
                break Some(block);
            }
            if times == Some(retries) || times == Some(0u64) {
                break None;
            }
            warn!("retries {}", retries);
            retries += 1;
            std::thread::sleep(Duration::from_secs(interval));
        }
    }

    pub fn nonce(&self, from: Address, block: Option<BlockNumber>) -> Option<U256> {
        self.rt.block_on(self.eth.transaction_count(from, block)).ok()
    }

    pub fn pending_nonce(&self, from: Address) -> Option<U256> {
        self.pending_nonce_inner(from, Some(3), None)
    }

    pub fn pending_nonce_inner(&self, from: Address, interval: Option<u64>, times: Option<u64>) -> Option<U256> {
        let interval = interval.unwrap_or(5);
        let mut tries = 1u64;
        loop {
            match self
                .rt
                .block_on(self.eth.transaction_count(from, Some(BlockNumber::Pending)))
            {
                Ok(nonce) => break Some(nonce),
                Err(e) => error!("failed to get nonce, tries {}, {:?}", tries, e),
            }
            std::thread::sleep(Duration::from_secs(interval));
            if times == Some(tries) || times == Some(0u64) {
                break None;
            }
            tries += 1;
        }
    }

    pub fn gas_price(&self) -> Option<U256> {
        self.rt.block_on(self.eth.gas_price()).ok()
    }

    pub fn frc20_code(&self) -> Option<Bytes> {
        self.rt
            .block_on(self.eth.code(H160::from_low_u64_be(FRC20_ADDRESS), None))
            .ok()
    }

    #[allow(unused)]
    pub fn transaction(&self, id: TransactionId) -> Option<Transaction> {
        self.rt.block_on(self.eth.transaction(id)).unwrap_or_default()
    }

    pub fn transaction_receipt(&self, hash: H256) -> Option<TransactionReceipt> {
        self.rt.block_on(self.eth.transaction_receipt(hash)).unwrap_or_default()
    }

    #[allow(unused)]
    pub fn accounts(&self) -> Vec<Address> {
        self.rt.block_on(self.eth.accounts()).unwrap_or_default()
    }

    pub fn balance(&self, address: Address, number: Option<BlockNumber>) -> U256 {
        self.rt.block_on(self.eth.balance(address, number)).unwrap_or_default()
    }

    pub fn wait_for_tx_receipt(&self, hash: H256, interval: Duration, times: u64) -> (u64, Option<TransactionReceipt>) {
        let mut wait = 0;
        let mut retry = times;
        loop {
            if let Some(receipt) = self.transaction_receipt(hash) {
                wait = times + 1 - retry;
                break (wait, Some(receipt));
            } else {
                std::thread::sleep(interval);
                retry -= 1;
                if retry == 0 {
                    break (wait, None);
                }
            }
        }
    }

    pub fn check_wait_overflow(&self, id: usize, interval: Option<u64>) {
        loop {
            let flag = self.overflow_flag.load(Ordering::Relaxed);
            if flag == 0 || flag == id {
                break;
            }
            std::thread::sleep(Duration::from_secs(interval.unwrap_or(3)));
        }
    }

    pub fn parse_error(&self, err: Option<&dyn StdError>) -> Error {
        match err {
            Some(e) => {
                let err_str = e.to_string();
                if err_str.contains("broadcast_tx_sync") {
                    Error::SyncTx
                } else if err_str.contains("Transaction check error") {
                    Error::CheckTx
                } else if err_str.contains("error sending request") {
                    Error::SendErr
                } else if err_str.contains("InternalError") {
                    if err_str.contains("InvalidNonce") {
                        Error::TxInternalErr(InternalError::InvalidNonce(err_str))
                    } else {
                        Error::TxInternalErr(InternalError::Other(err_str))
                    }
                } else {
                    Error::Unknown(err_str)
                }
            }
            None => Error::Unknown("empty error".to_string()),
        }
    }

    pub fn distribution(
        &self,
        id: usize,
        source: Option<(secp256k1::SecretKey, Address)>,
        targets: &[(Address, U256)],
        block_time: &Option<u64>,
        need_wait: bool,
        need_retry: bool,
    ) -> Result<TransferMetrics> {
        let mut results = vec![];
        let mut succeed = 0u64;
        let total = targets.len();
        let source_address = source.unwrap_or((self.root_sk, self.root_addr)).1;
        let source_sk = source.unwrap_or((self.root_sk, self.root_addr)).0;
        let wait_time = block_time.unwrap_or(BLOCK_TIME) * 3 + 1;
        let chain_id = self.chain_id().map(|id| id.as_u64());
        let gas_price = self.gas_price();
        let nonce = RefCell::new(self.pending_nonce(source_address).unwrap());
        let last_err_cnt = RefCell::new(0u64);
        targets
            .iter()
            .map(|(account, am)| {
                let to = Some(*account);
                let tm = TxMetric {
                    to: to.unwrap(),
                    amount: *am,
                    status: 99,
                    ..Default::default()
                };
                let tp = TransactionParameters {
                    to,
                    value: *am,
                    chain_id,
                    gas_price,
                    nonce: Some(*nonce.borrow()),
                    ..Default::default()
                };
                (tp, tm)
            })
            .enumerate()
            // Sign the txs (can be done offline)
            .for_each(|(idx, (mut tx_object, mut metric))| {
                match self
                    .rt
                    .block_on(self.accounts.sign_transaction(tx_object.clone(), &source_sk))
                {
                    Ok(signed) => {
                        self.check_wait_overflow(id, None);
                        match self.rt.block_on(self.eth.send_raw_transaction(signed.raw_transaction)) {
                            Ok(hash) => {
                                metric.hash = Some(hash);
                                debug!("{}/{} {:?} {:?}", idx + 1, total, metric.to, hash);
                                nonce.borrow_mut().add_assign(U256::one());
                                if let Ok(val) =
                                    self.overflow_flag
                                        .compare_exchange(id, 0, Ordering::Acquire, Ordering::Relaxed)
                                {
                                    warn!("overflow flag cleared by {} me {}", val, id);
                                }
                            }
                            Err(e) => {
                                match self.parse_error(e.source()) {
                                    Error::SyncTx => {
                                        if let Ok(mut val) = self.overflow_flag.compare_exchange(
                                            0,
                                            id,
                                            Ordering::Acquire,
                                            Ordering::Relaxed,
                                        ) {
                                            if val == 0 {
                                                val = id;
                                            }
                                            warn!("overflow flag set by {}, me {}", val, id);
                                        }
                                        self.check_wait_overflow(id, None);
                                    }
                                    Error::SendErr => {
                                        // TODO: adjust timeout
                                        error!("Failed to send request, increase timeout could be helpful");
                                    }
                                    Error::CheckTx => {
                                        error!("Transaction check error");
                                    }
                                    Error::TxInternalErr(e) => {
                                        error!("Internal error: {:?}", e);
                                    }
                                    _ => {
                                        error!("other error {:?}", e);
                                    }
                                }
                                let mut skip = false;
                                while self.overflow_flag.load(Ordering::Relaxed) == id {
                                    warn!("try to check if error persists {}", id);
                                    let mut tx_object = tx_object.clone();
                                    if let Some(nonce) = self.pending_nonce(source_address) {
                                        tx_object.nonce = Some(nonce);
                                    }
                                    if let Ok(signed) =
                                        self.rt.block_on(self.accounts.sign_transaction(tx_object, &source_sk))
                                    {
                                        match self
                                            .rt
                                            .block_on(self.eth.send_raw_transaction(signed.raw_transaction.clone()))
                                        {
                                            Ok(_) => {
                                                if self.overflow_flag.compare_exchange(
                                                    id,
                                                    0,
                                                    Ordering::Acquire,
                                                    Ordering::Relaxed,
                                                ) != Ok(id)
                                                {
                                                    panic!("This should never happened");
                                                } else {
                                                    skip = true;
                                                    break;
                                                }
                                            }
                                            Err(e) => {
                                                error!("Failed to send tx {:?}, continue to trying", e);
                                            }
                                        }
                                    } else {
                                        error!("Failed to sign tx, nothing we can do...")
                                    }
                                    std::thread::sleep(Duration::from_secs(3));
                                }
                                if need_retry && !skip {
                                    error!("retry for error {:?}", e);
                                    metric.status = 97;
                                    let wait_time = 2u64;
                                    last_err_cnt.borrow_mut().add_assign(1);
                                    let factor = *last_err_cnt.borrow();
                                    std::thread::sleep(Duration::from_secs(wait_time * factor));
                                    // retrieve nonce if failed to send tx
                                    *nonce.borrow_mut() = self.pending_nonce(source_address).unwrap();
                                    // give it another chance
                                    tx_object.nonce = Some(*nonce.borrow());
                                    if let Ok(signed) =
                                        self.rt.block_on(self.accounts.sign_transaction(tx_object, &source_sk))
                                    {
                                        match self.rt.block_on(self.eth.send_raw_transaction(signed.raw_transaction)) {
                                            Ok(hash) => {
                                                metric.hash = Some(hash);
                                                warn!(
                                                    "retry {}/{} {:?} {:?} {}",
                                                    idx + 1,
                                                    total,
                                                    metric.to,
                                                    hash,
                                                    *last_err_cnt.borrow()
                                                );
                                                *last_err_cnt.borrow_mut() = 0;
                                                nonce.borrow_mut().add_assign(U256::one());
                                            }
                                            Err(e) => {
                                                error!(
                                                    "give up send {}/{} {:?} {} {:?}",
                                                    idx + 1,
                                                    total,
                                                    metric.to,
                                                    *last_err_cnt.borrow(),
                                                    e
                                                );
                                                last_err_cnt.borrow_mut().add_assign(1);
                                                *nonce.borrow_mut() = self.pending_nonce(source_address).unwrap();
                                            }
                                        }
                                    } else {
                                        error!(
                                            "give up retry sign {}/{} {:?} {} {:?}",
                                            idx + 1,
                                            total,
                                            metric.to,
                                            *last_err_cnt.borrow(),
                                            e
                                        );
                                        *nonce.borrow_mut() = self.pending_nonce(source_address).unwrap();
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("give up sign {}/{} {:?} {:?}", idx + 1, total, metric.to, e);
                        metric.status = 98;
                        // retrieve nonce if failed to send tx
                        *nonce.borrow_mut() = self.pending_nonce(source_address).unwrap();
                    }
                }

                results.push(metric);
            });

        if !need_wait {
            return Ok(TransferMetrics {
                from: source_address,
                total: targets.len() as u64,
                succeed,
                txs: results,
            });
        }

        info!("Waiting for final results...");

        results.iter_mut().enumerate().for_each(|(idx, metric)| {
            let mut retry = wait_time;
            loop {
                if let Some(hash) = metric.hash {
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
            println!(
                "{}/{} {:?} {:?} {}",
                idx,
                total,
                metric.to,
                metric.hash,
                metric.status == 1
            );
        });

        info!("Tx succeeded: {}/{}", succeed, total);

        Ok(TransferMetrics {
            from: source_address,
            total: targets.len() as u64,
            succeed,
            txs: results,
        })
    }

    pub fn distribution_simple(
        &self,
        source: &secp256k1::SecretKey,
        target: &(Address, U256),
        chain_id: Option<u64>,
        gas_price: Option<U256>,
        nonce: Option<U256>,
    ) -> Result<H256> {
        let (account, amount) = target;
        let tx_object = TransactionParameters {
            to: Some(*account),
            value: *amount,
            chain_id,
            gas_price,
            nonce,
            ..Default::default()
        };
        // Sign the txs (can be done offline)
        match self.rt.block_on(self.accounts.sign_transaction(tx_object, source)) {
            Ok(signed) => {
                let result = self.rt.block_on(self.eth.send_raw_transaction(signed.raw_transaction));
                match result {
                    Err(e) => Err(self.parse_error(e.source())),
                    Ok(hash) => Ok(hash),
                }
            }
            Err(e) => Err(self.parse_error(e.source())),
        }
    }

    pub fn contract_deploy(&self, deploy_json: DeployJson) -> anyhow::Result<()> {
        self.rt.block_on(async {
            let mut task_queue = Vec::with_capacity(deploy_json.deploy_obj.len());

            for deploy_obj in deploy_json.deploy_obj {
                let DeployJsonObj {
                    code_path,
                    abi_path,
                    sec_key,
                    gas,
                    gas_price,
                    args,
                } = deploy_obj;

                let eth = (*self.eth.clone()).clone();
                let task = tokio::spawn(async move {
                    CUR_TASKS.store(CUR_TASKS.load(Ordering::Acquire) + 1, Ordering::Release);

                    let beg_time = get_timestamp();

                    match contract_deploy(eth, &sec_key, &code_path, &abi_path, gas, gas_price, args).await {
                        Ok(v) => {
                            let end_time = get_timestamp();
                            update_res_queue_secs(end_time - beg_time).await;

                            println!("contract address: {:?}", v);
                        }
                        Err(e) => {
                            println!("deploy contract failed: {:?}", e);
                        }
                    };
                    CUR_TASKS.store(CUR_TASKS.load(Ordering::Acquire) - 1, Ordering::Release);
                });
                task_queue.push(task);

                while MAX_TASKS.load(Ordering::Acquire) <= CUR_TASKS.load(Ordering::Acquire) {
                    let task = task_queue.pop().unwrap();
                    task.await?;
                }
            }

            let (tx1, rx1) = tokio::sync::mpsc::channel(2);
            tokio::spawn(max_tasks_update(rx1));

            for task in task_queue {
                task.await?;
            }

            tx1.send(()).await?;

            let success_task = RES_QUEUE_SECS.lock().await.0;
            let total_times = RES_QUEUE_SECS.lock().await.1;

            log::info!(
                "success task: {} total times: {} average time: {}",
                success_task,
                total_times,
                if success_task == 0 {
                    0
                } else {
                    total_times / success_task as u128
                }
            );

            anyhow::Ok(())
        })?;
        Ok(())
    }

    pub fn contract_call(&self, call_json: CallJson) -> anyhow::Result<()> {
        self.rt.block_on(async {
            let mut task_queue = Vec::with_capacity(call_json.call_obj.len());

            for call_obj in call_json.call_obj {
                let CallJsonObj {
                    contract_addr,
                    abi_path,
                    sec_key,
                    gas,
                    gas_price,
                    args,
                } = call_obj;

                let eth = (*self.eth.clone()).clone();
                let task = tokio::spawn(async move {
                    CUR_TASKS.store(CUR_TASKS.load(Ordering::Acquire) + 1, Ordering::Release);

                    let beg_time = get_timestamp();

                    match contract_call(eth, &contract_addr, &sec_key, &abi_path, gas, gas_price, args).await {
                        Ok(v) => {
                            let end_time = get_timestamp();
                            update_res_queue_secs(end_time - beg_time).await;

                            println!("transaction hash: {:?}", v);
                        }
                        Err(e) => {
                            println!("call contract failed: {:?}", e);
                        }
                    };
                    CUR_TASKS.store(CUR_TASKS.load(Ordering::Acquire) - 1, Ordering::Release);
                });
                task_queue.push(task);

                while MAX_TASKS.load(Ordering::Acquire) <= CUR_TASKS.load(Ordering::Acquire) {
                    let task = task_queue.pop().unwrap();
                    task.await?;
                }
            }

            let (tx1, rx1) = tokio::sync::mpsc::channel(2);
            tokio::spawn(max_tasks_update(rx1));

            for task in task_queue {
                task.await?;
            }

            tx1.send(()).await?;

            let success_task = RES_QUEUE_SECS.lock().await.0;
            let total_times = RES_QUEUE_SECS.lock().await.1;

            log::info!(
                "success task: {} total times: {} average time: {}",
                success_task,
                total_times,
                if success_task == 0 {
                    0
                } else {
                    total_times / success_task as u128
                }
            );

            anyhow::Ok(())
        })?;
        Ok(())
    }

    pub fn contract_query(&self, query_json: QueryJson) -> anyhow::Result<()> {
        self.rt.block_on(async {
            let eth = (*self.eth.clone()).clone();
            let result = contract_query(eth, &query_json.contract_addr, &query_json.abi_path, query_json.args).await?;

            log::info!("query result: {:?}", result);
            anyhow::Ok(())
        })?;

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DeployJsonObj {
    pub code_path: String,
    pub abi_path: String,
    pub sec_key: String,
    pub gas: u32,
    pub gas_price: u32,
    pub args: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct DeployJson {
    pub deploy_obj: Vec<DeployJsonObj>,
}
#[derive(Serialize, Deserialize, Clone)]
pub struct CallJsonObj {
    pub contract_addr: String,
    pub abi_path: String,
    pub sec_key: String,
    pub gas: u32,
    pub gas_price: u32,
    pub args: String,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CallJson {
    pub call_obj: Vec<CallJsonObj>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct QueryJson {
    pub contract_addr: String,
    pub abi_path: String,
    pub args: String,
}

pub fn parse_deploy_json(pat: &PathBuf) -> anyhow::Result<DeployJson> {
    let deploy_json_bytes = fs::read(pat)?;
    let deply_json_obj: DeployJson = serde_json::from_slice(deploy_json_bytes.as_slice())?;

    return Ok(deply_json_obj);
}

pub fn parse_call_json(pat: &PathBuf) -> anyhow::Result<CallJson> {
    let call_json_bytes = fs::read(pat)?;
    let call_json_obj: CallJson = serde_json::from_slice(call_json_bytes.as_slice())?;

    return Ok(call_json_obj);
}

pub fn parse_query_json(pat: &PathBuf) -> anyhow::Result<QueryJson> {
    let query_json_bytes = fs::read(pat)?;
    let query_json_obj: QueryJson = serde_json::from_slice(query_json_bytes.as_slice())?;

    return Ok(query_json_obj);
}

async fn contract_deploy(
    eth: Eth<Http>,
    sec_key: &str,
    code_path: &str,
    abi_path: &str,
    gas: u32,
    gas_price: u32,
    _args: String,
) -> web3::contract::Result<H160> {
    let byetcode = fs::read(code_path).unwrap();
    let abi = fs::read(abi_path).unwrap();

    let secretkey = SecretKey2::from_str(sec_key).unwrap();

    let contract = Contract::deploy(eth, &abi)?
        .confirmations(1)
        .poll_interval(time::Duration::from_millis(PULL_INTERVAL))
        .options(Options::with(|opt| {
            opt.gas = Some(gas.into());
            opt.gas_price = Some(gas_price.into());
            // opt.nonce = Some(nonce + nonce_add);
        }))
        .sign_with_key_and_execute(std::str::from_utf8(&byetcode).unwrap(), (), &secretkey, None)
        .await?;

    Ok(contract.address())
}

async fn contract_call(
    eth: Eth<Http>,
    contr_addr: &str,
    sec_key: &str,
    // _account: &str,
    abi_path: &str,
    gas: u32,
    gas_price: u32,
    _args: String,
) -> web3::contract::Result<H256> {
    let abi = fs::read(abi_path).unwrap();
    let contr_addr: H160 = contr_addr.parse().unwrap();
    // let _account: H160 = _account.parse().unwrap();
    let contract = Contract::from_json(eth, contr_addr, &abi)?;
    let secretkey = SecretKey2::from_str(sec_key).unwrap();

    let mut opt = Options::default();
    opt.gas = Some(gas.into());
    opt.gas_price = Some(gas_price.into());

    let transaction_hash = contract.signed_call("store", (12345u32,), opt, &secretkey).await?;

    Ok(transaction_hash)
}

async fn contract_query(
    eth: Eth<Http>,
    contr_addr: &str,
    // _account: &str,
    abi_path: &str,
    _args: String,
) -> web3::contract::Result<U256> {
    let abi = fs::read(abi_path).unwrap();
    let contr_addr: H160 = contr_addr.parse().unwrap();
    // let _account: H160 = _account.parse().unwrap();

    let contract = Contract::from_json(eth, contr_addr, &abi)?;
    // let _secretkey = SecretKey::from_str(_sec_key).unwrap();
    let opt = Options::default();

    let result = contract.query("retrieve", (), None, opt, None).await?;
    Ok(result)
}

async fn max_tasks_update(mut rx: Receiver<()>) {
    loop {
        let res_queue_secs = RES_QUEUE_SECS.lock().await;
        let average_time_queue = &res_queue_secs.2;
        if average_time_queue.len() > 1 {
            let end_cost_time = average_time_queue.iter().last().unwrap();
            let mut big: u8 = 0;
            let mut less: u8 = 0;

            for cost in average_time_queue.iter().rev().skip(1) {
                if end_cost_time > cost && (end_cost_time - *cost) > DELTA_RANGE {
                    big += 1;
                }
                if end_cost_time < cost && (*cost - end_cost_time) > DELTA_RANGE {
                    less += 1;
                }
            }

            if big > less {
                MAX_TASKS.store(2 * MAX_TASKS.load(Ordering::Acquire), Ordering::Release);
            } else if big < less {
                MAX_TASKS.store(MAX_TASKS.load(Ordering::Acquire) - 1, Ordering::Release);
            } else {
                let end_cost_time2 = average_time_queue.iter().rev().skip(1).rev().last().unwrap();

                if end_cost_time > end_cost_time2 && end_cost_time - end_cost_time2 > DELTA_RANGE {
                    MAX_TASKS.store(2 * MAX_TASKS.load(Ordering::Acquire), Ordering::Release);
                } else if end_cost_time < end_cost_time2 && end_cost_time2 - end_cost_time > DELTA_RANGE {
                    MAX_TASKS.store(MAX_TASKS.load(Ordering::Acquire) - 1, Ordering::Release);
                }
            }
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(UPDATE_INTERVAL)).await;
        if rx.try_recv().is_ok() {
            break;
        }
    }
}

async fn update_res_queue_secs(interval: u128) {
    let mut res_queue_secs = RES_QUEUE_SECS.lock().await;

    res_queue_secs.0 += 1;
    res_queue_secs.1 += interval;
    let aveage_time = res_queue_secs.1 / res_queue_secs.0 as u128;
    res_queue_secs.2.push(aveage_time);

    while res_queue_secs.2.len() > RES_QUEUE_MAX_LEN {
        res_queue_secs.2.pop();
    }
}

fn get_timestamp() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};

    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(n) => n.as_millis(),
        Err(_) => panic!("SystemTime before UNIX EPOCH!"),
    }
}
