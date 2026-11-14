# Open and restricted traits

Ube classes and traits do not inherit from other classes or traits. Trait satisfaction is implicit: a class must provide compatible instance-method signatures, with trait defaults filling missing methods. There is no `implements` declaration.

An ordinary trait is open:

```ube
trait Printable {
    print()
}
```

Any compatible concrete Ube class loaded into the application qualifies, including classes loaded through `use`. Runtime objects, foreign handles, and parsed JSON/XML objects do not qualify merely because their methods match. Wrap foreign data in an application class to satisfy an ordinary trait. Generated `Mockery` classes belong to the application and remain usable with ordinary traits.

A trait with a `permit` list is restricted:

```ube
use LegitUser
use Admin

trait AuthorizedUser permit LegitUser, Admin {
    bool isAuthorized()
}
```

Only `LegitUser` and `Admin` can satisfy `AuthorizedUser`, and each must still provide a compatible `isAuthorized()` method. Listing a class does not bypass method checking. A different class with exactly the same methods is rejected when assigned or passed as `AuthorizedUser`.

```ube
// LegitUser.ube
class LegitUser {
    bool isAuthorized() { return true }
}
```

```ube
// Admin.ube
class Admin {
    bool isAuthorized() { return true }
}
```

```ube
// App.ube
use AuthorizedUser
use Admin

class App {
    bool authorize(AuthorizedUser user) {
        return user.isAuthorized()
    }

    start() {
        print(this.authorize(new Admin()))
        // this.authorize(new Visitor()) is rejected, even if Visitor has isAuthorized().
    }
}
```

Each declaration belongs in a separate file matching its type name. Import permitted classes with ordinary `use` declarations; a permit list does not load files automatically. Forward references among loaded declarations work.

## Rules

- `permit` follows the trait name and takes one or more comma-separated concrete class names.
- Unknown names, trait names, repeated names, and incompatible classes are declaration errors, even if the trait is never used.
- Restrictions apply to variable initialization and assignment, fields, arguments, callbacks, return values (including method-level `yield`), and nullable trait values.
- Channel aliases must have exactly the same payload type: a `Channel<AuthorizedUser>` cannot become a `Channel<OpenUser>`, since that would allow sending an unlisted implementation through the alias.
- Default methods are available only to compatible, permitted classes.
- Ordinary structural conversion to an open trait remains available.
- Ordinary traits are open within the compiled application, not to foreign objects. Catch clauses use the same application-class boundary.
- The built-in `Error` contract remains able to represent runtime errors. An `Error` value cannot implicitly convert to an ordinary application trait, since it may contain a foreign runtime error.
- An open trait value cannot be converted to a restricted trait: its erased type cannot guarantee a permitted concrete implementation.
- A restricted trait value can convert to another restricted trait only when its permit list is a subset of the target's list and its method signatures satisfy the target. This is type compatibility, not inheritance.
- Catch clauses using restricted error-compatible traits check concrete identity. An unlisted error with matching methods does not match that handler.
- Generated `Mockery` implementations cannot satisfy a restricted trait; `Mockery.mock` rejects restricted contracts. Use a permitted concrete test implementation instead.
- `permit` is contextual in a trait declaration. Existing variables and methods named `permit` remain valid.

A permit list controls type membership; it does not authenticate users or authorize requests by itself. Applications must still implement the actual authorization policy.

## Example and tests

From the repository root:

```sh
cargo build --workspace
./target/debug/ube-compiler run examples/traits/App.ube
cargo test -p ube-compiler --test trait_permits_e2e
```

The example prints `true`, `true`, and `false`, on separate lines. Tests compile and execute allowed conversions, default dispatch, restricted exception handlers, and check rejection of unlisted implementations and invalid declarations.
