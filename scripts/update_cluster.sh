#!/usr/bin/env bash

if [ -z "$root_dir" ];then
    root_dir=/tmp/findora
fi

MAX_NODE=20

mode=$1

case "$mode" in
"single" | "seq" | "fast_half" | "fast_two_third" |"swarm_reboot")
    ;;
*)
    echo "Invalid mode \"$mode\""
    exit 1
    ;;
esac

rm -f checkpoint.toml

if [ "$mode" != "single" ]; then
    for ((i=0; i<=MAX_NODE; i++)); do
        nodes="$nodes node$i";
    done
    interval=$2
    if [ -z "$interval" ]; then
        interval=30
    fi
fi

consensus=$3
if [ -z "$consensus" ]; then
    consensus="one"
fi

. ./update_node_env.sh

if [ "$mode" == "fast_half" ]; then
    for ((i=0; i<21; i++)); do
        if (( i > 11 )); then
            sleep 1200
        else
            sleep "$interval"
        fi
        node="node$i"
        file="$root_dir/devnet/$node/config/config.toml"
        update_consensus "$consensus" "$file"
        echo "current_height $(current_height) before restart"
        restart_node "$node" 5
        echo "current_height $(current_height) after restart"
    done
elif [ "$mode" == "fast_two_third" ]; then
    for ((i=0; i<21; i++)); do
        if (( i > 14 )); then
            sleep 1200
        else
            sleep "$interval"
        fi
        file="$root_dir/devnet/$node/config/config.toml"
        update_consensus "$consensus" "$file"
        echo "current_height $(current_height) before restart"
        restart_node "$node" 5
        echo "current_height $(current_height) after restart"
    done
elif [ "$mode" == "swarm_reboot" ]; then
    for node in $nodes; do
        file="$root_dir/devnet/$node/config/config.toml"
        update_consensus "$consensus" "$file"
    done

    echo "current_height $(current_height) before restart"
    for node in $nodes; do
        restart_node "$node" 5 &
        sleep 5
    done
    echo "current_height $(current_height) after restart"
elif [ "$mode" == "seq" ];  then
    for node in $nodes; do
        file="$root_dir/devnet/$node/config/config.toml"
        update_consensus "$consensus" "$file"
        echo "current_height $(current_height) before restart"
        restart_node "$node" 5
        echo "current_height $(current_height) after restart"
        sleep "$interval"
    done
elif [ "$mode" == "single" ]; then
    node=$2
    file="$root_dir/devnet/$node/config/config.toml"
    update_consensus "$consensus" "$file"
    echo "current_height $(current_height) before restart"
    restart_node "$node" 5
    echo "current_height $(current_height) after restart"
else
    echo "mode \"$mode\" not support yet!"
fi
