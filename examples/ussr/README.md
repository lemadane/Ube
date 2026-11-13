# USSR Todo example

From the repository root:

```bash
cargo build --workspace
./target/debug/ube-compiler check examples/ussr/App.ube
./target/debug/ube-compiler run examples/ussr/App.ube
```

Open http://127.0.0.1:8080. The page renders `TodoPage` and `Todo` components. HTMX replaces the Todo row when Toggle is clicked. State is stored in the current session and survives page refreshes until the session expires or the server stops. Sessions have a fixed 30-minute lifetime; see [sessions](../../docs/sessions.md). HTMX loads from a CDN, so browser interaction needs internet access. Stop the server with Ctrl+C.

The example consists of three files:

- [App.ube](App.ube) registers `GET /` and `POST /toggle` and reads or updates the session.
- [TodoPage.ube](TodoPage.ube) renders the document, CSS, HTMX script, and Todo component.
- [Todo.ube](Todo.ube) renders one typed `Html` fragment with its label, status, and Toggle button.

The HTTP interaction can also be exercised without HTMX:

```bash
curl -i -c /tmp/ube-todo.cookies http://127.0.0.1:8080/
curl -i -b /tmp/ube-todo.cookies -X POST http://127.0.0.1:8080/toggle
curl -i -b /tmp/ube-todo.cookies http://127.0.0.1:8080/
```

The first request returns a full HTML document showing Pending. The POST returns only a `<li>` fragment showing Done, with `class="todo completed"`. The final GET returns the full page with Done still selected. Both routes return `Content-Type: text/html; charset=utf-8`; the title text appears in the source as `Learn Ube &amp; HTML`. Repeating the POST toggles the same Todo back to Pending.

This is a local rendering demonstration, not a production Todo service: it has one fixed Todo, in-memory session state, and no persistence or CSRF middleware. See the [USSR guide](../../docs/ussr.md) for supported HTML contexts, type checking, and current limits.
