# Persistent objects as JSON

This example stores a concrete Ube object using its default `.toJson()` method, exits, then reads the actual JSON from a separate application process. The store has no cache eviction or expiration.

From the repository root:

```bash
cargo build -p ube-compiler -p ube-runtime
cd examples/key-value-store
UBE_BIN=../../target/debug/ube-compiler
mkdir -p data
"$UBE_BIN" run -- data/records.db write
"$UBE_BIN" run -- data/records.db read
"$UBE_BIN" run -- data/records.db delete
"$UBE_BIN" run -- data/records.db read
```

A matching installed toolchain can use `UBE_BIN=ube`. Expected application output in order:

```text
saved
42
Alice
東京
true
missing
```

Sources:

- [App.ube](src/App.ube): store/read/delete and cleanup
- [Profile.ube](src/Profile.ube): concrete data fields
- [Address.ube](src/Address.ube): nested data object

The default serializer emits `id`, `name`, and `address` automatically. Methods and runtime identity are not stored. Reading returns JSON text, parsed here into a JSON document with typed getters; it does not recreate a `Profile` instance automatically.

`data/` is ignored by Git and remains outside `.build`, so `ube clean` does not remove the records. Create its directory before opening the store. Do not open the same file with `Cache.open`; cache files and key-value-store files have different identities and policies.

See [KeyValueStore](../../docs/key-value-store.md) and [JSON](../../docs/json.md) for durability, supported data types, limits, failure behavior, and stress-test commands.

The store validates JSON text; automatic `.toXml()` does not change its format. See the [XML example](../xml/README.md) for object/list/map XML serialization and document traversal, and the [XML guide](../../docs/xml.md) for file round-trip coverage.
