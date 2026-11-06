# JSON and default `toJson()`

Ube's concrete data objects have an automatic `toJson()` method. It returns JSON text describing their declared data fields. You do not need to write a serializer method or import a library.

```ube
// Address.ube
class Address(string city) {}
```

```ube
// Profile.ube
import Address
class Profile(int id, string name, Address address) {}
```

```ube
// Main.ube
import Address
import Profile

class Main {
    main() {
        final profile = Profile(42, 'Alice', Address('東京'))
        Console.printLine(profile.toJson())

        final document = Json.toObject(profile.toJson())
        Console.printLine(document.getInt('id'))
        Console.printLine(document.getObject('address').getString('city'))
    }
}
```

The JSON object contains `id`, `name`, and nested `address` fields. Object-key order is not an application contract. `toJson()` requires a value instance, not a type name. See the [runnable persistence example](../examples/key-value-store/README.md).

## What is serialized

| Value | Representation |
| --- | --- |
| `int`, `long`, `byte` | JSON integer, with 64-bit integer digits preserved by Ube |
| Finite `decimal` | JSON number |
| `bool` | JSON boolean |
| `string` | Escaped JSON string, preserving Unicode |
| Nullable scalar with no value | JSON null, distinct from zero and false |
| Null data-object/string reference | JSON null |
| Concrete data-class instance | JSON object of its declared constructor fields, recursively |
| `List` | JSON array, recursively preserving each inserted value’s type |
| `Map` | JSON object, recursively preserving each entry’s value type |
| Parsed `JsonObject` / `JsonArray` | Its actual JSON value, including nested arrays and objects |

Data fields are included under their declared names, including private fields. There is no automatic secret redaction. Methods, computed getter calls, native identity, and executable code are not stored. Repeated references to the same child are serialized as repeated data, not reference IDs. Reading JSON does not preserve object identity.

The compiler supplies field layout/type metadata. Serialization reads scalar fields and references through runtime synchronization and keeps referenced objects rooted while traversing them. It is not an atomic snapshot across a mutable object graph: coordinate application mutations when you need several fields to represent the same logical instant.

`toJson` is reserved for the default encoder; custom methods or constructor fields with that name are rejected. There is no user-defined serialization hook in this version.

## Unsupported values fail clearly

JSON cannot faithfully represent every runtime object. Ube does not silently drop unsupported fields or stringify a resource pointer as meaningful data.

- Cyclic object references raise `JsonError`.
- Non-finite decimals, such as NaN or infinity, raise `JsonError`.
- Runtime resources such as `Cache`, `KeyValueStore`, channels, tasks, database connections, and file handles raise `JsonError`, including when encountered in a data field.
- Lists and maps carry element metadata and support nested collections and data objects. Collections containing runtime resources or native values without serialization metadata raise `JsonError`. Cyclic collections raise `JsonError`. Nullable scalar collection elements remain unsupported by the compiler.
- Anonymous object and function layouts are rejected by the compiler for automatic JSON conversion. Use a declared data class or a parsed JSON document.
- Unsupported or trait-erased field types cannot be reconstructed from their raw runtime pointers and raise an error.

```ube
final values = [1, 'two', true, [3, 4]]
Console.printLine(values.toJson())
// [1,"two",true,[3,4]]
```

Maps serialize as JSON objects. String keys remain unchanged; integer, finite decimal, and boolean keys convert to strings. Null, object, collection, and resource keys raise `JsonError`. Keys that collide after conversion (such as `1` and `'1'`) also raise `JsonError` rather than overwrite an entry. JSON object-key order is not guaranteed.

```ube
final profile = ['name': 'Alice', 'scores': [95, 88]]
Console.printLine(profile.toJson())
// {"name":"Alice","scores":[95,88]}

Console.printLine([1000L: 95].toJson())
// {"1000":95}
```

Collections retain references to inserted objects until runtime cleanup so they remain valid during garbage collection.

For example, inside a method:

```ube
final cache = Cache.memory(4096)
try {
    cache.toJson()
} catch (JsonError err) {
    Console.printLine('A live cache handle is not JSON data')
} finally {
    cache.close()
}
```

The automatic encoder limits nesting depth to 64, traversal to 100,000 nodes, and encoded output to 16 MiB. Oversized or excessively nested values raise errors. Shared children count each time they are encoded. Temporary allocations and JSON document storage are not a hard RSS budget.

## Parsing and typed getters

`Json.toObject(string text)` parses actual JSON; it does not return placeholder data. Invalid syntax raises `JsonParseError`. Its result uses the `JsonObject` document type even when the JSON root is an array or scalar; object getters require an object root. Use `toJson()` to encode the document's root value again.

| Object method | Result | Requirement |
| --- | --- | --- |
| `getString(string key)` | `string` | Field is a JSON string |
| `getInt(string key)` | `int` | Field is an integer within signed 32-bit range |
| `getLong(string key)` | `long` | Field is an integer within signed 64-bit range |
| `getDecimal(string key)` | `decimal` | Field is a JSON number; conversion uses floating-point precision |
| `getBool(string key)` | `bool` | Field is a JSON boolean |
| `getObject(string key)` | `JsonObject` | Field is a JSON object |
| `getArray(string key)` | `JsonArray` | Field is a JSON array |

Missing fields, incompatible types, and invalid handles raise `JsonError`; getters do not invent zero, false, or placeholder strings. A null field does not satisfy a non-null typed getter. Ube native strings cannot represent embedded NUL, so getting a JSON string that decodes to such a value raises an error rather than returning truncated text. The encoded JSON can still preserve its `\u0000` escape.

| Array method | Result | Requirement |
| --- | --- | --- |
| `len()` | `int` | Number of array entries |
| `get(int index)` | `JsonObject` | Indexed element is an object |
| `getString(int index)` | `string` | Indexed element is a string |

Array indexes are zero-based. Negative/past-end indexes and wrong element types raise `JsonError`. There are no general primitive-root getters or JSON array mutation methods in this first API.

`Json.partialParse(text, JsonObject)` and `Json.to(text, JsonObject)` are compatibility aliases for parsing a JSON document. Passing an application class as the target is rejected: automatic typed object reconstruction is not implemented. Create your class explicitly from typed getters when needed.

Parsing accepts up to 16 MiB of JSON text and uses the JSON parser's own nesting checks; automatic encoding additionally enforces the traversal limits above. Documents and child document handles remain owned by the runtime until cleanup. There is no per-document `close`/dispose API yet, so long-lived workloads repeatedly parsing documents should account for that retained memory. The current stress tests are bounded correctness checks, not long-running memory-stability evidence.

## Persisting objects

```ube
// Inside a method with a Profile instance:
final store = KeyValueStore.open('data/records.db')
try {
    store.set('profile', profile.toJson())
    final document = Json.toObject(store.get('profile') ?? '{}')
    Console.printLine(document.getString('name'))
} finally {
    store.close()
}
```

Serialization happens before the store mutation. If it fails, the old stored record is not replaced. `KeyValueStore.set` validates JSON before writing. Read values remain JSON text; there is no implicit conversion back to `Profile`. See [KeyValueStore](key-value-store.md) for synchronous durability, atomic operations, and crash-recovery evidence.

## JavaScript comparison

JavaScript's `JSON.stringify` does not faithfully convert every value: cyclic references and ordinary BigInt serialization can throw, some unsupported values are omitted, and non-finite numbers become null. Ube's automatic encoder rejects unsupported data instead of silently applying those lossy conversions. [JavaScript reference](https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/JSON/stringify)

Ube can preserve a signed 64-bit integer's decimal digits in JSON and read them with `getLong`. That does not make another language's numeric parser lossless: choose an interoperable string representation for large identifiers when a consumer uses limited-precision numbers. JSON also does not retain Ube class names, methods, pointer identity, or schema versions. Applications must define schema migration and source-data validation themselves.

## Native validation

[The end-to-end tests](../compiler/tests/key_value_store_e2e.rs) verify real stored fields, nullable zero/false versus null, all scalar field layouts, Unicode/escaping, nested/shared references, cycles, non-finite decimals, unsupported resource fields, malformed JSON, parsing arrays, and depth errors. The persistence stress suite also invokes garbage collection while serializing and writing objects.

```bash
cargo build -p ube-runtime
cargo test -p ube-compiler --test key_value_store_e2e -- --nocapture
```
