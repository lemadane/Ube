# Yielding channel-await benchmark

This Linux benchmark measures the first native yielding slice, not a comparison
against Rust or Java. Each method signals that it started, suspends on a channel,
and performs an addition after resuming. A warmed runtime establishes the OS
thread baseline. The harness samples `/proc` while all methods are waiting and
again after completion, verifies the sum, and fails on unexpected thread growth.

```sh
cargo build -p ube-compiler --offline
python3 benchmarks/yielding.py --counts 100 1000 5000 --repeat 3
```

The generated workload is compiled with `--release` (LLVM O3). The Rust runtime
profile follows the supplied compiler's paired archive. To measure an optimized
Rust runtime too:

```sh
cargo build --release -p ube-compiler --offline
python3 benchmarks/yielding.py --compiler target/release/ube-compiler --counts 100 1000 5000 --repeat 3
```

`--workers` defaults to two and controls `TOKIO_WORKER_THREADS` for each workload.
JSON lines report baseline/parked OS threads, baseline/parked RSS, the sampled
memory high-water mark before shutdown, spawn-to-park time, and batch completion
time/rate after release. Compilation and warm-up are excluded. Batch completion
includes the synchronous producer, scheduler, channel sends, and result joining;
it is not a per-task latency percentile or an isolated scheduler throughput test.
Each checkpoint and process exit has a timeout; failing workloads are killed.

## Development measurement, 2026-11-06

Linux x86_64; two Tokio workers; debug/unoptimized Rust runtime; LLVM O3 for the
generated workload. Two repetitions per task count. Values below are means of
the [recorded samples](results/yielding-dev-linux.json).

| Suspended methods | Warm OS threads | Parked OS threads | Parked RSS (MiB) | Release-to-completion batch time (ms) |
|---|---|---|---|---|
| 100 | 3 | 3 | 19.2 | 6.6 |
| 1,000 | 3 | 3 | 26.1 | 66.9 |
| 5,000 | 3 | 3 | 55.8 | 313.6 |

These results demonstrate bounded worker usage for this workload. They do not
establish production capacity, tail latency, long-running memory stability, or
Rust/Java performance parity. Blocking-pool fallback methods are outside this
measurement. Shutdown-lived task handles and conservative escape pins still
contribute to memory usage.
