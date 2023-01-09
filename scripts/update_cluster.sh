#!/usr/bin/env bash

if [ -z "$root_dir" ];then
    root_dir=/tmp/findora
fi

MAX_NODE=20

mode=$1

case "$mode" in
"single")
    nodes=$2
    consensus=$3
    ;;
"seq")
    consensus=$2
    ;;
"fast_half")
    consensus=$2
    ;;
"fast_two_third")
    consensus=$2
    ;;
"swarm_reboot")
    consensus=$2
    ;;
*)
    exit 1
    ;;
esac


if [ -z "$consensus" ]; then
    consensus="one"
fi

if [ "$mode" != "single" ]; then
    for ((i=0; i<MAX_NODE; i++)); do
        nodes="$nodes node$i";
    done
fi

if [ -z "$nodes" ]; then
    echo "Empty node"
    exit 1
fi

. ./update_node_env.sh

for node in $nodes; do
    file="$root_dir/devnet/$node/config/config.toml"
    update_node_env "$consensus" "$file"
    restart_node "$node" 5
done
