```
=== PER SCENARIO METRICS ===
 ------------------------------------------------------------------------------
 Name                     |  # users |  # times run | scenarios/s | iterations
 ------------------------------------------------------------------------------
 1: CeramicWriteOnly      |        0 |        28417 |        7.89 |        inf
 ------------------------------------------------------------------------------
 Name                     |    Avg (ms) |        Min |         Max |     Median
 ------------------------------------------------------------------------------
   1: CeramicWriteOnly    |       77888 |     54,682 |     143,339 |     74,994

 === PER TRANSACTION METRICS ===
 ------------------------------------------------------------------------------
 Name                     |   # times run |        # fails |  trans/s |  fail/s
 ------------------------------------------------------------------------------
 1: CeramicWriteOnly
   1: setup               |         2,000 |  1,354 (67.7%) |     0.56 |    0.38
   2: update_small_model  |        29,063 |        99 (0%) |     8.07 |    0.03
   3: update_large_model  |        28,638 |     418 (1.5%) |     7.95 |    0.12
 -------------------------+---------------+----------------+----------+--------
 Aggregated               |        59,701 |   1,871 (3.1%) |    16.58 |    0.52
 ------------------------------------------------------------------------------
 Name                     |    Avg (ms) |        Min |         Max |     Median
 ------------------------------------------------------------------------------
 1: CeramicWriteOnly
   1: setup               |       91354 |     60,000 |     169,593 |     60,001
   2: update_small_model  |       28776 |     10,542 |      66,907 |     27,790
   3: update_large_model  |       28976 |      3,261 |      66,917 |     27,963
 -------------------------+-------------+------------+-------------+-----------
 Aggregated               |       30968 |      3,261 |     169,593 |     28,021

 === PER REQUEST METRICS ===
 ------------------------------------------------------------------------------
 Name                     |        # reqs |        # fails |    req/s |  fail/s
 ------------------------------------------------------------------------------
 GET update_large_model   |        28,638 |         0 (0%) |     7.95 |    0.00
 GET update_small_model   |        29,063 |         0 (0%) |     8.07 |    0.00
 POST setup               |         4,169 |  1,354 (32.5%) |     1.16 |    0.38
 POST update_large_model  |        28,638 |     418 (1.5%) |     7.95 |    0.12
 POST update_small_model  |        29,063 |        99 (0%) |     8.07 |    0.03
 -------------------------+---------------+----------------+----------+--------
 Aggregated               |       119,571 |   1,871 (1.6%) |    33.21 |    0.52
 ------------------------------------------------------------------------------
 Name                     |    Avg (ms) |        Min |         Max |     Median
 ------------------------------------------------------------------------------
 GET update_large_model   |        1602 |         86 |      20,335 |      1,183
 GET update_small_model   |        1525 |        153 |      19,954 |      1,202
 POST setup               |       43825 |      9,355 |      60,021 |     47,856
 POST update_large_model  |       27373 |         48 |      60,008 |     26,571
 POST update_small_model  |       27250 |      7,160 |      60,007 |     26,311
 -------------------------+-------------+------------+-------------+-----------
 Aggregated               |       15462 |         48 |      60,021 |     18,486
 ------------------------------------------------------------------------------
 Slowest page load within specified percentile of requests (in ms):
 ------------------------------------------------------------------------------
 Name                     |    50% |    75% |    98% |    99% |  99.9% | 99.99%
 ------------------------------------------------------------------------------
 GET update_large_model   |  1,183 |  1,668 |  5,531 |  9,751 | 15,502 | 19,312
 GET update_small_model   |  1,202 |  1,680 |  4,857 |  5,683 | 15,363 | 18,517
 POST setup               | 47,856 | 60,000 | 60,001 | 60,001 | 60,005 | 60,021
 POST update_large_model  | 26,571 | 29,155 | 50,141 | 60,000 | 60,001 | 60,003
 POST update_small_model  | 26,311 | 28,762 | 51,863 | 57,649 | 60,001 | 60,003
 -------------------------+--------+--------+--------+--------+--------+-------
 Aggregated               | 18,486 | 26,819 | 56,028 | 60,000 | 60,001 | 60,004

 === ERRORS ===
 ------------------------------------------------------------------------------
 Count       | Error
 ------------------------------------------------------------------------------
 1,354         POST setup: error sending request setup: operation timed out
 418           POST update_large_model: error sending request update_large_model: operation timed out
 99            POST update_small_model: error sending request update_small_model: operation timed out
 ------------------------------------------------------------------------------
 ``````