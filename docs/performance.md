# Performance

## Target

- 200 payment creations per second (create plus start a lightning attempt), 64 concurrent clients.
- 50 concurrent settlements, at least 50 settled payments per second.

## How to measure

```
docker run -d --name pgw-test-db -e MARIADB_ROOT_PASSWORD=test -p 127.0.0.1:33907:3306 mariadb:11.4
TEST_DATABASE_URL=mysql://root:test@127.0.0.1:33907/ scripts/load.sh
```

`scripts/load.sh` runs `tests/load.rs` in release mode against the mock provider and a real MariaDB, with rate limits lifted. It prints `LOAD` lines and fails when a floor is missed. Tune with `LOAD_PAYMENTS`, `LOAD_CREATE_CONCURRENCY`, `LOAD_SETTLE_CONCURRENCY`, `LOAD_MIN_CREATIONS_PER_SEC` and `LOAD_MIN_SETTLEMENTS_PER_SEC`. After the run it checks every payment is paid and the integrity check is clean.

## Results

Developer machine, MariaDB 11.4 in docker on the same host, 2000 payments, in process (no HTTP hop):

| Measure | Result |
| --- | --- |
| Creations per second | 372 |
| Create latency p50 / p95 / p99 | 173 / 267 / 332 ms |
| Settlements per second (50 concurrent) | 599 |

The target is met. No deadlocks or lock wait timeouts surfaced. Settlement deadlocks are retried (`service::retry`), and the chaos suite covers the race between settlement and expiry.
