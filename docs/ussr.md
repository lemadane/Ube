# USSR: Ube Server-Side Rendering

USSR is Ube's built-in, compiled HTML template facility. `html` followed by a backtick template produces an `Html` value. It is a compiler-recognized template form, not a general JavaScript-style tagged-template API. Ordinary backtick templates continue to produce strings.

```kotlin
final name = '<Ube>'
final greeting = html`<h1>Hello, ${name}!</h1>`
// Renders: <h1>Hello, &lt;Ube&gt;!</h1>
```

To try the complete application, run these commands from the repository root:

```bash
cargo build --workspace
./target/debug/ube-compiler check examples/ussr/App.ube
./target/debug/ube-compiler run examples/ussr/App.ube
```

Open http://127.0.0.1:8080 and click Toggle. See the [example walkthrough](../examples/ussr/README.md) for the component files and HTTP checks. Stop the server with Ctrl+C.

The [Space Invaders example](../examples/space-invaders/README.md) uses a static `<canvas>` element, Ube game rules, and a browser adapter for pixel rendering and input. Canvas templates support escaped fallback content and ordinary attributes such as `width` and `height`; script interpolation remains prohibited.

## Typed components

A component is an ordinary method returning `Html`. Put each class in a file matching its name and use `use` to reference another component.

```kotlin
// Todo.ube
class Todo {
    Html render(string title, bool completed) {
        return html`
            <li>
                <span>${title}</span>
                ${completed then html`<b>Done</b>` else html`<i>Pending</i>`}
            </li>
        `
    }
}
```

```kotlin
// App.ube
use Todo

class App {
    start() {
        final server = Server()
        server.get('/', (Request request) -> Response.ok(html`
            <!doctype html>
            <html>
                <head><title>Todos</title></head>
                <body><ul>${Todo().render('Learn Ube', false)}</ul></body>
            </html>
        `))
        server.start(8080)
    }
}
```

`ube check` checks template expressions and component argument types/counts. For example, `new Todo().render(42, false)` is rejected. A string cannot be passed as an `Html` parameter or returned from an `Html` method. `Html()` and declarations named `Html` are rejected: checked templates construct this reserved type.

Nested `Html` is inserted as markup in element content. Strings and numeric/boolean scalars are escaped. Nullable values, arbitrary objects, functions, and collections are not accepted as interpolations. Use a conditional to select a non-null value. Converting a fragment to an ordinary string with `${fragment}` in a normal text block loses its HTML type; embedding that string in an HTML template escapes it again.

## Responses and HTMX

`Response.ok(Html)` sends `text/html; charset=utf-8`. `Response.ok(string)` remains `text/plain; charset=utf-8`. `Response.html(Html)` is an explicit alias; it does not accept an unchecked string. This applies across HTTP/1.1, HTTP/2, and HTTP/3.

Include HTMX once in the page, and use ordinary attributes on components:

```kotlin
html`<li><span>${title}</span><button hx-post="/toggle" hx-target="closest li" hx-swap="outerHTML">Toggle</button></li>`
```

The POST handler returns `Response.ok(updatedComponent)`, containing the replacement `<li>`. No special HTMX adapter is needed. Route matching currently uses exact paths; `/todos/{id}` is not a parameterized route.

See the [runnable Todo example](../examples/ussr/README.md), including session-backed toggle state and ordinary CSS. USSR does not add form decoding, CSRF middleware, static-file serving, a service worker, or PWA installation support.

## Loops

Use existing Ube loops to compose a fragment:

```kotlin
var rows = html``
for (i = 1 to 3) {
    rows = html`${rows}<li>Todo ${i}</li>`
}
final list = html`<ul>${rows}</ul>`
```

There are no `@if` or `@each` directives. Direct interpolation of a loop comprehension or a list is not supported in this first version. Object-list iteration and typed fragment collections require further collection work. Repeated string composition copies the accumulated output; use it for modest lists rather than treating this implementation as a streaming renderer.

## HTML contexts and safety

Literal markup is trusted application source. Interpolated scalar values escape `&`, `<`, `>`, double quotes, and apostrophes. `Html` values can compose only in element content, not in attributes.

The initial grammar accepts a bounded set of common HTML elements, balanced explicit end tags, void elements, quoted attribute values, static comments, and `<!doctype html>`. It is intentionally stricter than browser HTML parsing. It does not implement general HTML conformance or DOM content-model validation.

Dynamic attributes are limited to ordinary presentation/form attributes, `aria-*`, `data-*`, selected HTMX attributes, and supported URL attributes. Dynamic tag/attribute names, unquoted attribute values, inline event handlers, `style`, `srcdoc`, `srcset`, and interpolation inside comments, scripts, styles, title, or textarea are rejected. SVG, MathML, custom component tags, and an unchecked/raw HTML constructor are not supported.

HTMX's `data-hx-*` aliases follow the same restrictions as `hx-*`: supported request attributes validate URLs, and `data-hx-target`, `data-hx-swap`, and `data-hx-select` accept escaped scalar values. Other dynamic HTMX attributes, including event handlers, trigger expressions, headers, and values, are rejected in either spelling. Ordinary `data-*` attributes remain available; application scripts must treat their values as untrusted data.

| Dynamic attribute category | Supported names | Interpolation rule |
| --- | --- | --- |
| HTML URLs | `href`, `src`, `action`, `formaction`, `poster`, `cite` | One interpolation occupying the entire quoted value; URL validation followed by escaping |
| HTMX request URLs | `hx-get`, `hx-post`, `hx-put`, `hx-patch`, `hx-delete`, plus their `data-` aliases | Same rule as HTML URLs |
| HTMX presentation | `hx-target`, `hx-swap`, `hx-select`, plus their `data-` aliases | Escaped scalars inside a quoted value |
| Presentation and form values | `id`, `class`, `title`, `name`, `value`, `alt`, `placeholder`, `role`, `for`, `type`, `width`, `height`, `min`, `max`, `step`, `rows`, `cols`, `colspan`, `rowspan`, `maxlength`, `minlength`, `pattern`, `autocomplete`, `lang`, `dir`, `datetime`, `tabindex` | Escaped scalars inside a quoted value |
| Metadata | `aria-*`, ordinary `data-*` | Escaped scalars inside a quoted value; `data-hx-*` follows the HTMX rules above |

These restrictions apply to interpolated values. Static application source can still contain scripts, styles, and HTMX configuration. Boolean HTML attributes such as `required` can be written literally; dynamic attribute names or conditional attribute omission are not supported.

For a URL attribute, the interpolation must occupy the **entire** quoted value:

```kotlin
final path = `/todos?id=${id}`
final link = html`<a href="${path}">Open</a>`
```

URL values accept relative references and `http:`, `https:`, `mailto:`, or `tel:` schemes. Control characters, backslashes, and other schemes raise a catchable `GenericError` before rendering continues. HTML escaping follows validation. This does not enforce same-origin navigation or application authorization; external HTTPS destinations are allowed. Static URLs remain trusted source.

The compiler checks code types and interpolation contexts; form validation, authorization, and application-specific URL policies remain application responsibilities. No client-side hydration or JavaScript component runtime is generated.

## Verification

`compiler/tests/html_e2e.rs` exercises the real CLI and native executable, including escaping, typed components, closure captures, conditional fragments, URL failures, and rejected templates. `compiler/tests/server_e2e.rs` checks HTML pages, HTMX-style fragment POST responses, plain-text compatibility, and rejection of unsafe URLs. Runtime transport tests check HTML content types over HTTP/2 and HTTP/3.

Run the HTML suite and the USSR server regression from the repository root:

```bash
cargo test -p ube-compiler --test html_e2e
cargo test -p ube-compiler --test server_e2e ussr_components_serve_html_pages_and_htmx_fragments
```

The server test needs permission to bind local sockets. The HTML suite also checks that executable `data-hx-*` aliases are rejected, supported aliases render correctly, and alias URLs receive runtime validation.
