from datetime import datetime
import logging
from math import sqrt
from pathlib import Path
import re
from statistics import mean, median, mode, variance
import sys

"""
Parses a log with records resembling the following:
    2023-10-16T12:53:38.657Z INFO  mempool > {"block_fees":0.21,"block_score":45.8,"hash":"000000000000000000022b4f8a6a1eb8a9779e74ae1e72a68ae2f7b391e2a7c1"}

First, compute the time between consecutive entries, indicating the time elapsed between blocks.
Throw out any entries for which the block time is less than the configured block template interval *and*
no refresh occurred in the interim. For example, if we run Core's `getblocktemplate` at 5min intervals
starting at 00:00, and we see block 1 at 00:01 and block 2 at 00:02, we throw out the score for block 2, as
we'd be working from a stale template. However, if we see block 1 at 00:03 and block 2 at 00:06, we keep
both entries despite the block time being less than the template interval, as presumably a new
template would have been run at 00:05.

Finally, compute the average, median, mode, and standard deviation of block score. A robust dataset
should include at least 1000 entries.

Usage: python3 data.py path/to/input.log path/to/output.csv
"""

# Block template refresh interval
INTERVAL = 5
INTERVAL_SECS = INTERVAL * 60

# Regex patterns for capturing datetimes and block scores
RE_DATETIME = re.compile("^.*([\d]{4})-([\d]{2})-([\d]{2})T([\d]{2}):([\d]{2}):([\d]{2}).([\d]{3}).*$")
RE_BLOCK_HASH = re.compile('^.*"hash":"([0-9a-f]{64})".*$')
RE_SCORE = re.compile('^.*"block_fees":([\d.]+),"block_score":([\d.]+).*$')

# Setup console logger
log = logging.getLogger('my data')
log.setLevel(logging.DEBUG)
log_fmt = logging.Formatter('%(levelname)s - %(message)s')
chan = logging.StreamHandler()
chan.setFormatter(log_fmt)
chan.setLevel(logging.DEBUG)
log.addHandler(chan)

def main():
    args = sys.argv
    if len(args) < 3:
        print("Usage: data.py <in_file> <out_file>")
        exit(1)
    
    in_file = Path(args[1])
    out_file = Path(args[2])
    
    # Read in file
    records = []
    with open(in_file, 'r') as f:
        records = f.readlines()

    table = parse_input(records)
    out_lines = validate(table)
    
    # Write out file
    with open(out_file, 'w') as f:
        f.writelines(out_lines)

    log.info('Done')


def parse_input(records: list[str]) -> list[dict]:
    """
    Creates a table for holding data prior to validation, where a row has the following fields:
    row = {
        "datetime": datetime,
        "timestamp": float,
        "fees": float,
        "score": float,
        "hash": str,
    }
    """
    
    tb = []
    for ln in records:
        # Drop null
        if RE_DATETIME.match(ln) is None:
            log.debug("skipping line: {}".format(ln.rstrip()))
            continue

        # Match datetime
        match = RE_DATETIME.search(ln)
        l = [int(cap) for cap in match.groups()]
        (y, mo, d, h, m, s, ms) = l
        dt = datetime(y, mo, d, h, m, s, microsecond=ms*1000)

        # Match hash
        match = RE_BLOCK_HASH.search(ln)
        hash = match.group(1)
        
        # Match fees + score
        fees = None
        score = None
        match = RE_SCORE.search(ln)
        if match is not None:
            fees = float(match.group(1))
            score = float(match.group(2))

        # Create row
        d = {
            "datetime": dt,
            "timestamp": dt.timestamp(),
            "fees": fees,
            "score": score,
            "hash": hash,
        }
        tb.append(d)
    
    return tb


def validate(table: list[dict]) -> list[str]:
    """
    Collect scores, filtering any invalid, and return a list
    of csv rows
    """

    raw_len = len(table)

    # Prepare output as csv
    header = "block_time,fees,score,block_hash\n"
    out_lines: list[str] = [header]
    
    block_scores: list[float] = []
    for (i, row) in enumerate(table):
        # collect first row, but skip computing blocktime
        fees = row["fees"]
        score = row["score"]
        hash = row["hash"]
        if i == 0: 
            out_lines.append(f"None,{fees},{score},{hash}\n")
            if score is not None:
                block_scores.append(score)
            continue

        valid = True
        prev_row = table[i-1]
        t0 = prev_row["timestamp"]
        t1 = row["timestamp"]
        elapsed = t1 - t0
        elapsed_min = round((elapsed / 60.0), 2)
        if elapsed < INTERVAL_SECS:
            valid = False
            # Toss the entry, unless the duration includes a multiple of 5, which we test
            # by finding the range (prev_min, cur_min], and checking whether any of the
            # intervening minutes modulo `INTERVAL` is 0.
            #
            # Some edge cases:
            # - Check for and allow the case where a new hour has passed.
            # - Disallow case where two blocks arrive in the same minute.
            #
            # If `valid` is false, drop this row's score, but keep the previous
            m0 = prev_row["datetime"].minute
            m1 = row["datetime"].minute
            if m0 > m1:
                # passed new hour
                valid = True
            elif m0 < m1:
                for m in range(m0 + 1, m1 + 1):
                    if m % INTERVAL == 0:
                        valid = True
                        break
        if valid:
            out_lines.append(f"{elapsed_min},{fees},{score},{hash}\n")
            if score is not None:
                block_scores.append(score)
        #else:
        #   log.debug(f"dropping record with range: ({m0}-{m1})")
    
    build_result(block_scores, raw_len)
    return out_lines


def build_result(scores: list[float], raw_len: int):
    """Crunch stats"""
    valid_ln = len(scores)
    avg_score = round(mean(scores), 1)
    med_score = round(median(scores), 1)
    var_score = variance(scores, avg_score)
    std_score = round(sqrt(var_score), 1)
    min_score = min(scores)
    max_score = max(scores)
    mode_score = mode(scores)

    res = {
        "total": raw_len,
        "valid": valid_ln,
        "min score": min_score,
        "max score": max_score,
        "median score": med_score,
        "mode score": mode_score,
        "avg score": avg_score,
        "stdev score": std_score,
    }

    print(res)

main()