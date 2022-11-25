#!/usr/bin/env bash

ENV=prod
NAMESPACE=mainnet
SERV_URL=https://${ENV}-${NAMESPACE}.${ENV}.findora.org
IMG_PREFIX='public.ecr.aws/k6m5b6e2/release/findorad'

. ./node_env.sh

START_MODE=$1
RUNNER=$2

case "${START_MODE}" in
"initConfig")
  echo "initial tendermint config..."
  ;;
"restart")
  echo "restarting local node"
  ;;
*)
  echo "invalid start mode"
  exit 1
  ;;
esac

case "${RUNNER}" in
"native")
  echo "using native binary"
  ;;
"img")
  if ! NODE_IMG=$(net_image); then
      exit 2
  fi
  echo "using docker image $NODE_IMG"
  ;;
*)
  echo "invalid runner"
  exit 1
  ;;
esac

[ -n "$ROOT_DIR" ] || ROOT_DIR=/data/findora/"$NAMESPACE"

if [ "${START_MODE}" = "initConfig" ]; then
    init_tendermint_config "${ROOT_DIR}" "${NODE_IMG}"
fi

echo; echo; echo "done"; echo
