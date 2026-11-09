# Mockery

`Mockery` is Ube's built-in mocking API. It creates concrete implementations of
declared structural traits, so production classes receive mocks through ordinary
constructor or method parameters. No inheritance, proxies, or runtime code
generation are required.

```ube
trait UserLookup {
    string name(int id)
}

class Profile(UserLookup users) {
    string title(int id) {
        return this._users.name(id)
    }
}

class ProfileTests {
    testTitle() {
        final users = Mockery.mock(UserLookup)
        try {
            Mockery.returns(users, 'name', 'Ada')
            Testicle.equal('Ada', Profile(users).title(42))
            Mockery.verify(users, 'name', 1)
        } finally {
            Mockery.dispose(users)
        }
    }
}
```

Run the included example with `ube test examples/testing/ProfileTests.ube`.
Mockery also works in ordinary Ube programs, not just the test runner.

## API

All operations are class calls on `Mockery`; Mockery itself has no constructor.

| Operation | Result / behavior |
| --- | --- |
| `mock(TraitName)` | Creates a fresh, strict mock implementing the declared trait. |
| `returns(mock, 'method', value)` | Configures a primitive return value. Its type must exactly match the method's return type. |
| `allow(mock, 'method')` | Configures a method with an implicit void return to succeed. |
| `throws(mock, 'method', message)` | Configures a method to throw `MockeryError` with the supplied string. |
| `verify(mock, 'method', count)` | Requires exactly `count` invocations; `count` is a non-negative `int`. A mismatch throws `AssertionError`. |
| `calls(mock, 'method')` | Returns the invocation count as `long`, including calls that threw. |
| `verifyNoCalls(mock)` | Requires no recorded calls on the mock; otherwise throws `AssertionError`. |
| `clearCalls(mock)` | Clears recorded counts while retaining configured behavior. |
| `reset(mock)` | Clears counts and all configured behavior. The mock becomes strict and unconfigured again. |
| `dispose(mock)` | Releases registry state. Further operations or invocations throw `MockeryError`. |

For every operation taking a method name, the name must be a string literal
declared on the mocked trait. The compiler rejects misspellings, incompatible
return values, invalid argument counts, and invalid handles. Keep the inferred
handle from `mock(...)` for configuration and verification; pass it into
trait-typed production APIs for use. A value already erased to a trait type
cannot serve as the configuration handle.

Unconfigured calls always throw `MockeryError`, including methods with default
trait implementations and methods returning void. There are no implicit zero,
false, or null defaults. Configuration calls do not count as invocations.
Reconfiguring a method replaces its behavior and preserves its call count.

Stubs currently match **every argument combination** for a method. Production
call signatures are still type checked. For example, `returns(users, 'name',
'Ada')` returns `'Ada'` for every valid integer ID; verifying one call does not
verify which ID was passed.

## Exceptions and async consumers

```ube
testFailure() {
    final users = Mockery.mock(UserLookup)
    Mockery.throws(users, 'name', 'database offline')
    var caught = false
    try { Profile(users).title(42) }
    catch (MockeryError error) {
        caught = true
        Testicle.equal('database offline', error.message())
    }
    Testicle.isTrue(caught)
    Mockery.verify(users, 'name', 1)
    Mockery.dispose(users)
}
```

Mocks can be shared by async consumers. Registry changes, dispatch, and counts
are synchronized, and invocation observes one configured behavior atomically.
Await all consumers before checking final counts or disposing a mock. Verification
examines the count at that instant; it does not wait for future calls. There is no
call-order guarantee across tasks.

## Current scope and lifetime

- Mock declared synchronous traits whose methods return `int`, `long`, `byte`,
  `bool`, `decimal`, `string`, or implicit void. Parameters use ordinary Ube type
  checking. Async trait methods, nullable returns, objects, task returns, and
  collection returns are rejected by this initial API.
- `mockeryId`, `_mockeryId`, and type names beginning with `UbeMockery_` are reserved for generated
  mock implementations. Internal `_call_*` entry points cannot be called from
  user classes. Mock classes are generated only for requested traits.
- Dispose handles explicitly when done. Registry state otherwise lasts until
  runtime shutdown. Handles are monotonic and never reused, so an old disposed
  wrapper cannot address a newly created mock.
- Returned strings remain valid through reconfiguration, reset, and disposal.
  One retained string allocation per string-stub configuration lasts until
  runtime shutdown; repeated calls reuse that string. This is intended for tests,
  not unrestricted long-running production allocation.
- Argument matching/capture, ordered verification, return sequences, callbacks,
  spies on real instances, deep stubs, and custom thrown exception objects are
  not implemented.

The compiler generates ordinary concrete methods and uses Ube's existing trait
dispatch and exception ABI. `AssertionError` and `MockeryError` remain catchable,
and failed verification is reported as a failed test by `ube test`.
