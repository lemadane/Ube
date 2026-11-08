# KeyValueStore

`KeyValueStore` is an embedded persistent JSON key-value store owned by one application process. It is separate from `Cache`: it reads records from disk as needed and has **no TTL, automatic eviction, or entry-count budget**. Records remain until an explicit overwrite or delete, subject to the selected durability mode and functioning storage.

## Object storage

Each source file must match its class name exactly. For example:

```ube
// Address.ube
class Address(string city)
```

```ube
// Profile.ube
import Address
class Profile(int id, string name, Address address)
```

```ube
// App.ube
import Address
import Profile

class App {
    start() {
        final store = KeyValueStore.open('data/records.db')
        try {
            final profile = Profile(42, 'Alice', Address('東京'))
            store.set('user:42', profile.toJson())

            final saved = Json.toObject(store.get('user:42') ?? '{}')
            Console.printLine(saved.getInt('id'))
            Console.printLine(saved.getString('name'))
            Console.printLine(saved.getObject('address').getString('city'))
        } finally {
            store.close()
        }
    }
}
```

Create `data/` first. The [runnable example](../examples/key-value-store/README.md) supplies these files, a manifest, separate write/read processes, commands, and expected output. `toJson` is automatic for concrete data classes; see [JSON](json.md) for unsupported values and serialization rules.

`set` accepts **JSON text**, not an arbitrary object handle. Calling `.toJson()` explicitly makes that conversion visible. Reading returns JSON text, not a reconstructed class instance. Automatic typed reconstruction, schema migration, and arbitrary object-graph persistence are not implemented.

[XML serialization](xml.md) does not change this JSON-only storage contract: raw `.toXml()` output is not a valid store value. For an XML file, use `File.write` and `File.read`, then `Xml.toObject`; the native XML suite tests that path across independent processes.

## API

`KeyValueStore` is built in and needs no import. It cannot be directly constructed with `KeyValueStore()`.

| Method | Result | Behavior |
| --- | --- | --- |
| `KeyValueStore.open(string path)` | `KeyValueStore` | Open a dedicated database with durable writes |
| `KeyValueStore.openBuffered(string path)` | `KeyValueStore` | Open with buffered synchronization |
| `store.get(string key)` | `string?` | Return stored JSON text, or null if missing |
| `store.set(string key, string json)` | `void` | Validate JSON and atomically insert or overwrite |
| `store.add(string key, string json)` | `bool` | Validate JSON and insert only if absent; one concurrent caller wins |
| `store.has(string key)` | `bool` | Check whether a record exists |
| `store.delete(string key)` | `bool` | Remove a record; false if absent |
| `store.increment(string key, long delta)` | `long` | Atomically increment a signed JSON integer, preserving other records |
| `store.close()` | `void` | Release the connection and exclusive ownership; safe to repeat |

Keys are nonempty UTF-8 strings of at most 4,096 bytes. Values are valid JSON text of at most 16 MiB. Objects, arrays, strings, numbers, booleans, and JSON null are accepted. Ube string arguments cannot contain embedded NUL; JSON escapes such as `\u0000` can remain encoded in stored JSON text. The stored text is preserved rather than reformatted on write.

A stored JSON null is the non-null Ube string `'null'`; a missing record returns Ube null. Those are distinct:

```ube
// Inside an application method:
store.set('present-null', 'null')
Console.printLine(store.has('present-null')) // true
Console.printLine(store.get('absent') ?? 'missing') // missing
```

`increment` requires an existing signed 64-bit JSON integer, such as the JSON text `'0'`. A JSON string containing a number, such as `'"0"'`, is not an integer. Missing keys, decimals, non-integers, and overflow raise errors. Negative deltas are supported. Numeric method parameters accept `int` through widening to `long`.

Calls are synchronous and can raise catchable `Error`s for invalid JSON, arguments, missing directories, conflicting ownership, incompatible files, storage failures, or closed handles. A missing key is a normal result. Storage errors must not be treated as a missing record. Runtime cleanup closes remaining stores after joining application tasks.

## Concurrency and ownership

Tasks within the owning process can share one store handle. A mutex serializes its connection, and each mutation is an atomic SQLite transaction. `add` and `increment` are atomic; a separate `get` and `set` sequence is not. There is no public multi-operation transaction API in this first version.

One owning handle can open a database at a time. A second concurrent open is rejected by SQLite's exclusive lock, including from another process. To transfer ownership, close the first handle before reopening. There is no standalone KeyValueStore server, network client, replication, or distributed consistency protocol.

The store does not load every record into a managed in-memory map. SQLite has a page-cache target of approximately 2 MiB; individual values, query results, serialization buffers, and Ube runtime allocations consume additional memory. This is not a hard process RSS cap. Cache metadata, eviction, and TTL logic are not used.

## Persistence

The database has a KeyValueStore-specific application ID and format version 1. Cache databases and unrelated SQLite files are rejected. Parent directories must already exist. `:memory:` is rejected because this API requires a disk path.

Both modes use SQLite WAL with automatic checkpoints after approximately 256 WAL pages:

| Mode | Synchronization | Guarantee boundary |
| --- | --- | --- |
| `open` | `FULL` | Mutation commits and synchronizes before returning success, relying on the storage device/filesystem honoring synchronization |
| `openBuffered` | `NORMAL` | Atomic transactions, with possible loss of recent successful writes after machine/power failure |

Clean close requests a truncating checkpoint. A forced process kill does not require clean close for WAL recovery. Expiration and shrinking a cache budget cannot delete these records: there is no cache budget or TTL here. Running out of storage causes an error rather than an eviction policy.

Keep the database and any existing `-wal`/`-shm` companion files together. Stop the owner before a filesystem backup and copy the whole set. There is no live backup or restore CLI for this API yet. Store data outside `.build` to survive `ube clean`, and ignore local generated data in Git:

```gitignore
.build/
data/
```

The files are not encrypted by KeyValueStore. App-level schema/version handling and any encryption requirements remain explicit application concerns.

## End-to-end stress evidence

The native tests compile Ube through the real CLI, emit LLVM, link the current runtime, and execute separate processes. They cover:

- **5,000 object records**, fully reread and checked after reopening, including nested objects, Unicode/escaping, booleans, decimals, nullable values, and 64-bit integers.
- **Four concurrent tasks**, writing 1,000 additional records and performing 1,000 atomic increments, with exactly one conditional-insert winner; records and counters are verified again after reopening.
- **Three SIGKILL recovery runs**, each killed during a 5,000-record writer after 1,000 acknowledged records; every acknowledged record, prior deletion, and overwrite must recover in a new process.
- Explicit garbage collection during write stress to exercise serialization roots.
- An injected SQLite write failure that must raise an error and leave the previous record unchanged, including after reopen.
- Invalid JSON, closed/stale handles, exclusive ownership, buffered reopen, cache/store format separation, and serialization failure cases.

Run from the repository root:

```bash
cargo build -p ube-runtime
cargo test -p ube-compiler --test key_value_store_e2e -- --nocapture
```

These tests exercise process-crash recovery and concurrency correctness. They do not establish physical power-loss behavior, long-running production stability, throughput targets, or distributed availability. Buffered host-failure loss is explicitly allowed. The test fixture and assertions are in [key_value_store_e2e.rs](../compiler/tests/key_value_store_e2e.rs).
