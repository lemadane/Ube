# Ube cache TCP protocol

This documents the initial protocol used by `Cache.connect`, embedded `cache.listen`, and `ube cache`. It is a Ube-specific protocol; Memcached, Redis, and HTTP clients cannot use it directly. Future protocol changes may require matching client/server versions.

## Connection and framing

Each TCP connection carries exactly one request and one response, after which the server closes it. There is no connection pooling, pipelining, TLS, or compression in this implementation.

```text
4-byte unsigned big-endian JSON byte length
UTF-8 JSON body of exactly that length
```

The length excludes its own four-byte header. Length zero and bodies larger than 8 MiB are rejected. Malformed JSON is rejected. Clients must read exactly four bytes for the header and then exactly the announced body size; one socket read is not guaranteed to return a complete frame.

The client uses a two-second TCP connection timeout. Each complete frame read and frame write has a two-second deadline. Storage processing is not bounded by those I/O deadlines; a client can time out while the server is still completing a mutation or trying to send its response. See [mutation uncertainty](cache-recipes.md#treat-a-timeout-as-an-uncertain-mutation-result).

## Request body

Every field below is required, including unused `value` and `number` fields. JSON object field order is irrelevant:

```json
{
  "token": "example-only-token-replace-me",
  "op": "set",
  "key": "greeting",
  "value": "hello",
  "number": 60000
}
```

| Field | Type | Meaning |
| --- | --- | --- |
| `token` | UTF-8 string | Shared authentication token, 16–4,096 bytes |
| `op` | String | `get`, `set`, `add`, `increment`, or `delete` |
| `key` | UTF-8 string | 1–1,024 bytes, no NUL |
| `value` | UTF-8 string | Value for `set`/`add`; at most 1 MiB, no NUL. Send an empty string for other operations. |
| `number` | Signed 64-bit JSON integer | TTL milliseconds for `set`/`add`; signed delta for `increment`. Send zero for `get`/`delete`. |

The example token is illustrative, not a production credential. The server authenticates before executing the operation. Tokens and values are transmitted without encryption; use a separate encrypted tunnel or TLS proxy where needed. Internal authentication comparison does not provide transport confidentiality.

For `set` and `add`, a zero TTL means no expiry and a negative TTL is invalid. For `increment`, the existing string must parse as a signed 64-bit integer and the result must fit that range. The original deadline is retained. Get/delete ignore the placeholder value and numeric argument after their types have been validated.

## Responses

A successful response contains `result`:

| Operation | Example response | Meaning |
| --- | --- | --- |
| `get` hit | `{"result":"hello"}` | Cached string, including an empty string if stored |
| `get` miss | `{"result":null}` | Absent or expired |
| `set` | `{"result":true}` | Mutation committed/applied; Ube's public method returns `void` |
| `add` | `{"result":true}` / `{"result":false}` | Inserted / already existed |
| `increment` | `{"result":42}` | Updated signed integer |
| `delete` | `{"result":true}` / `{"result":false}` | Removed an unexpired entry / no such entry |

Failure responses contain an error string:

```json
{"error":"Cache authentication failed"}
```

Errors include authentication, malformed requests, invalid keys/values, capacity, unsupported operations, and storage failures. Strings are diagnostic text, not stable machine-readable error codes. An invalid or truncated connection may close before a response can be delivered. There is no request identifier, server-side deduplication, remote enumeration, remote shutdown, or administrative API.

## Concurrency and resource limits

The listener has four workers and a bounded queue of 64 accepted connections. Each worker processes a single connection at a time. A full queue may cause a new connection to close without a framed error. The store serializes mutations, so concurrent clients see atomic adds and increments on that owning cache. A read followed by a later write is not a transaction.

Persistent mutations and their associated evictions commit together before success. Transport loss after commit does not roll back the mutation. Clients must decide whether a retry is safe for their operation.

See [the reference](cache.md) for capacity/TTL semantics and [deployment](cache-deployment.md) for authentication configuration and recovery.
