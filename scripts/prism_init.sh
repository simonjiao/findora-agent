#!/usr/bin/env bash

# initialize
stt init -s

# deposit to the following eth account some token before `prismxx_init_height`
fn contract-deposit --addr 0x72488baa718f52b76118c79168e55c209056a2e6 --amount 10000000

sleep 30

rm -rf .openzeppelin/

# then deploy the prismxx contract
npx hardhat run scripts/deploy.js --network qa05

# $ stt init -s
# >>> Block interval: 16 seconds
# >>> Define and issue FRA ...
# a15252970b8aff738c1175bf44c2ee655edf1c8ca762c21b1d3732784884b442
# >>> Wait 1.2 block ...
# >>> DONE !
# $ fn contract-deposit --addr 0x72488baa718f52b76118c79168e55c209056a2e6 --amount 10000000
# 8008eeef2b9df199cdd1af9eb26644b12b391e16f58338404afc8d2577d72f32
# Note:
#         Your operations has been executed without local error,
#         but the final result may need an asynchronous query.
# $ sleep 30
# $ npx hardhat run scripts/deploy.js --network qa05
# Bridge address is: 0x5f9552fEd754F20B636C996DaDB32806554Bb995
# asset address is: 0xeE8Ffb1D3CE088A2415f1F9C00585a296EE063B7
# ledger address is: 0xa897D081bf941bBD60E831EDFE219D5887eFC755
