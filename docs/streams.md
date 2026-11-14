# Collection streams

Ube supports sequential, lazy collection pipelines inspired by Java streams. Rust's iterator adapters provide a similar model: intermediate operations describe how to process values, and a terminal operation drives the pipeline. These collection streams are separate from Ube's byte-oriented `InputStream`, `OutputStream`, and `FileStream` APIs.

```ube
final values = [5, 2, 3, 2, 1]
final result = values.stream()
    .filter((int n) -> n > 1)
    .unique()
    .sort()
    .map((int n) -> n * 10)
    .toList()

// result is List<int>, containing 20, 30, 50.
print(result.stream().sum()) // 100
```

Run the [complete example](../examples/streams/App.ube):

```bash
cargo build --workspace
./target/debug/ube-compiler run examples/streams/App.ube
```

It prints `20`, `30`, `50`, `100`, `apple`, `pear`, `3`, `-1`, `true`, `true`, and `true`, each on its own line.

## Sources and types

| Source | Result | Behavior |
| --- | --- | --- |
| `Stream.of(value, ...)` | `Stream<T>` | Snapshot same-typed, non-null values. No arguments produces an empty `Stream<int>`. |
| `Stream.range(start, end)` | `Stream<int>` | Lazy ascending integers, including start and excluding end. Both arguments are `int`; start greater than or equal to end produces an empty stream. |
| `list.stream()` on an existing untyped `List` | `Stream<int>` | Snapshot integer elements. Byte values widen to int. Other stored types raise `GenericError`. |
| `list.stream()` on a typed `List<T>` | `Stream<T>` | Snapshot elements with their preserved type. |

`Stream.of` supports `int`, `long`, `byte`, `decimal`, `bool`, `string`, and class values. Arguments must have exactly the same type; there is no implicit mixed numeric promotion. Nullable values, functions, nested streams, and nested typed collections are not supported as elements in this version.

Existing list literals retain their legacy untyped `List` representation. To stream strings or objects, use `Stream.of('a', 'b')` or `Stream.of(new User(...), new User(...))`. Collected results preserve the element type as `List<T>`.

```ube
Stream<string> names = Stream.of('pear', 'apple')
List<string> sorted = names.sort().toList()
print(sorted[0])
```

Typed lists support `length()`, checked `get(int)` or `[index]`, `add(T)`, `each`, and `.stream()`. They are mutable; adding a value requires the exact element type. List sources snapshot their contents when `.stream()` is called. Later additions to the list do not affect that stream. Object elements are shared references, not deep copies.

## Intermediate operations

| Operation | Result | Behavior |
| --- | --- | --- |
| `filter(Func(T) -> bool)` | `Stream<T>` | Keep matching elements. |
| `map(Func(T) -> R)` | `Stream<R>` | Transform each element; R must be a supported non-null element type. |
| `peek(Func(T) -> void)` | `Stream<T>` | Run an action for each element pulled through this stage. |
| `limit(int/long)` | `Stream<T>` | Pull at most this many elements from upstream. |
| `skip(int/long)` | `Stream<T>` | Discard this many upstream elements. |
| `unique()` | `Stream<T>` | Keep the first occurrence of each value, preserving order. |
| `sort()` | `Stream<T>` | Ascending order for numbers or strings; buffer upstream values when first pulled. |

Callbacks use Ube's typed lambda syntax, for example `(int n) -> n * 2`. Constructing a pipeline does not invoke callbacks. `sort()` is lazy but must consume its upstream pipeline before yielding its first element. `limit(0)` pulls nothing; limits and short-circuiting terminals stop further upstream callbacks.

`sort()` and `unique()` return a new stream handle and leave the source collection unchanged. `sorted()` and `distinct()` remain compatibility aliases, respectively.

Unique strings compare by content. Class values compare by reference identity, without invoking an `equals` method. Decimal equality for deduplication treats NaNs as equal and distinguishes positive and negative zero. Decimal sorting uses a total floating-point order; string sorting follows UTF-8 byte order without locale-aware collation. Custom comparators are not yet supported.

## Terminal operations

| Operation | Result | Empty stream |
| --- | --- | --- |
| `toList()` / `collect()` | `List<T>` | Empty typed list |
| `count()` | `long` | 0 |
| `forEach(Func(T) -> void)` | `void` | No callbacks |
| `some(Func(T) -> bool)` | `bool` | false |
| `every(Func(T) -> bool)` | `bool` | true |
| `none(Func(T) -> bool)` | `bool` | true |
| `findFirst()` | `T?` | null |
| `findFirst(Func(T) -> bool)` | `T?` | null |
| `sum()` | `long` for integer/byte streams, `decimal` for decimal streams | 0 |
| `reduce(T identity, Func(T, T) -> T)` | `T` | identity |

`some(predicate)` returns whether at least one element matches; `every(predicate)` checks that all elements match; `none(predicate)` checks that no elements match. These operations short-circuit as soon as the result is known. The previous names `anyMatch`, `allMatch`, and `noneMatch` remain aliases, respectively.

`collect()` is a no-argument alias for `toList()` in this version; Java-style collector objects are not implemented. `findFirst()` uses Ube nullability, so `stream.findFirst() else fallback` works without an Optional wrapper. `reduce` is an ordered left fold with an explicitly supplied identity.

Use `findFirst(predicate)` to get the first matching element directly. It stops at the first match and returns `null` if no element matches. Both overloads consume the stream, including when a predicate throws.

```ube
final first = Stream.of(1, 2, 3).findFirst((int n) -> n > 1) else -1
print(first) // 2
```

Integer sums detect signed 64-bit overflow and raise `GenericError`. Decimal sums use floating-point arithmetic. All callbacks execute synchronously on the calling thread in encounter order.

## Single use, errors, and cleanup

Like Java streams, each stream handle is single-use. Adding an intermediate operation transfers ownership to a new stream handle. The old handle and its aliases cannot be used for another operation. Terminal operations consume the pipeline, including when a callback throws. Reuse raises a catchable `GenericError`.

```ube
final source = Stream.of(1, 2, 3)
final filtered = source.filter((int n) -> n > 1)
print(filtered.count())
// source.count() and filtered.count() would now raise GenericError.
```

A terminal operation releases pipeline buffers and callback roots on success, short-circuit, or failure. If a pipeline will not be consumed, call `close()` on its current handle, preferably in `finally`. Closing is idempotent. Closing an already-linked old handle does not close the new pipeline. Unconsumed pipelines otherwise remain registered until runtime cleanup.

Captures and object elements are explicitly rooted while retained by a pipeline. Callback exceptions propagate unchanged to the caller's `try`/`catch`; the runtime never invokes user callbacks while holding its registry lock. Collected lists retain the existing collection runtime's ownership behavior. Streams do not close resources referenced by their elements or callbacks.

Negative skip/limit counts raise `GenericError` before linking the pipeline. There is a maximum of 128 intermediate stages; exceeding it consumes and releases the attempted pipeline. Source snapshots, sorted buffers, distinct sets, and collected results are limited to one million elements. Ranges are lazy and are not restricted to one million values unless materialized by one of these operations.

This first version provides sequential streams. Parallel execution, `flatMap`, custom collectors/comparators, infinite generators, and automatic conversion of legacy non-integer lists are not implemented.

## Verification

```bash
cargo test -p ube-compiler --test streams_e2e
cargo test -p ube-runtime sequences::tests --lib
```

Tests cover native execution, typed collections, object/capture lifetimes, snapshots, short-circuiting, exceptions, single-use semantics, invalid signatures, and pipeline root cleanup.
