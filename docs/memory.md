# Tracing heap and current ownership boundary

Generated class objects now use a precise mark-and-sweep heap. The compiler emits
pointer-field descriptors and method roots for receivers, parameters, SSA
references, and reference-valued stack slots. Stack slots are allocated once at
function entry, including declarations inside loops.

Roots cover nested calls and exception cleanup. Returning a managed object
transfers a root to the caller before the callee releases its roots. Field reads
publish their result roots atomically; pointer-field reads, writes, root updates,
and collection share a mutex. Scalar accesses do not participate in tracing; generated reads and writes use a
separate lock to keep scalar payloads and nullable presence coherent.
This permits collection while generated workers run concurrently without scanning
uninitialized or concurrently modified pointer bytes.

Root liveness is conservative within a method: a temporary may remain rooted
until its SSA slot is reused or the method returns. Automatic collection runs
after each batch of 256 managed allocations in generated execution. For explicit
collection and managed-heap diagnostics:

```ube
Gc.collect()
print(
  Gc.objects())
print(
  Gc.reclaimed())
```

`Gc.collect()` returns true after completing a collection. Counts describe the
managed tracing heap, not total process memory. Rooted children survive; cycles
without roots are reclaimed. Type metadata for reclaimed objects is removed.

Yielding async methods also publish roots from their heap-backed task frames.
Those roots survive suspension and worker migration and are removed when a frame
is destroyed. A regression clears the source field, collects while the method is
suspended, and checks that the saved object is reclaimed after completion.
Separate conservative escape pins described below still retain their existing
shutdown lifetime; frame rooting does not eliminate those pins.

## Native ABI

`ube_gc_allocate(bytes, align, offsets, count)` returns a C-layout pair
`{ void *object; uint64_t root; }`, creating the object and its initial root
atomically. Failure returns a null pointer and zero root. Offsets identify
pointer fields, each of which must fit within the allocation. Memory is zeroed
before publication.

Native consumers must keep a root alive while using an object. Root add/update/
remove operations reject unknown objects and stale handles. Native pointer
fields use `ube_gc_store_pointer`; unknown fields and foreign targets are
rejected. Generated code uses barriers that also support legacy runtime-created
error layouts and references to resources outside the managed heap.

`ube_gc_collect()` collects this heap. `ube_gc_stats()` reports object, byte,
root, collection, and reclaimed-object counts. Ordinary `ube_free()` calls
cannot invalidate GC-owned allocations. Runtime shutdown drains the tracing
heap before releasing the legacy allocation registry.

## Serialization ownership

Automatic `.toJson()` and `.toXml()` share typed traversal that keeps managed references rooted while reading them. Runtime synchronization protects individual reads; callers must coordinate mutations for a consistent whole-graph snapshot. Collections retain inserted object references until runtime cleanup.

Parsed XML documents, element handles, and `XmlOptions` are runtime-owned and retained until cleanup. They do not add XML fields or cached output to user objects. There is no explicit XML disposal API. Parser/output limits bound individual operations, not total process memory across repeated document creation. See [XML limits](xml.md).

## Validation

The audit suite tests:

- 1,000 two-node cycles through the native ABI, rooted-child preservation,
  direct-free rejection, and four native mutators; all 3,024 objects are reclaimed.
- 1,000 cyclic graphs in a compiled Ube program, live child references, mixed
  scalar/reference layouts, return-value handoff, and `finally` cleanup.
- Four generated workers collecting while they create 1,600 cyclic objects,
  plus managed-object handoff through async returns and channels.
- Automatic collection bounding 5,000 temporary generated objects.
- A million-iteration loop reusing its stack slots.

These are bounded regressions, not process RSS or sustained service soak tests.

## Remaining ownership work

Opaque runtime references are conservatively pinned when managed objects escape
into tasks, closures, channels, server routes, or foreign runtime calls. Those
pins currently last until shutdown. Several native resources, strings, Money
objects, task/result registries, and error records also retain their existing
explicit/shutdown lifetimes. The collector does not yet reclaim all such
resource graphs automatically.

Consequently a long-running service can still grow through those paths even
when ordinary generated temporary objects are collected correctly. Precise
runtime-resource tracing, disposal/unpin contracts, longer contention/sanitizer
runs, and measured process RSS remain release requirements. Rebuild compiler and
runtime together; Cargo-built CLI binaries prefer their current dependency
archive over an older standalone archive beside the executable.
