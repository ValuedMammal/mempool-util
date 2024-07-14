#!/bin/bash

# Script that figures the duration in minutes between the block
# of the given blockhash and its immediate predecessor

if [[ $# -lt 1 ]]; then
    echo "Usage: blocktime <hash>"
    exit 1
fi

hash=$1

bitcoin-cli getblockheader "$hash" > /tmp/header.json
time=$(jq -r '.time' /tmp/header.json)

prev_hash=$(jq -r '.previousblockhash' /tmp/header.json)
prev_time=$(bitcoin-cli getblockheader "$prev_hash" | jq -r '.time')

diff=$((time-prev_time))

# round up
min=$(((diff+59)/60))

echo $min

rm -f /tmp/header.json
