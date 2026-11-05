//! Cross-process correctness and stability regressions for the compiler/runtime.
//! Exact-money and missing-map regressions are enabled after their fixes.
#![cfg(unix)]

mod support;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
};

fn runtime_directory() -> PathBuf {
    let directory = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    assert!(
        directory.join("libube_runtime.a").is_file(),
        "build the runtime static archive before running native tests"
    );
    directory
}

fn run_ube(source: &str) -> Output {
    let directory = tempfile::tempdir().unwrap();
    let source_path = directory.path().join("audit.ube");
    let source_path = support::write_source(&source_path, source);
    // The CLI searches beside its executable first. Keep this test's compiler
    // and current archive together so a stale installed/build runtime cannot
    // hide a regression or make a fixed operation appear broken.
    let cli = directory.path().join("ube-compiler");
    let compiler = env!("CARGO_BIN_EXE_ube-compiler");
    fs::hard_link(compiler, &cli)
        .or_else(|_| fs::copy(compiler, &cli).map(|_| ()))
        .unwrap();
    std::os::unix::fs::symlink(
        runtime_directory().join("libube_runtime.a"),
        directory.path().join("libube_runtime.a"),
    )
    .unwrap();
    // Use the real CLI, parser, checker, LLVM emitter, clang, and executable.
    Command::new("timeout")
        .arg("30s")
        .arg(cli)
        .arg("run")
        .arg(source_path)
        .current_dir(runtime_directory())
        .env("TMPDIR", directory.path())
        .output()
        .expect("run the Ube CLI")
}

fn expect_output(output: Output, expected: &str) {
    assert!(
        output.status.success(),
        "status {:?}, stdout {}, stderr {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), expected);
}

#[test]
fn currency_mismatch_recovers_through_the_real_cli() {
    expect_output(run_ube(r#"
        class Application {
            Money fail(bool subtract) {
                if (subtract) { return Money.php(10.0).subtract(Money.usd(5.0)) }
                return Money.php(10.0).add(Money.usd(5.0))
            }
            async Money failAsync() { return this.fail(true) }
            main() {
                try { print(this.fail(false).amount()) }
                catch (CurrencyMismatchError error) { print(error.message()) }
                finally { print('add cleanup') }
                try { print(this.fail(true).amount()) }
                catch (Error error) { print(error.message()) }
                finally { print('subtract cleanup') }
                try { final value = await this.failAsync() }
                catch (CurrencyMismatchError error) { print('async recovered') }
                print(Money.php(10.0).add(Money.php(5.0)).amount() == 15.0)
                print('still running')
            }
        }
    "#), "Money currency mismatch: PHP vs USD\nadd cleanup\nMoney currency mismatch: PHP vs USD\nsubtract cleanup\nasync recovered\ntrue\nstill running\n");
}

#[test]
fn list_lookup_distinguishes_zero_from_invalid_indices_through_the_real_cli() {
    expect_output(
        run_ube(
            r#"
        class Application {
            main() {
                final items = [0, 42]
                print(items[0])
                try { print(items[-1]) }
                catch (Error error) { print('negative index absent') }
                try { print(items[2]) }
                catch (Error error) { print('past end absent') }
                print(items[1])
            }
        }
    "#,
        ),
        "0\nnegative index absent\npast end absent\n42\n",
    );
}

#[test]
fn money_decimal_arithmetic_is_exact_through_the_real_cli() {
    expect_output(
        run_ube(
            r#"
        class Application {
            main() {
                final sum = Money.php(0.1).add(Money.php(0.2))
                print(sum.amount() == 0.3)
                print(Money.php(0.3).subtract(Money.php(0.2)).amount() == 0.1)
                print(sum.subtract(Money.php(0.3)).amount() == 0.0)
            }
        }
    "#,
        ),
        "true\ntrue\ntrue\n",
    );
}

#[test]
fn map_lookup_distinguishes_missing_entries_from_zero_through_the_real_cli() {
    expect_output(
        run_ube(
            r#"
        class Application {
            main() {
                final values = [1: 0, 2: 42]
                print(values[1] ?? -1)
                print(values[1] != null)
                print(values[999] == null)
                print(values[999] ?? -1)
                print(values[2] ?? -1)
            }
        }
    "#,
        ),
        "0\ntrue\ntrue\n-1\n42\n",
    );
}

#[test]
fn array_allocation_failure_is_safe_through_the_exported_c_abi() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("array_audit.c");
    let binary = directory.path().join("array_audit");
    // Array creation has no public Ube-language binding. A compiled C consumer
    // exercises the actual exported ABI and its by-value struct layout.
    fs::write(
        &source,
        r#"
        #include <stddef.h>
        #include <stdint.h>
        #include <stdio.h>
        typedef struct {
            unsigned char *data;
            size_t len, capacity, element_size;
        } UbeArray;
        extern UbeArray ube_array_create(size_t capacity, size_t element_size);
        extern void ube_free(void *ptr, size_t size, size_t align);
        int main(void) {
            const size_t sizes[] = {1, 2};
            for (size_t i = 0; i < 2; ++i) {
                /* Invalid layout and multiplication overflow: no OS OOM needed. */
                UbeArray failed = ube_array_create(SIZE_MAX, sizes[i]);
                if (failed.data != NULL || failed.len != 0 ||
                    failed.capacity != 0 || failed.element_size != sizes[i]) return 10;
            }
            for (size_t i = 0; i < 100; ++i) {
                UbeArray valid = ube_array_create(4, sizeof(uint64_t));
                if (!valid.data || valid.len != 0 || valid.capacity != 4 ||
                    valid.element_size != sizeof(uint64_t)) return 11;
                uint64_t *items = (uint64_t *)valid.data;
                items[0] = 0;
                items[3] = 42;
                if (items[0] != 0 || items[3] != 42) return 12;
                ube_free(valid.data, 4 * sizeof(uint64_t), 8);
            }
            puts("array allocation recovered");
            return 0;
        }
    "#,
    )
    .unwrap();
    let compiled = Command::new("clang")
        .arg("-std=c11")
        .arg("-Wall")
        .arg("-Wextra")
        .arg("-Werror")
        .arg(&source)
        .arg(runtime_directory().join("libube_runtime.a"))
        .args(["-ldl", "-lpthread", "-lm"])
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    expect_output(
        Command::new("timeout")
            .arg("15s")
            .arg(binary)
            .output()
            .unwrap(),
        "array allocation recovered\n",
    );
}

#[test]
fn trait_defaults_and_mixed_concrete_arguments_execute_through_the_real_cli() {
    expect_output(
        run_ube(
            r#"
        trait Named {
            string name()
            string title() { return 'title' }
        }
        class First { string name() { return 'first' } }
        class Second { string name() { return 'second' } }
        class Unrelated { int value() { return 42 } }
        class Application {
            string render(Named first, Named second) { return '${first.name()} ${second.name()}' }
            string describe(Named value) { return '${value.title()} ${value.name()}' }
            main() {
                print(this.render(First(), Second()))
                print(this.describe(First()))
                print(this.describe(Second()))
            }
        }
    "#,
        ),
        "first second\ntitle first\ntitle second\n",
    );
}

#[test]
fn exact_money_text_api_preserves_large_values_and_rejects_rounding() {
    expect_output(run_ube(r#"
        class Application {
            main() {
                final large = Money.parse('9007199254740993.01', 'PHP')
                print(large.add(Money.parse('0.02', 'PHP')).amountText())
                print(large.amountText())
                print(Money.parse('100.00', 'PHP').multiplyPercent('12.5').amountText())
                final converted = Money.parse('3.5', 'PHP').convertExact('USD', '0.1')
                print(converted.amountText())
                print(converted.currency().code())
                try { Money.parse('79228162514264337593543950335', 'PHP').add(Money.parse('1', 'PHP')) }
                catch (MoneyError error) { print('overflow rejected') }
                try { Money.parse('0.0000000000000000000000000001', 'PHP').multiplyPercent('1') }
                catch (MoneyError error) { print('rounding rejected') }
                try { Money.parse('1e2', 'PHP') }
                catch (MoneyError error) { print('invalid text rejected') }
                try { Money.parse('1', 'php') }
                catch (MoneyError error) { print('invalid currency rejected') }
                print(Money.parse('-0.10', 'PHP').add(Money.parse('0.10', 'PHP')).amountText())
                print('alive')
            }
        }
    "#), "9007199254740993.03\n9007199254740993.01\n12.5\n0.35\nUSD\noverflow rejected\nrounding rejected\ninvalid text rejected\ninvalid currency rejected\n0\nalive\n");
}

#[test]
fn tracing_heap_preserves_roots_collects_cycles_and_handles_concurrent_mutators() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("gc_audit.c");
    let binary = directory.path().join("gc_audit");
    fs::write(&source, r#"
        #include <stddef.h>
        #include <stdint.h>
        #include <stdio.h>
        #include <pthread.h>
        typedef struct { void *object; uint64_t root; } Allocation;
        typedef struct { size_t objects, bytes, roots, collections, reclaimed; } Stats;
        extern Allocation ube_gc_allocate(size_t, size_t, const size_t *, size_t);
        extern uint64_t ube_gc_root_add(void *);
        extern _Bool ube_gc_root_update(uint64_t, void *);
        extern _Bool ube_gc_root_remove(uint64_t);
        extern _Bool ube_gc_store_pointer(void *, size_t, void *);
        extern void ube_gc_collect(void);
        extern Stats ube_gc_stats(void);
        extern void ube_free(void *, size_t, size_t);
        static void *worker(void *argument) {
            for (size_t i = 0; i < 256; ++i) {
                Allocation value = ube_gc_allocate(sizeof(uintptr_t), _Alignof(uintptr_t), NULL, 0);
                if (!value.object || !value.root) return (void *)1;
                *(uintptr_t *)value.object = (uintptr_t)argument;
                ube_gc_collect();
                if (*(uintptr_t *)value.object != (uintptr_t)argument) return (void *)2;
                if (!ube_gc_root_remove(value.root)) return (void *)3;
                ube_gc_collect();
            }
            return NULL;
        }
        int main(void) {
            size_t edge = 0;
            size_t invalid = SIZE_MAX;
            Allocation bad = ube_gc_allocate(8, 8, &invalid, 1);
            if (bad.object || bad.root) return 10;
            bad = ube_gc_allocate(8, 8, NULL, 1);
            if (bad.object || bad.root) return 11;
            for (size_t i = 0; i < 1000; ++i) {
                Allocation parent = ube_gc_allocate(2 * sizeof(uintptr_t), _Alignof(uintptr_t), &edge, 1);
                Allocation child = ube_gc_allocate(2 * sizeof(uintptr_t), _Alignof(uintptr_t), &edge, 1);
                if (!parent.object || !child.object || !parent.root || !child.root) return 12;
                if (!ube_gc_store_pointer(parent.object, 0, child.object) ||
                    !ube_gc_store_pointer(child.object, 0, parent.object)) return 13;
                if (ube_gc_store_pointer(parent.object, sizeof(uintptr_t), child.object)) return 14;
                if (ube_gc_store_pointer(parent.object, 0, (void *)16)) return 15;
                if (!ube_gc_root_remove(child.root)) return 16;
                /* An ordinary allocator free cannot invalidate a managed node. */
                ube_free(parent.object, 2 * sizeof(uintptr_t), _Alignof(uintptr_t));
                ube_gc_collect();
                Stats live = ube_gc_stats();
                if (live.objects != 2 || live.roots != 1) return 17;
                uint64_t alias = ube_gc_root_add(child.object);
                if (!alias || !ube_gc_root_update(alias, NULL)) return 18;
                if (!ube_gc_root_remove(parent.root) || !ube_gc_root_remove(alias)) return 19;
                ube_gc_collect();
                Stats dead = ube_gc_stats();
                if (dead.objects || dead.bytes || dead.roots) return 20;
            }
            if (ube_gc_root_add((void *)16) || ube_gc_root_update(0, NULL) || ube_gc_root_remove(0)) return 21;
            pthread_t threads[4];
            for (size_t i = 0; i < 4; ++i)
                if (pthread_create(&threads[i], NULL, worker, (void *)(uintptr_t)(i + 1))) return 22;
            for (size_t i = 0; i < 4; ++i) {
                void *result = NULL;
                if (pthread_join(threads[i], &result) || result) return 23;
            }
            ube_gc_collect();
            Stats final = ube_gc_stats();
            if (final.objects || final.bytes || final.roots || final.reclaimed != 3024) return 24;
            puts("tracing heap stable: 3024 objects reclaimed");
            return 0;
        }
    "#).unwrap();
    let compiled = Command::new("clang")
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror"])
        .arg(&source)
        .arg(runtime_directory().join("libube_runtime.a"))
        .args(["-ldl", "-lpthread", "-lm"])
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        compiled.status.success(),
        "{}",
        String::from_utf8_lossy(&compiled.stderr)
    );
    expect_output(
        Command::new("timeout")
            .arg("20s")
            .arg(binary)
            .output()
            .unwrap(),
        "tracing heap stable: 3024 objects reclaimed\n",
    );
}

#[test]
fn nullable_channels_cancellation_and_cleanup_survive_repeated_native_execution() {
    expect_output(
        run_ube(
            r#"
        class Application {
            int? number(int iteration) { if (iteration % 2 == 0) { return 0 } return null }
            bool? flag(int iteration) { if (iteration % 2 == 0) { return false } return null }
            async int? wait(Channel<int?> blocked, Channel<int> ready, Channel<int> completed) {
                try {
                    final signal = await ready.send(1)
                    return await blocked.receive()
                }
                finally { final done = await completed.send(1) }
            }
            main() {
                var iteration = 0
                var closed = 0
                var cancelled = 0
                for (iteration < 32) {
                    final numbers = Channel<int?>(1)
                    final flags = Channel<bool?>(1)
                    final number = this.number(iteration)
                    final flag = this.flag(iteration)
                    final receivedNumber = numbers.receive()
                    final receivedFlag = flags.receive()
                    final sentNumber = await numbers.send(number)
                    final sentFlag = await flags.send(flag)
                    if ((await receivedNumber.timeout(2000)) != number) { throw 'nullable number corrupted' }
                    if ((await receivedFlag.timeout(2000)) != flag) { throw 'nullable flag corrupted' }
                    numbers.close()
                    flags.close()
                    try { final value = await numbers.receive() }
                    catch (Error error) { closed = closed + 1 }
                    final blocked = Channel<int?>(1)
                    final ready = Channel<int>(1)
                    final completed = Channel<int>(1)
                    final waiting = this.wait(blocked, ready, completed)
                    final started = await ready.receive().timeout(2000)
                    final requested = waiting.cancel()
                    try { final value = await waiting }
                    catch (Error error) { cancelled = cancelled + 1 }
                    final cleaned = await completed.receive().timeout(2000)
                    if (cleaned != 1) { throw 'missing cancellation cleanup' }
                    blocked.close()
                    ready.close()
                    completed.close()
                    iteration = iteration + 1
                }
                if (not Gc.collect()) { throw 'cancelled task leaked a root frame' }
                print('${iteration} ${closed} ${cancelled}')
            }
        }
    "#,
        ),
        "32 32 32\n",
    );
}

#[test]
fn generated_objects_keep_live_graphs_and_reclaim_unreachable_cycles() {
    expect_output(
        run_ube(
            r#"
        class Node(var Node? next = null, int value = 0) {}
        class Mixed(bool active, int count, Node child, bool flag) {}
        class Application {
            Node make(int value) {
                try { return Node(null, value) }
                finally { Gc.collect() }
            }
            void cycle(int value) {
                final left = Node(null, value)
                final right = Node(left, value + 1)
                left.next(right)
                if (not Gc.collect()) { throw 'serial collection deferred' }
                if ((left.next() ?? left).value() != value + 1) { throw 'live child lost' }
                if ((right.next() ?? right).value() != value) { throw 'live parent lost' }
            }
            main() {
                var kept = this.make(-1)
                final mixed = Mixed(true, 7, Node(null, 42), false)
                Gc.collect()
                if (not mixed.active() or mixed.count() != 7 or mixed.flag() or mixed.child().value() != 42) { throw 'mixed object layout corrupted' }
                final baseline = Gc.objects()
                var iteration = 0
                for (iteration < 1000) {
                    this.cycle(iteration)
                    kept = this.make(iteration)
                    if (kept.value() != iteration) { throw 'return root lost' }
                    iteration = iteration + 1
                }
                if (not Gc.collect()) { throw 'final collection deferred' }
                if (Gc.objects() > baseline + 16) { throw 'generated heap grew without bound' }
                if (Gc.reclaimed() < 2000) { throw 'cycles not reclaimed' }
                print(kept.value())
                print('generated heap stable')
            }
        }
    "#,
        ),
        "999\ngenerated heap stable\n",
    );
}

#[test]
fn generated_tracing_heap_survives_parallel_collection_and_async_object_handoffs() {
    expect_output(
        run_ube(
            r#"
        class Node(var Node? next = null, int value = 0) {}
        class Application {
            async int work(int seed) {
                var iteration = 0
                for (iteration < 200) {
                    final left = Node(null, seed + iteration)
                    final right = Node(left, seed + iteration + 1)
                    left.next(right)
                    Gc.collect()
                    if ((left.next() ?? left).value() != seed + iteration + 1) { throw 'parallel child lost' }
                    if ((right.next() ?? right).value() != seed + iteration) { throw 'parallel parent lost' }
                    iteration = iteration + 1
                }
                return seed
            }
            async Node echo(Node value) { Gc.collect() return value }
            main() {
                final one = this.work(1000)
                final two = this.work(2000)
                final three = this.work(3000)
                final four = this.work(4000)
                final outcomes = await Task.all([one, two, three, four])
                print(outcomes[0])
                print(outcomes[1])
                print(outcomes[2])
                print(outcomes[3])
                final returned = await this.echo(Node(null, 42))
                final channel = Channel<Node>(1)
                final sent = await channel.send(returned)
                final received = await channel.receive()
                channel.close()
                Gc.collect()
                print(received.value())
                if (Gc.reclaimed() < 1000) { throw 'parallel cycles not reclaimed' }
                print('parallel tracing stable')
            }
        }
    "#,
        ),
        "1000\n2000\n3000\n4000\n42\nparallel tracing stable\n",
    );
}

#[test]
fn loop_scoped_locals_reuse_stack_storage_through_the_real_cli() {
    expect_output(
        run_ube(
            r#"
        class Application {
            main() {
                var iteration = 0
                var total = 0L
                for (iteration < 1000000) {
                    final next = iteration + 1
                    total = total + 1L
                    iteration = next
                }
                print(total)
            }
        }
    "#,
        ),
        "1000000\n",
    );
}

#[test]
fn generated_temporary_objects_are_collected_automatically() {
    expect_output(
        run_ube(
            r#"
        class Entry(int value) {}
        class Application {
            int read(int value) { final entry = Entry(value) return entry.value() }
            main() {
                var iteration = 0
                for (iteration < 5000) {
                    if (this.read(iteration) != iteration) { throw 'live temporary lost' }
                    iteration = iteration + 1
                }
                if (Gc.objects() > 300) { throw 'automatic collection did not bound temporaries' }
                if (Gc.reclaimed() < 4500) { throw 'automatic collection did not run' }
                print('automatic collection stable')
            }
        }
    "#,
        ),
        "automatic collection stable\n",
    );
}
