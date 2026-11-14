# HTTP/2 servers

Enable HTTP/2 before starting a server. Existing route callbacks and response
factories work with both HTTP/1.1 and HTTP/2.

```ube
class App {
    start() {
        final server = new Server()
        server.enableHttp2()
        server.tls(
          'cert.pem',
          'key.pem')
        server.get(
          '/',
          (Request request) -> Response.ok(
            'Hello over ${request.protocol()}'))
        server.start(8443)
    }
}
```

`tls(certPath, keyPath)` loads a PEM certificate chain and private key. Invalid
files or a mismatched key raise `GenericError`. TLS clients negotiate `h2` or
`http/1.1` through ALPN. HTTP/1.1 remains available on the same port; clients
without ALPN use HTTP/1.1. Configure TLS and HTTP/2 before calling `start()`.
The listener currently binds to `127.0.0.1`.

For local testing, omit `tls(...)` to accept cleartext HTTP/2 with prior
knowledge (h2c), alongside ordinary HTTP/1.1. HTTP/1.1 `Upgrade: h2c` is not
implemented. Browsers generally require HTTPS for HTTP/2.

With a curl build that supports HTTP/2:

```sh
# HTTPS; use --cacert with a local certificate authority if needed.
curl --http2 https://localhost:8443/

# Cleartext example: run examples/http2/App.ube first.
curl --http2-prior-knowledge http://127.0.0.1:8080/
```

`request.protocol()` returns `HTTP/2` for HTTP/2 requests, or the actual HTTP/1
version for HTTP/1 requests. `request.isHttp3()` returns false for both.
HTTP/2 connections support concurrent streams and response flow control.
Callbacks run serially on the server thread; concurrent streams do not make
blocking Ube handlers execute in parallel. `server.stop()` stops accepting new
connections and allows active connections up to ten seconds to finish.

`request.body()` supports HTTP/2 DATA frames with a 1 MiB payload limit and a
ten-second body-read deadline. Uploads respect flow control and do not block
other streams; callbacks run after the complete body is validated. See
[request bodies](request-bodies.md) for framing rules and rejection behavior.
Middleware retains its existing runtime limitations.
HTTP/3 uses a separate QUIC listener; see [HTTP/3 setup](http3.md). A Ube HTTP
client API and WebTransport are not implemented.
