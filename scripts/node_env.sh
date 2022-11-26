net_image() {
    VER=$1
    if [ -n "$VER" ]; then
        NODE_IMG="$IMG_PREFIX:$VER"
    elif VER=$(curl -s "$SERV_URL":8668/version); then
        if VER=$(echo "$VER" | awk '{print $2}'); then
            NODE_IMG="$IMG_PREFIX:$VER"
        else
            echo "Invalid image version"
            return 2
        fi
    else
        echo "Failed to obtain image version"
        return 1
    fi
    echo "$NODE_IMG"
    return 0
}

get_chain_id() {
    if ChainID=$(curl -s -X POST -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_chainId","id":1}' \
        "$SERV_URL:8545"); then
        HEX=$(echo "$ChainID" | jq -r .result | awk -F'x' '{print $2}')
        if ! EVM_CHAIN_ID=$(echo "obase=10; ibase=16; $HEX" | bc); then
            echo "Invalid Evm chain id"
            return 1
        fi
    fi
    echo "$EVM_CHAIN_ID"
    return 0
}

init_tendermint_config() {
    ROOT_DIR=$1
    NODE_IMG=$2
    # backup old data and config files
    if [ -d "${ROOT_DIR}"/tendermint/config ]; then
        sudo mv "${ROOT_DIR}"/tendermint/config "${ROOT_DIR}"/tendermint/config.bak || exit 1
    fi

    sudo docker run --rm -v "${ROOT_DIR}"/tendermint:/root/.tendermint "${NODE_IMG}" init --${NAMESPACE} || exit 1
    sudo chown -R "$(id -u)":"$(id -g)" "${ROOT_DIR}"/tendermint/
}

prepare_snapshot_data() {
    ROOT_DIR=$1
    # download latest link and get url
    wget -O "${ROOT_DIR}/latest" "https://${ENV}-${NAMESPACE}-us-west-2-chain-data-backup.s3.us-west-2.amazonaws.com/latest"
    CHAINDATA_URL=$(cut -d , -f 1 "${ROOT_DIR}/latest")

    # remove old data
    rm -rf "${ROOT_DIR}/findorad"
    rm -rf "${ROOT_DIR}/tendermint/data"
    rm -rf "${ROOT_DIR}/tendermint/config/addrbook.json"

    wget -O "${ROOT_DIR}/snapshot" "${CHAINDATA_URL}"
    mkdir "${ROOT_DIR}/snapshot_data"
    echo "extracting snapshot data..."
    tar zxf "${ROOT_DIR}/snapshot" -C "${ROOT_DIR}/snapshot_data"

    echo "moving data to right place..."
    mv "${ROOT_DIR}/snapshot_data/data/ledger" "${ROOT_DIR}/findorad"
    mv "${ROOT_DIR}/snapshot_data/data/tendermint/mainnet/node0/data" "${ROOT_DIR}/tendermint/data"
    cp "${ROOT_DIR}/checkpoint.toml" "${ROOT_DIR}/findorad/"

    rm -rf "${ROOT_DIR}"/snapshot_data
}

docker_run(){
   ROOT_DIR=$1
   NODE_IMG=$2
   sudo docker rm -f findorad || exit 1
    sudo docker run -d \
        -v "${ROOT_DIR}"/tendermint:/root/.tendermint \
        -v "${ROOT_DIR}"/findorad:/tmp/findora \
        -p 8669:8669 \
        -p 8668:8668 \
        -p 8667:8667 \
        -p 26657:26657 \
        -p 8545:8545 \
        -e EVM_CHAIN_ID="$EVM_CHAIN_ID" \
        -e RUST_LOG="abciapp=info,baseapp=debug,account=info,ethereum=debug,evm=debug,eth_rpc=debug" \
        --name findorad \
        "$NODE_IMG" node \
        --ledger-dir /tmp/findora \
        --checkpoint-file /tmp/findora/checkpoint.toml \
        --tendermint-host 0.0.0.0 \
        --tendermint-node-key-config-path="/root/.tendermint/config/priv_validator_key.json" \
        --enable-eth-api-service
}

native_run() {
    ROOT_DIR=$1
    EVM_CHAIN_ID=$2
    TRACE=$3
    FRESH=$4

    TRACE_OPTIONS="${TRACE}"
    if [ "${FRESH}" ]; then
        TRACE_OPTIONS="$TRACE_OPTIONS --fresh"
    fi

    EVM_CHAIN_ID="$EVM_CHAIN_ID" \
        RUST_LOG="abciapp=info,baseapp=debug,account=debug,ethereum=debug,evm=debug,eth_rpc=debug" \
        abcid --submission-service-port 8669 \
        --trace TRACE_OPTIONS \
        --ledger-service-port 8668 \
        --ledger-dir "${ROOT_DIR}"/findorad \
        --enable-eth-api-service \
        --tendermint-node-key-config-path "${ROOT_DIR}"/tendermint/config/priv_validator_key.json \
        >>"${ROOT_DIR}"/abcid.log 2>&1 &

    tendermint node --home "${ROOT_DIR}"/tendermint --fast_sync=true >>"${ROOT_DIR}"/tendermint.log 2>&1 &
}

node_info() {
    curl -s 'http://localhost:26657/status' | jq -r .result.node_info.network
    curl -s 'http://localhost:8668/version'
    echo

    curl -s -X POST -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_hashrate","id":1}' \
    'http://localhost:8545'
    echo
}
