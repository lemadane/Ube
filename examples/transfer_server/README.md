# Bounded text-file transfer

This example uses implemented APIs: exact route paths, buffered UTF-8 request
bodies, file reads/writes, and JSON metadata. It stores one file at
`uploads/example.txt` in the working directory. Run it in a private directory;
it has no authentication and binds to loopback.

```bash
ube run FileTransferApplication.ube
```

Without `cert.pem` and `key.pem`, it serves HTTP/1.1 on port 8443:

```bash
curl --data-binary 'Hello from Ube' http://127.0.0.1:8443/files/example.txt
curl http://127.0.0.1:8443/files/example.txt
curl http://127.0.0.1:8443/files/example.txt/info
curl -X DELETE http://127.0.0.1:8443/files/example.txt
```

If both certificate files exist, it enables TLS, HTTP/2, and HTTP/3 on TCP/UDP
8443. See [HTTP/3 setup](../../docs/http3.md) for generating a local certificate
and using curl with `--cacert cert.pem` and an `https://localhost:8443` URL.

Uploads are limited to 1 MiB and must be UTF-8 without embedded NUL. Downloads
are buffered strings. Binary HTTP streaming, dynamic `:name` route parameters,
range/resumable transfers, SHA-256 HTTP metadata, and parallel Ube handler
execution are not provided by this example. The previous prototype advertised
these features without supported compiler/runtime APIs.
