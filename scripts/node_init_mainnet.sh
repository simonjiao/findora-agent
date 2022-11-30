#!/usr/bin/env bash

ENV=prod
NAMESPACE=mainnet
SERV_URL=https://${ENV}-${NAMESPACE}.${ENV}.findora.org
IMG_PREFIX='public.ecr.aws/k6m5b6e2/release/findorad'
IMG_DEV_PREFIX='public.ecr.aws/k6m5b6e2/dev/findorad'

. ./node_env.sh

MODE=unset
RUNNER=unset
IMG=""
DIR=/data/findora/${NAMESPACE}
CHAIN=""
TRACE=90
FRESH=false

usage()
{
  echo "Usage: node [ -m | --mode ]
                    [ -r | --runner ]
                    [ -i | --img IMG ]
                    [ -d | --dir DIR ]"
  exit 2
}

PARSED_ARGUMENTS=$(getopt -a -n alphabet -o hfm:r:i:d:c:t: --long help,fresh,mode:,runner:,img:,dir:,chain:,trace: -- "$@")
VALID_ARGUMENTS=$?
if [ "$VALID_ARGUMENTS" != "0" ]; then
  usage
fi

echo "PARSED_ARGUMENTS is $PARSED_ARGUMENTS"
eval set -- "$PARSED_ARGUMENTS"
while :
do
  case "$1" in
    -h | --help)    usage       ; shift   ;;
    -f | --fresh)   FRESH=true  ; shift   ;;
    -m | --mode)    MODE="$2"   ; shift 2 ;;
    -r | --runner)  RUNNER="$2" ; shift 2 ;;
    -i | --img)     IMG="$2"    ; shift 2 ;;
    -d | --dir)     DIR="$2"    ; shift 2 ;;
    -c | --chain)   CHAIN="$2"    ; shift 2 ;;
    -t | --trace)   TRACE="$2"    ; shift 2 ;;
    # -- means the end of the arguments; drop this, and break out of the while loop
    --) shift; break ;;
    # If invalid options were passed, then getopt should have reported an error,
    # which we checked as VALID_ARGUMENTS when getopt was called...
    *) echo "Unexpected option: $1 - this should not happen."
       usage ;;
  esac
done


case "${MODE}" in
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
  if ! NODE_IMG=$(net_image "${IMG}"); then
      exit 2
  fi
  echo "using docker image $NODE_IMG"
  ;;
*)
  echo "invalid runner"
  exit 1
  ;;
esac

if [ "${MODE}" = "initConfig" ]; then
    init_tendermint_config "${DIR}" "${NODE_IMG}"
elif [ "${MODE}" = "restart" ]; then
    [ -n "${CHAIN}" ] || CHAIN=$(get_chain_id)
    if [ "${RUNNER}" = "native" ]; then
        native_run "${DIR}" "${CHAIN}" "${TRACE}" "${FRESH}"
    else
        docker_run "${NODE_IMG}" "${DIR}" "${CHAIN}" "${TRACE}" "${FRESH}"
    fi
fi

sleep 15; node_info

echo; echo; echo "done"; echo
