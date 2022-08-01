#!/usr/bin/env bash

set -e

#endpoint00="http://dev-qa01-us-west-2-full-000-open.dev.findora.org:8545"
endpoint01="http://dev-qa01-us-west-2-full-001-open.dev.findora.org:8545"
#submission="https://dev-qa01.dev.findora.org:8669"

#cd "$script_dir"/..

run_one_test() {
    endpoint=$1
    count=$2
    concurrency=$3
    timeout=$4
    waittime=$5
    source=$6
    logfile=$7

    {
        echo "$endpoint $count $concurrency $timeout"
        RUST_LOG=info ./feth --network "$endpoint" --source "$source" --count "$count" --max-parallelism "$concurrency" --timeout "$timeout" 2>&1
        echo "wait for $waittime seconds to make sure all previous txns have been finished..."
    } >>"$logfile"
    sleep "$waittime"
}

switch_profiler() {
    endpoint=$1
    enable=$2
    logfile=$3

    if [ "$enable" ]; then
        { RUST_LOG=info ./feth profiler --network "$endpoint" --enable >> "$logfile"; } 2>&1
    else
        { RUST_LOG=info ./feth profiler --network "$endpoint" >>"$logfile"; } 2>&1
    fi

}

#switch_profiler "$endpoint01" true "test01.log"

for ((i = 0; i < 10000; i++)); do
    run_one_test "$endpoint01" 1 200 100 5 "source_keys.001" "test01.log"
done

#switch_profiler "$endpoint01" false "test01.log"

#for ((i = 0; i < 10000; i++)); do
#    run_one_test "$endpoint01" 1 300 100 1 "source_keys.002" "test02.log"
#done &
#
#for ((i = 0; i < 10000; i++)); do
#    run_one_test "$endpoint00" 1 300 100 1 "source_keys.003" "test03.log"
#done &
#
#for ((i = 0; i < 10000; i++)); do
#    run_one_test "$endpoint01" 1 300 100 1 "source_keys.004" "test04.log"
#done
