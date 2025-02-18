# mempool-util

The goal of the project is to create tools for mempool analysis. Areas of inquiry include:

### Mempool
- Retrospective block audit
    - How does the composition of the last confirmed block compare to our node's recent block template? If a discrepancy exists between expected and actual block composition, ask why.
- Fee reporting / estimation
    - emulate block assembly from raw mempool
    - track the spread between Core's historical estimator and our own near-term analysis
    - give context for rising/falling fee environment
- Cluster analysis
    - not implemented
- Taproot adoption

### Ideas for experimental writeups:
- Two flavors of research:
    - real-time mempool analysis, and the state of p2p
    - historical observation (replicable)

### Disclaimer
Please note this is experimental software - there will be bugs.
