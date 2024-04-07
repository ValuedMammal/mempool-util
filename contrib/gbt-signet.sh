#!/bin/bash
set -euo pipefail

# Run `getblocktemplate` at a regular interval, e.g. 5min
# collecting txids and writing to txt file

bcli=$(which bitcoin-cli)
path="Library/Application Support/Bitcoin/signet/bitcoind.pid"
pid="${HOME}/${path}"

# Block template refresh interval
interval=3
interval_secs=$((interval * 60))

# Sync local time with interval before starting
min=$(date +%M)
now=$(date +%S)
rem=$((min % interval))

if ! { [ $now -eq 0 ] && [ $rem -eq 0 ]; }; then
    delay_min=$((interval - rem))
    delay_sec=$((delay_min * 60))
    n=$((delay_sec - now))

    echo "waiting ${n} seconds"
    sleep $n
fi
date

if [ -f "$pid" ]; then
    echo 'Running gbt'

    while true
    do
        $bcli -signet getblocktemplate '{"rules": ["segwit", "signet"]}' \
            | jq -r '.transactions[].txid' > ~/mempool-util/gbt.txt
        
        sleep $interval_secs;
    done
else
    echo 'None'
fi
