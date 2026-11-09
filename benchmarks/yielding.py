#!/usr/bin/env python3
"""Linux channel-await benchmark. Emits measurements, not a Rust/Java comparison."""
import argparse
import json
import os
from pathlib import Path
import selectors
import subprocess
import tempfile
import time

# Legacy declarations intentionally exercise native method-frame lowering.
SOURCE = """
class App {
    async int work(Channel<int> ready, Channel<int> gate) {
        final sent = ready.send(1).await()
        final value = gate.receive().await()
        return value + 1
    }
    start() {
        final ready = Channel<int>(COUNT) final gate = Channel<int>(COUNT)
        final warm = this.work(ready, gate) final started = ready.receive().await()
        final sent = gate.send(1).await() final warmed = warm.await()
        print('warm') final begin = Console.readLine()
        final tasks = [] var index = 0
        for (index < COUNT) { tasks.add(this.work(ready, gate)) index = index + 1 }
        index = 0
        for (index < COUNT) { final started = ready.receive().await() index = index + 1 }
        print('parked') final release = Console.readLine()
        index = 0
        for (index < COUNT) { final sent = gate.send(1).await() index = index + 1 }
        final results = Async.all(tasks).await()
        var total = 0 index = 0
        for (index < COUNT) { total = total + results[index] index = index + 1 }
        print(total) final finished = Console.readLine()
    }
}
"""


def status(pid):
    fields = {}
    for line in Path(f"/proc/{pid}/status").read_text().splitlines():
        key, _, value = line.partition(":")
        if key in ("Threads", "VmRSS", "VmHWM"):
            fields[key] = int(value.split()[0])
    return fields


def line(process, selector, seconds):
    if not selector.select(seconds):
        raise TimeoutError("native workload did not reach its next checkpoint")
    value = process.stdout.readline().strip()
    if not value:
        raise RuntimeError("native workload stopped: " + process.stderr.read())
    return value


def measure(executable, count, workers, seconds):
    env = dict(os.environ, TOKIO_WORKER_THREADS=str(workers))
    with subprocess.Popen([str(executable)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                          stderr=subprocess.PIPE, text=True, bufsize=1, env=env) as process:
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)
        try:
            assert line(process, selector, seconds) == "warm"
            baseline = status(process.pid)
            start = time.perf_counter()
            process.stdin.write("\n")
            process.stdin.flush()
            assert line(process, selector, seconds) == "parked"
            parked_at = time.perf_counter()
            parked = status(process.pid)
            process.stdin.write("\n")
            process.stdin.flush()
            released_at = time.perf_counter()
            assert int(line(process, selector, seconds)) == count * 2
            complete_at = time.perf_counter()
            final = status(process.pid)
            process.stdin.write("\n")
            process.stdin.flush()
            code = process.wait(timeout=seconds)
            if code:
                raise RuntimeError(process.stderr.read())
            if parked["Threads"] > baseline["Threads"] + 2:
                raise RuntimeError("waiting tasks caused unexpected OS thread growth")
            return {
                "tasks": count, "workers": workers,
                "baseline_threads": baseline["Threads"], "parked_threads": parked["Threads"],
                "baseline_rss_kib": baseline["VmRSS"], "parked_rss_kib": parked["VmRSS"],
                "peak_rss_before_shutdown_kib": final["VmHWM"],
                "spawn_and_park_ms": round((parked_at - start) * 1000, 3),
                "release_and_complete_ms": round((complete_at - released_at) * 1000, 3),
                "batch_completions_per_second": round(count / (complete_at - released_at), 1),
            }
        finally:
            selector.close()
            if process.poll() is None:
                process.kill()
            process.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, default=Path("target/debug/ube-compiler"))
    parser.add_argument("--counts", type=int, nargs="+", default=[100, 1000, 5000])
    parser.add_argument("--repeat", type=int, default=3)
    parser.add_argument("--workers", type=int, default=2)
    parser.add_argument("--timeout", type=float, default=30)
    args = parser.parse_args()
    if min(args.counts + [args.repeat, args.workers]) <= 0:
        parser.error("counts, repeats, and workers must be positive")
    compiler = args.compiler.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="ube-yielding-benchmark-") as temp:
        directory = Path(temp)
        (directory / ".gitignore").write_text(".build/\n")
        source = directory / "App.ube"
        executable = directory / "workload"
        for count in args.counts:
            source.write_text(SOURCE.replace("COUNT", str(count)))
            built = subprocess.run([str(compiler), "build", str(source), "--release", "-o", str(executable)],
                                   capture_output=True, text=True, timeout=args.timeout)
            if built.returncode:
                raise RuntimeError(built.stderr)
            if "define internal i1 @__ube_poll_App_work" not in source.with_suffix(".ll").read_text():
                raise RuntimeError("compiler did not select the yielding backend")
            for repeat in range(args.repeat):
                result = measure(executable, count, args.workers, args.timeout)
                result.update(repeat=repeat + 1, compiler=str(compiler), generated_code_optimization="O3")
                print(json.dumps(result), flush=True)


if __name__ == "__main__":
    main()
