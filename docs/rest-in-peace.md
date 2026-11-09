# RestInPeace

RestInPeace tests running HTTP services with fluent requests and Testicle
assertions. It sends real network requests; use Mockery to replace an in-process
trait dependency instead.

```ube
class ApiTests {
    testEcho() {
        final request = RestInPeace.at('http://127.0.0.1:3000')
          .accept('application/json')
          .timeout(3000)
        try {
            final response = request.json(`{"name":"Ada"}`)
              .post('/echo')
            try {
                response.expectStatus(200)
                  .expectJsonString(
                    '/name',
                    'Ada')
                Testicle.equal(
                  'Ada',
                  response.jsonString('/name'))
                Testicle.equal(
                  200,
                  response.status())
            } finally {
                response.close()
            }
        } finally {
            request.close()
        }
    }
}
```

Run the included service in one terminal and its tests in another:

```sh
ube run examples/rest_in_peace/App.ube
```

```sh
ube test examples/rest_in_peace --timeout 15000
```

## Request API

`RestInPeace.given()` creates a `RestInPeaceRequest` without a base URL.
`RestInPeace.at(baseUrl)` creates one with a base URL. These are the only class
factories; request and response handles cannot be constructed directly.

Configuration methods return the same request handle for chaining:

| Method | Behavior |
| --- | --- |
| `baseUrl(string)` | Sets an absolute HTTP(S) base URL. |
| `header(name, value)` | Sets/replaces a header, case-insensitively. |
| `query(name, value)` | Appends a query pair with URL encoding; repeated names remain repeated. |
| `body(string)` | Sets the UTF-8 request body. |
| `json(string)` | Validates JSON, sets the body and `Content-Type: application/json`. |
| `contentType(string)` | Sets `Content-Type`. |
| `accept(string)` | Sets `Accept`. |
| `bearer(string)` | Sets `Authorization: Bearer ...`. |
| `timeout(int milliseconds)` | Sets the HTTP request timeout, including response streaming; default 10,000, range 1–300,000. Connection setup is additionally limited to at most 5 seconds. |
| `maxBytes(int)` | Sets the buffered response-body limit; default 1 MiB, range 1–16 MiB. |
| `caFile(string path)` | Adds a PEM test CA certificate while retaining hostname and certificate verification. Maximum certificate file size is 64 KiB. |

`get(path)`, `post(path)`, `put(path)`, `patch(path)`, `delete(path)`, `head(path)`,
and `options(path)` send a request and return `RestInPeaceResponse`.
`send(method, path)` supports other HTTP method tokens except CONNECT tunnels.

Paths use standard URL resolution: `/users` replaces the base path; `users`
resolves relative to it. Without a base URL, pass an absolute URL to the send
method. URLs require HTTP(S), a host, and no embedded credentials or fragment.
The final encoded URL is limited to 16 KiB.

Builders are reusable and mutable. Headers, query pairs, and body persist across
sends, and configuration affects every alias of the builder. Sending snapshots
the configuration before doing network work. Use separate builders for independent
requests and parallel tests. `close()` releases request state; sending through a
closed handle raises `RestInPeaceError`.

Request bodies are limited to 16 MiB. Up to 128 headers and 128 query pairs are
accepted, with 16 KiB per header or query pair. The HTTP client manages
`Content-Length` and `Transfer-Encoding`; contradictory manual framing is
rejected. Use the `Cookie` header explicitly when testing session cookies.

## Response API

| Method | Return |
| --- | --- |
| `status()` | HTTP status as `int`. |
| `body()` | Buffered UTF-8 text as `string`. |
| `header(name)` | First matching header as `string?`; `null` if absent, `''` if present but empty. |
| `hasHeader(name)` | Whether the header exists. |
| `elapsedMs()` | Request time through body buffering as `long`; useful for diagnostics. |
| `protocol()` | Negotiated protocol, such as `'HTTP/1.1'` or `'HTTP/2'`. |
| `jsonString(pointer)` | JSON string. |
| `jsonInt(pointer)` | JSON integer fitting signed 32-bit `int`. |
| `jsonLong(pointer)` | JSON integer fitting signed 64-bit `long`. |
| `jsonDecimal(pointer)` | JSON number as floating-point `decimal`. |
| `jsonBool(pointer)` | JSON boolean. |
| `jsonIsNull(pointer)` | Whether an existing JSON value is null. |
| `close()` | Releases buffered response state. |

JSON selectors use JSON Pointer, not JSONPath: `/user/name`, `/items/0`, and
`''` for the whole document. Escape `/` as `~1` and `~` as `~0` within a property
name. Missing paths, malformed JSON, wrong value types, and integer overflow raise
`RestInPeaceError`. Missing values do not silently become zero, false, or null.
JSON decimals retain the language's floating-point behavior; use JSON strings
for exact monetary values.

## Assertions and failures

Ordinary Testicle assertions work with every response getter. Responses also
provide these fluent checks, each returning the same response on success:

```ube
response.expectStatus(201)
  .expectHeader(
    'content-type',
    'application/json')
  .expectJsonString(
    '/name',
    'Ada')
response.expectBody('exact text')
```

Checks use Testicle's exception ABI and throw `AssertionError` on mismatch.
Transport/configuration/decoding failures throw `RestInPeaceError`; both satisfy
`Error` and can be caught normally. `ube test` reports uncaught errors as failures
and continues running subsequent tests.

HTTP 4xx and 5xx responses are normal responses for assertions. Redirects are
returned without following them automatically. Header comparisons are exact
values with case-insensitive header names; there are no substring or media-type
normalizations. The full response body is buffered before assertions run.

## Protocols, cleanup, and current limits

HTTP/1.x and HTTPS with HTTP/2 ALPN negotiation are supported. TLS verification is
enabled; use `caFile` for a local certificate. HTTP/3 and cleartext HTTP/2 prior
knowledge are not exposed by this initial client.

Requests are blocking operations. They can be called from async Ube tasks; client
runtime creation and teardown occur outside Tokio worker contexts. This is a
testing API, rather than a nonblocking high-throughput HTTP client. The test
runner's per-test timeout remains a separate outer limit and should allow time
for all requests in that test.

Close requests and responses when done. Handles are validated and never reused.
Runtime shutdown releases remaining handle state; strings already returned to Ube
retain the runtime's existing string lifetime and remain valid after `close()`.
When catching assertion failures, assign the response before asserting so a
`finally` block can close it.

This version supports UTF-8 text bodies, including chunked transfer decoding. It
rejects binary/non-UTF-8 bodies and embedded null bytes. It does not expose file
uploads/downloads, streaming responses, automatic cookies, JSONPath, schema
validation, retries, or automatic compressed-body decoding. Start and stop the
service under test separately; RestInPeace does not launch applications.
