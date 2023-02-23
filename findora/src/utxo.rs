mod utils {
    use crate::{Error, Result};
    use finutils::{common::utils, wallet, zei};
    use std::{path::Path, str::FromStr};
    use tendermint::block::Height;
    use tendermint_rpc::{endpoint::abci_query::AbciQuery, Client, HttpClient};
    use tokio::runtime::Runtime;
    pub(super) use utils::{gen_transfer_op, new_tx_builder, send_tx};
    use wallet::restore_keypair_from_mnemonic_default;
    use zei::xfr::sig::XfrKeyPair;

    pub(crate) fn restore_keypair<P>(mn_path: P) -> Result<XfrKeyPair>
    where
        P: AsRef<Path>,
    {
        let m = std::fs::read_to_string(mn_path).map_err(Error::Io)?;
        restore_keypair_from_mnemonic_default(m.trim()).map_err(|o| Error::Other(o.to_string()))
    }

    pub(crate) fn one_shot_abci_query(
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
    use finutils::common::get_serv_addr;
    use finutils::{fp_types, fp_utils, ledger, zei};

    use fp_types::{
        actions::{
            xhub::{self, NonConfidentialOutput, NonConfidentialTransfer},
            Action,
        },
        assemble::{CheckFee, CheckNonce},
        crypto::{Address, MultiSignature, MultiSigner},
        transaction::UncheckedTransaction,
        U256,
    };
    use fp_utils::{ecdsa::SecpPair, tx::EvmRawTxWrapper};
    use ledger::data_model::{ASSET_TYPE_FRA, BLACK_HOLE_PUBKEY_STAKING};
    use std::{path::Path, str::FromStr};
    use tendermint_rpc::Client;
    use tokio::runtime::Runtime;
    use zei::xfr::{
        asset_record::AssetRecordType,
        sig::{XfrKeyPair, XfrPublicKey},
    };

    enum Keypair {
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

    pub(crate) fn deposit<P>(src_mn: P, target_addr: &str, amount: u64) -> Result<()>
    where
        P: AsRef<Path>,
    {
        let kp = utils::restore_keypair(src_mn)?;
        let mut builder = utils::new_tx_builder().map_err(|o| Error::Prism(o.to_string()))?;

        let transfer_op = utils::gen_transfer_op(
            &kp,
            vec![(&BLACK_HOLE_PUBKEY_STAKING, amount)],
            None,
            false,
            false,
            Some(AssetRecordType::NonConfidentialAmount_NonConfidentialAssetType),
        )
        .map_err(|o| Error::Prism(o.to_string()))?;

        let target_address = MultiSigner::from_str(target_addr).map_err(|o| Error::Prism(o.to_string()))?;

        builder
            .add_operation(transfer_op)
            .add_operation_convert_account(&kp, target_address, amount)
            .map_err(|o| Error::Prism(o.to_string()))?
            .sign(&kp);

        let mut tx = builder.take_transaction();
        tx.sign_to_map(&kp);

        utils::send_tx(&tx).map_err(|o| Error::Prism(o.to_string()))
    }

    pub(crate) fn withdraw<P>(src_eth_mn: P, target_pk: XfrPublicKey, amount: u64) -> Result<()>
    where
        P: AsRef<Path>,
    {
        let output = NonConfidentialOutput {
            target: target_pk,
            amount,
            asset: ASSET_TYPE_FRA,
        };

        let eth_mn = std::fs::read_to_string(src_eth_mn)?;
        let kp = SecpPair::from_phrase(eth_mn.trim(), None)
            .map_err(|o| Error::Prism(o.to_string()))?
            .0;
        let signer = Address::from(kp.address());
        let kp = Keypair::Ecdsa(kp);

        let serv_addr = get_serv_addr().map_err(|o| Error::Prism(o.to_string()))?;
        let tm_client = tendermint_rpc::HttpClient::new(format!("{serv_addr}:26657").as_str()).unwrap();

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
}
