# Sessions

`request.session()` accesses a server-side `Session` for the current request.
Ube handles the session cookie automatically for HTTP/1.1, HTTP/2, and HTTP/3, including
HTTPS. No session middleware or explicit response-cookie calls are required.

```ube
class App {
    start() {
        final server = Server()
        server.get(
          '/',
          (Request request) -> {
              final theme = request.session()
                .get('theme')
                ?? 'light'
              return Response.ok('Theme: ${theme}')
          })
        server.post(
          '/theme',
          (Request request) -> {
              request.session()
                .set(
                  'theme',
                  'dark')
              return Response.ok('Theme saved')
          })
        server.post(
          '/logout',
          (Request request) -> {
              request.session()
                .destroy()
              return Response.ok('Session deleted')
          })
        server.start(8080)
    }
}
```

## API

| Call | Returns | Behavior |
| --- | --- | --- |
| `request.session()` | `Session` | Returns the same session handle throughout this request. |
| `session.get(key)` | `string?` | Reads a value; returns `null` if absent or expired. |
| `session.set(key, value)` | `void` | Stores a string, creating a session if necessary. |
| `session.remove(key)` | `void` | Removes a value; a missing key is harmless. |
| `session.regenerate()` | `void` | Replaces the ID, preserves data, and restarts the lifetime. |
| `session.destroy()` | `void` | Deletes the session and expires its cookie. |

Keys and values must be non-null strings. `Session` cannot be constructed or
used through static methods; obtain it through a request. A handle is valid only
during its request. Retaining it in a field or task does not extend its lifetime;
using it after the request ends raises `GenericError`.

## Storage and lifetime

Each `Server` has an isolated store in memory. Data survives between requests,
including requests using different HTTP versions. Sessions expire 30 minutes
after creation or regeneration; ordinary reads and writes do not extend this
fixed lifetime. Expired entries are removed when sessions are accessed.

Reading a missing value or removing a missing key does not create a session or
send a cookie. The first `set()` or `regenerate()` creates one. Destroying a
session clears its cookie; a subsequent `set()` in the same request can create
a new session. Mutations take effect immediately, even if the handler later
returns an error.

The browser holds only an opaque 256-bit random ID in `ube_session`. The cookie
has `Path=/`, `HttpOnly`, and `SameSite=Lax`; HTTPS adds `Secure`. The creation
cookie has no browser persistence deadline, while the server enforces its own
30-minute expiration. Unknown, malformed, expired, or duplicate session IDs do
not restore a session. Regeneration immediately invalidates the previous ID.

For login, validate credentials first, then call `session.regenerate()` before
storing the authenticated user's ID. Use HTTPS for authenticated sessions and
`destroy()` for logout. Cookie settings do not replace application authorization
or CSRF protection. These choices follow the [OWASP session management guidance](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html).

Storage is local to one server instance and is lost on process restart. It is
not shared between processes, and persistent/external session stores and
configurable lifetimes are not implemented yet.

## Try it

Run [the session example](../examples/sessions/App.ube), then use a cookie jar:

```sh
curl -c /tmp/ube-cookies.txt -b /tmp/ube-cookies.txt -X POST http://127.0.0.1:8080/theme
curl -b /tmp/ube-cookies.txt http://127.0.0.1:8080/
curl -c /tmp/ube-cookies.txt -b /tmp/ube-cookies.txt -X POST http://127.0.0.1:8080/logout
```
