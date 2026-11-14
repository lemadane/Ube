# Request bodies

`request.body()` returns the complete request payload as a string. HTTP/1.0,
HTTP/1.1, HTTP/2, and HTTP/3 are supported. For example:

```ube
class App {
    start() {
        final server = new Server()
        server.post(
          '/echo',
          (Request request) -> Response.ok(
            request.body()))
        server.put(
          '/echo',
          (Request request) -> Response.ok(
            request.body()))
        server.start(8080)
    }
}
```

JSON sent to these routes is returned verbatim; this example does not deserialize
JSON. Repeated calls return the same text.
Invalid UTF-8 and embedded null bytes raise catchable `GenericError` when the
handler calls `body()`.

Every transport validates and buffers the complete body before invoking a route.
The body limit is 1 MiB (413 on overflow). Reading a body has a ten-second deadline
(408 on timeout). HTTP/1.x additionally includes headers in this deadline.

HTTP/1.x uses these additional fixed limits:

| Limit | Value | Rejection |
| --- | --- | --- |
| Request line and headers together | 16 KiB, at most 128 headers | 431 |
| Chunk size line | 8 KiB | 413 |
| Chunk size lines together | 64 KiB | 413 |
| Reading headers and body together | 10 seconds | 408 |

For HTTP/1.x, an unframed request has an empty body. A single decimal
`Content-Length` is accepted. HTTP/1.1 additionally accepts a
single case-insensitive `Transfer-Encoding: chunked`; chunk sizes are hexadecimal,
data ends in CRLF, and a zero-size chunk plus an empty trailer section terminates
the payload. Chunked transfer encoding is rejected for HTTP/1.0.

Malformed or truncated framing returns 400 and never invokes the route. This
includes duplicate or comma-separated lengths, both framing headers together,
duplicate transfer encodings, unsupported transfer codings, chunk extensions,
nonempty trailers, and advertised `Trailer` headers. Header lines require CRLF;
folded headers are rejected. The server closes the connection after the response.

`Expect: 100-continue` is supported for HTTP/1.1. The server validates framing and
the declared length before sending the interim response. Unsupported expectations
return 417. A chunked body's cumulative limit is enforced as chunks arrive.

## HTTP/2 and HTTP/3

Bodies are accumulated from DATA frames until the upload stream ends. No
`Content-Length` is required; when present, a single decimal value must match the
received byte count. Empty streams produce an empty string. `Transfer-Encoding`
is forbidden on these transports. Nonempty trailers and advertised `Trailer`
headers are currently unsupported and rejected.

Malformed lengths and mismatched lengths cause rejection without invoking the
handler. A malformed stream may be reset with the protocol's error code instead
of receiving an HTTP error response. Oversized bodies receive 413. Rejections,
cancellation, and timeouts leave the connection usable for other streams.
`Expect: 100-continue` is supported after validating framing and any declared
length; unsupported expectations receive 417.

Uploads run concurrently on each connection and respect transport flow control.
One unfinished upload does not block other streams. App callbacks still
run serially on the calling server thread after their complete body is available.

See the framing requirements in [HTTP/2 RFC 9113](https://www.rfc-editor.org/rfc/rfc9113.html#section-8.1)
and [HTTP/3 RFC 9114](https://www.rfc-editor.org/rfc/rfc9114.html#section-4.1).
