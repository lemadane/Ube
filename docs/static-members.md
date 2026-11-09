# Static members

Members belong to instances unless declared `static`. Static fields have one
binding per class for the lifetime of the application. Constructing another
instance does not initialize or copy those fields.

```ube
class Counter {
    static var int total = 0
    static final int limit = 100

    static int current() {
        return static.total
    }

    static increment() {
        static.total = static.current() + 1
    }

    tick() {
        static.increment()
    }
}
```

Inside `Counter`, its static members require `static.total` and
`static.current()`, including when accessed from an instance method. Outside
`Counter`, use `Counter.total` and `Counter.current()`. Accessing a static member
through an instance, using `Counter.total` inside `Counter`, or omitting the
receiver is an error. Calls to another class's static members use that class's
name, including inside a static method.

Static fields use direct field syntax: `Counter.total = 5` writes the field.
Instance fields continue to use their existing accessors and private backing
fields. `static final` prevents rebinding; `static var` permits assignment.
A final object binding does not make that object's mutable fields immutable.
An underscore-prefixed static field or method is private to its declaring class.
Member names must be unique within the class.

Static methods have parameters, return types, and exception behavior like
instance methods, but have no `this`. They can operate on an instance passed
explicitly as a parameter. To run an ordinary static method as a task, use
`Async.start(() -> Calculator.calculate())` outside its class or
`Async.start(() -> static.calculate())` inside it, then call `.await()`. Traits describe instance behavior: static
methods do not satisfy a trait and cannot be declared in traits.

## Initialization

All static fields initialize once before `start()` runs, whether or not their
class is constructed. Startup first publishes default values for all fields,
then evaluates explicit initializers in class and field declaration order.
Imported source files precede the source that imports them. A field without an
initializer uses normal declaration zero-initialization rules, including fresh
zero-valued objects for user class types. Runtime handles need an explicit
initializer or a nullable type.

Reading a field before its explicit initializer completes raises `GenericError`.
This includes forward and cyclic reads through static methods, and prevents an
uninitialized reference from appearing as a non-nullable value. Fields without
explicit initializers are ready after default initialization. Writing a mutable
field publishes its value, so later reads can use it even before its initializer
runs; the explicit initializer will still run in declaration order.

Initializer expressions have static context: use `static.` for members of the
owning class; `this` is unavailable. An uncaught initializer error terminates
startup before application `start()` executes.

Both instance `start()` and `static start()` are supported. An instance entry point
receives the usual zero-valued application object; a static entry point requires
no application instance.

## Concurrency and lifetime

Individual static field loads and stores are serialized by the runtime, including
nullable presence and payload. This prevents torn values; a sequence of accesses
is **not atomic**. For example, `static.total = static.total + 1` can lose updates
when tasks execute it concurrently. Use an appropriate synchronization or
single-owner design for compound updates and invariants across fields.

Static object references remain GC roots while stored. Reads protect the object
in the reader's frame, and replacing a reference releases the slot's previous
root. Static bindings persist until runtime cleanup; resource handles still need
their normal explicit close operations where applicable.
