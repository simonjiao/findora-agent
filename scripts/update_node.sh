#!/usr/bin/env bash

root_dir=/tmp/findora
node=$1

file=${root_dir}/devnet/${node}/config/config.toml

perl -pi -e 's/(timeout_propose = )".*"/$1"9s"/' $file
perl -pi -e 's/(timeout_propose_delta = )".*"/$1"1500ms"/' $file
perl -pi -e 's/(timeout_prevote = )".*"/$1"3s"/' $file
perl -pi -e 's/(timeout_prevote_delta = )".*"/$1"1500ms"/' $file
perl -pi -e 's/(timeout_precommit = )".*"/$1"3s"/' $file
perl -pi -e 's/(timeout_precommit_delta = )".*"/$1"1500ms"/' $file
perl -pi -e 's/(timeout_commit = )".*"/$1"3s"/' $file

# kill related process and wait 5 seconds
pids=$(ps -ef|grep devnet/node0|grep -v grep|awk '{print $2}'); for pid in ${pids}; do kill -9 "$pid"; done
sleep 5

SelfAddr=$(grep 'address' ${root_dir}/devnet/"${node}"/config/priv_validator_key.json | grep -oE '[^",]{40}');
#ENABLE_QUERY_SERVICE=true \
TD_NODE_SELF_ADDR=$SelfAddr \
RUST_LOG=info \
LEDGER_DIR=$root_dir/devnet/$node/abci \
ENABLE_ETH_API_SERVICE=true \
ARC_HISTORY=4,2 \
abcid "${root_dir}/devnet/${node}" >> "$root_dir/devnet/$node/abcid.log" 2>&1 &

sleep 5
tendermint node --home "${root_dir}/devnet/${node}" >> "$root_dir/devnet/$node/consensus.log" 2>&1 &
