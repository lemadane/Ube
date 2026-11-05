//! Cross-process audit regressions. Known unresolved bugs have executable,
//! explicitly ignored tests rather than assertions that bless incorrect behavior.
#![cfg(unix)]

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
    fs::write(&source_path, source).unwrap();
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
        "status {:?}, stderr {}",
        output.status.code(),
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
#[ignore = "known issue: Money and decimal use f64; exact monetary arithmetic is not implemented"]
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
#[ignore = "known issue: generated Map lookup does not preserve missing-versus-zero presence"]
fn map_lookup_distinguishes_missing_entries_from_zero_through_the_real_cli() {
    expect_output(
        run_ube(
            r#"
        class Application {
            main() {
                final values = [1: 0, 2: 42]
                print(values[1] ?? -1)
                print(values[999] ?? -1)
                print(values[2] ?? -1)
            }
        }
    "#,
        ),
        "0\n-1\n42\n",
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
