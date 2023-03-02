pub use prism::*;
pub use utils::*;

mod utils {
    pub(crate) use crate::{Error, Result};
    pub(super) use finutils::{common::utils, fp_utils, wallet, zei};
    use fp_utils::ecdsa::SecpPair;
    use std::{path::Path, str::FromStr};
    use tendermint::block::Height;
    use tendermint_rpc::{endpoint::abci_query::AbciQuery, Client, HttpClient};
    use tokio::runtime::Runtime;
    pub(super) use utils::{gen_transfer_op, new_tx_builder, send_tx_to};
    pub use wallet::{public_key_from_base64, restore_keypair_from_mnemonic_default};
    pub(super) use zei::xfr::sig::{XfrKeyPair, XfrPublicKey};

    /// fra 24, en
    pub fn generate_mnemonic(count: usize, wordslen: u8, lang: &str) -> Result<Vec<String>> {
        let mut mns = Vec::with_capacity(count);
        while mns.len() < count {
            let mnemonic = wallet::generate_mnemonic_custom(wordslen, lang).map_err(|o| Error::Other(o.to_string()))?;
            mns.push(mnemonic);
        }
        Ok(mns)
    }

    pub fn restore_fra_keypair<P>(mn_path: P) -> Result<XfrKeyPair>
    where
        P: AsRef<Path>,
    {
        let m = std::fs::read_to_string(mn_path).map_err(Error::Io)?;
        restore_keypair_from_mnemonic_default(m.trim()).map_err(|o| Error::Other(o.to_string()))
    }

    pub fn restore_eth_keypair<P>(mn_path: P) -> Result<SecpPair>
    where
        P: AsRef<Path>,
    {
        let eth_mn = std::fs::read_to_string(mn_path)?;
        let kp = SecpPair::from_phrase(eth_mn.trim(), None)
            .map_err(|o| Error::Prism(o.to_string()))?
            .0;
        Ok(kp)
    }

    pub fn restore_xfr_pk_from_str(pk: &str) -> Result<XfrPublicKey> {
        public_key_from_base64(pk).map_err(|o| Error::Prism(o.to_string()))
    }

    pub(super) fn one_shot_abci_query(
        tm_client: &HttpClient,
        path: &str,
        data: Vec<u8>,
        height: Option<Height>,
        prove: bool,
    ) -> Result<AbciQuery> {
        let path = if path.is_empty() {
            None
        } else {
            Some(tendermint::abci::Path::from_str(path).unwrap())
        };

        let query_ret = Runtime::new()?
            .block_on(tm_client.abci_query(path, data, height, prove))
            .map_err(|o| Error::Prism(o.to_string()))?;

        if query_ret.code.is_err() {
            Err(Error::Prism(format!(
                "error code: {:?}, log: {}",
                query_ret.code, query_ret.log
            )))
        } else {
            Ok(query_ret)
        }
    }
}

mod prism {
    use super::utils;
    use crate::{Error, Result};
    use finutils::{fp_types, fp_utils, ledger, zei};

    use fp_types::{
        actions::{
            xhub::{self, NonConfidentialOutput, NonConfidentialTransfer},
            Action,
        },
        assemble::{CheckFee, CheckNonce},
        crypto::{Address, MultiSignature, MultiSigner},
        transaction::UncheckedTransaction,
        H160, U256,
    };
    use fp_utils::{ecdsa::SecpPair, tx::EvmRawTxWrapper};
    use ledger::data_model::{ASSET_TYPE_FRA, BLACK_HOLE_PUBKEY_STAKING};
    use tendermint_rpc::Client;
    use tokio::runtime::Runtime;
    use zei::xfr::{
        asset_record::AssetRecordType,
        sig::{XfrKeyPair, XfrPublicKey},
    };

    enum Keypair {
        #[allow(unused)]
        Ed25519(XfrKeyPair),
        Ecdsa(SecpPair),
    }

    impl Keypair {
        fn sign(&self, data: &[u8]) -> MultiSignature {
            match self {
                Keypair::Ecdsa(kp) => MultiSignature::from(kp.sign(data)),
                Keypair::Ed25519(kp) => MultiSignature::from(kp.get_sk_ref().sign(data, kp.get_pk_ref())),
            }
        }
    }

    pub fn deposit(endpoint: &str, src_kp: XfrKeyPair, target_addr: H160, amount: u64) -> Result<()> {
        let mut builder = utils::new_tx_builder().map_err(|o| Error::Prism(o.to_string()))?;

        let transfer_op = utils::gen_transfer_op(
            &src_kp,
            vec![(&BLACK_HOLE_PUBKEY_STAKING, amount)],
            None,
            false,
            false,
            Some(AssetRecordType::NonConfidentialAmount_NonConfidentialAssetType),
        )
        .map_err(|o| Error::Prism(o.to_string()))?;

        let target_address = MultiSigner::Ethereum(target_addr);

        builder
            .add_operation(transfer_op)
            .add_operation_convert_account(&src_kp, target_address, amount)
            .map_err(|o| Error::Prism(o.to_string()))?
            .sign(&src_kp);

        let mut tx = builder.take_transaction();
        tx.sign_to_map(&src_kp);

        utils::send_tx_to(&tx, Some(endpoint)).map_err(|o| Error::Prism(o.to_string()))
    }

    pub fn withdraw(endpoint: &str, src_kp: SecpPair, target_pk: XfrPublicKey, amount: u64) -> Result<()> {
        let output = NonConfidentialOutput {
            target: target_pk,
            amount,
            asset: ASSET_TYPE_FRA,
        };

        let signer = Address::from(src_kp.address());
        let kp = Keypair::Ecdsa(src_kp);

        let tm_client = tendermint_rpc::HttpClient::new(format!("{endpoint}:26657").as_str()).unwrap();

        let query_ret = utils::one_shot_abci_query(
            &tm_client,
            "module/account/nonce",
            serde_json::to_vec(&signer).unwrap(),
            None,
            false,
        )?;

        let nonce =
            serde_json::from_slice::<U256>(query_ret.value.as_slice()).map_err(|o| Error::Prism(o.to_string()))?;

        let account_call = xhub::Action::NonConfidentialTransfer(NonConfidentialTransfer {
            input_value: amount,
            outputs: vec![output],
        });
        let action = Action::XHub(account_call);
        let extra = (CheckNonce::new(nonce), CheckFee::new(None));
        let msg = serde_json::to_vec(&(action.clone(), extra.clone())).unwrap();

        let signature = kp.sign(msg.as_slice());

        let tx = UncheckedTransaction::new_signed(action, signer, signature, extra);
        let txn = serde_json::to_vec(&tx).unwrap();

        let txn_with_tag = EvmRawTxWrapper::wrap(&txn);

        Runtime::new()
            .unwrap()
            .block_on(tm_client.broadcast_tx_sync(txn_with_tag.into()))
            .map_err(|o| Error::Prism(o.to_string()))?;

        Ok(())
    }

    #[derive(Debug)]
    pub enum PrismOp {
        Deposit,
        WithDraw,
    }

    impl std::str::FromStr for PrismOp {
        type Err = String;

        fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
            match s.to_lowercase().trim() {
                "deposit" => Ok(Self::Deposit),
                "withdraw" => Ok(Self::WithDraw),
                n => Err(format!("invalid PrismOp {n}")),
            }
        }
    }
}

#[derive(Debug)]
pub enum NativeOp {
    Transfer,
    Delegate,
    Stake,
}

impl std::str::FromStr for NativeOp {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().trim() {
            "transfer" => Ok(NativeOp::Transfer),
            "delegate" => Ok(NativeOp::Delegate),
            "stake" => Ok(NativeOp::Stake),
            _ => Err("Unknown NativeOp".to_string()),
        }
    }
}

pub fn transfer(endpoint: &str, src_kp: XfrKeyPair, target_addr: XfrPublicKey, amount: u64) -> Result<()> {
    transfer_batch(endpoint, src_kp, vec![(&target_addr, amount)])
}

pub fn transfer_batch(endpoint: &str, src_kp: XfrKeyPair, target_list: Vec<(&XfrPublicKey, u64)>) -> Result<()> {
    let mut builder = new_tx_builder().map_err(|o| Error::Native(o.to_string()))?;
    let op = gen_transfer_op(
        &src_kp,
        target_list,
        None, // None for FRA,
        false,
        false,
        None,
    )
    .map_err(|o| Error::Native(o.to_string()))?;

    builder.add_operation(op);

    let mut tx = builder.take_transaction();
    tx.sign_to_map(&src_kp);

    send_tx_to(&tx, Some(endpoint)).map_err(|o| Error::Native(o.to_string()))
}
