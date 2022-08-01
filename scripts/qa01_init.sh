#!/usr/bin/env bash

feth_exec="RUST_LOG=info cargo run --release --"

fn setup -S https://dev-qa01.dev.findora.org
stt init --skip-validator
fn show -b

# keypair
#   ae68980cd3994f1e314e3805c3775ecdf4e3446103a76131ca31f115404dd336
#   0x2c52767e6772ddf3f63bb92e08ffe8cca5aa8f5f
fn contract-deposit --addr 0x2c52767e6772ddf3f63bb92e08ffe8cca5aa8f5f --amount 5000000000000

# wait for 3 blocks
sleep 45

$(feth_exec) -- fund --network qa01 --load --amount 3000 --redeposit --timeout 100 --count 500
