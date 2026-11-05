# HTTP/3 servers

`server.enableHttp3(udpPort)` adds an HTTP/3 listener over QUIC. Configure a TLS
certificate and key before starting. Routes, response factories, query/header
access, redirects, and sessions are shared with the TCP transports.

```ube
class Application {
    main() {
        final server = Server()
        server.tls('cert.pem', 'key.pem')
        server.enableHttp2()
        server.enableHttp3(8443)
        server.get('/', request -> Response.ok('Hello over ${request.protocol()}'))
        server.start(8443)
    }
}
```

`start(8443)` binds **TCP** port 8443 for HTTPS with HTTP/1.1 and optional HTTP/2.
`enableHttp3(8443)` binds **UDP** port 8443 for HTTP/3. TCP and UDP can share the
same port number. Both listeners currently bind to `127.0.0.1`. HTTP/3 uses TLS
1.3 with ALPN `h3`; it cannot run as cleartext HTTP/3.

## API and startup

| Call | Behavior |
| --- | --- |
| `server.enableHttp3(port)` | Enables HTTP/3 on a UDP port from 0 to 65535. Call before `start()`. |
| `server.http3Port()` | Returns the configured/bound UDP port, or `-1` when disabled. |
| `server.start(port)` | Starts the TCP listener and any configured HTTP/3 listener. Blocks the calling thread. |
| `server.stop()` | Stops accepting connections and allows active responses up to ten seconds to finish. |
| `request.protocol()` | Returns `HTTP/3` for HTTP/3 requests. |
| `request.isHttp3()` | Returns `true` only for HTTP/3 requests. |

UDP port `0` requests an ephemeral port. After startup, `http3Port()` returns the
actual bound port; `port()` continues to report the TCP port. The two ports are
independent. For browser use, choose a fixed UDP port and enable HTTP/2 for TCP
fallback. TLS is configured with the existing `tls(certPath, keyPath)` API and
PEM files. Missing TLS, invalid port values, and UDP bind failures raise
`GenericError`. Startup does not begin serving TCP if UDP binding fails.

HTTPS responses advertise the actual HTTP/3 port with an `Alt-Svc` header, such
as `h3=":8443"; ma=3600`. A browser can discover QUIC after an initial HTTPS
request. The certificate must be trusted by the client and match its hostname.
Direct HTTP/3 clients can connect without first making a TCP request.

## Local example

Generate a development certificate for localhost, then run
[examples/http3/Application.ube](../examples/http3/Application.ube):

```sh
openssl req -x509 -newkey rsa:2048 -nodes -keyout key.pem -out cert.pem \
  -days 1 -subj '/CN=localhost' \
  -addext 'subjectAltName=DNS:localhost,IP:127.0.0.1'
```

With a curl build that supports HTTP/3:

```sh
curl --http3-only --cacert cert.pem https://localhost:8443/
curl --http2 --cacert cert.pem https://localhost:8443/
curl --http1.1 --cacert cert.pem https://localhost:8443/
```

The responses identify the protocol used. Session cookies created over HTTP/3
include `Secure`, `HttpOnly`, and `SameSite=Lax`, and the same session can be
used through HTTPS over HTTP/1.1 or HTTP/2. Browser-facing credentials and OAuth
callbacks use the same route/session APIs on all three protocols.

## Runtime limits

HTTP/3 supports concurrent request streams and QUIC flow control, with up to
100 concurrent bidirectional streams per connection and 16 KiB request field
sections. Handshake/request I/O has a ten-second timeout; QUIC idle connections
expire after thirty seconds. Application callbacks run serially on the server
thread, so a blocking handler still delays other callbacks.

0-RTT application data is disabled. WebTransport, HTTP datagrams, server push,
and a Ube HTTP/3 client API are not implemented. `request.body()` buffers HTTP/3
DATA frames up to 1 MiB, with a ten-second body-read deadline. Other streams
continue while uploads are pending. See [request bodies](request-bodies.md).
Middleware, static-file serving, and WebSockets retain their existing runtime
limitations.
