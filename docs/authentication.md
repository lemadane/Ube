# OAuth 2.0 and JWT

Ube provides an OAuth 2.0 client for external providers and signed JWT creation
and verification. These are separate facilities: OAuth access tokens may be
opaque strings, and OAuth authorization does not by itself establish a user's
identity. Applications decide what authorization or identity evidence they
require before marking a user as logged in.

## JWT

`Jwt` supports **HS256** (a shared secret) and **RS256** (RSA PEM keys).

```ube
final key = File.read('jwt-secret.txt')
final expires = DateTime.now().timestamp() + 3600L
final payload = '{"sub":"user-123","iss":"my-app","aud":"my-api","exp":${expires}}'
final token = Jwt.sign(payload, key, 'HS256')
final claims = Jwt.verify(token, key, 'HS256', 'my-app', 'my-api')
try {
    print(claims.get('sub') ?? 'missing subject')
} finally {
    claims.close()
}
```

| Call | Returns | Behavior |
| --- | --- | --- |
| `Jwt.sign(claimsJson, key, algorithm)` | `string` | Signs a JSON object using the explicit algorithm. |
| `Jwt.verify(token, key, algorithm, issuer, audience)` | `JwtClaims` | Verifies the signature and registered claims before returning any claims. |
| `claims.get(name)` | `string?` | Reads a string claim; returns `null` for absent/null claims and raises an error for non-string values. |
| `claims.json()` | `string` | Returns the complete verified claims as JSON text, including numeric and structured values. |
| `claims.close()` | `void` | Releases the claims handle; subsequent reads fail. |

Both signing and verification require `exp` (an integer Unix time in seconds),
a nonempty `iss`, and a nonempty `aud` string or array of strings. Verification
also checks `nbf` and rejects future `iat` values when present. No clock-skew
leeway is applied. Signing allows a future `nbf`; verification rejects the
token until that time. Expired tokens and unsupported critical header extensions
are rejected. JWT input/claims text is capped at 1 MiB.

Pin the expected algorithm, issuer, audience, and trusted key in application
configuration. Do not select them from unverified token contents. HS256 requires
at least 32 bytes of high-entropy secret material and rejects PEM key text;
RS256 signs with a PEM private key and verifies with a PEM public key. For a
local HS256 example, generate a secret with `openssl rand -hex 32 > jwt-secret.txt`
and run [examples/jwt/Application.ube](../examples/jwt/Application.ube). Keep the file private and out of
version control.

JWTs here are signed, **not encrypted**. Their payload is readable. Verification
does not perform token revocation checks, discover signing keys, or implement
OpenID Connect. JWKS fetching, encrypted JWTs, and additional algorithms are not
implemented. See [RFC 7519](https://www.rfc-editor.org/rfc/rfc7519.html).

## OAuth 2.0 client

Create one client before registering routes and reuse it for authorization and
callbacks:

```ube
final client = OAuth2.create(
    'https://provider.example/authorize',
    'https://provider.example/token',
    'your-client-id',
    '',
    'http://127.0.0.1:8080/callback'
)
server.get('/authorize', request -> Response.redirect(client.authorize(request.session(), 'read')))
```

The five `create` arguments are the authorization endpoint, token endpoint,
client ID, client secret, and **exact registered redirect URI**, in that order.
The redirect URI is preserved verbatim in both protocol requests. Empty secret
selects a public client; a nonempty secret uses `client_secret_basic`, with
form-encoded credentials. Configure endpoints yourself from a trusted provider;
provider discovery is not performed. URLs must use HTTPS except for loopback
HTTP (`localhost`, `127.0.0.1`, or `[::1]`) for local development. Credentials in
URLs and URL fragments are rejected. Authorization endpoint URLs may contain
provider options such as `prompt=consent`, but not reserved OAuth parameters.

| Call | Returns | Behavior |
| --- | --- | --- |
| `OAuth2.create(authUrl, tokenUrl, clientId, clientSecret, redirectUri)` | `OAuth2` | Creates an immutable client configuration. |
| `client.authorize(session, scopes)` | `string` | Builds an authorization URL and saves a five-minute, session-bound transaction with S256 PKCE. Scopes are space-separated; empty scopes omit the parameter. |
| `client.exchange(session, code, state)` | `OAuth2Token` | Validates and consumes the transaction, then exchanges the code at the configured token endpoint. |
| `client.refresh(refreshToken)` | `OAuth2Token` | Requests a new token using the refresh-token grant. |
| `client.close()` | `void` | Releases the client and removes its pending transactions. |
| `token.accessToken()` | `string` | Returns the access token. |
| `token.tokenType()` | `string` | Returns the provider's Bearer token type. Other token types are rejected. |
| `token.refreshToken()` | `string?` | Returns the refresh token when supplied; refresh responses that omit it retain the supplied refresh token. |
| `token.expiresIn()` | `long` | Returns the lifetime in seconds reported by the provider, or `-1` if absent. This is not a live countdown. |
| `token.scope()` | `string?` | Returns the scope string when supplied. |
| `token.close()` | `void` | Releases the token handle. |

On the callback, read `request.query('code')` and `request.query('state')` and
pass them with `request.session()` to `exchange`. Duplicate query parameters
raise `GenericError`. The callback must carry the original session cookie;
unknown sessions, mismatched clients, expired transactions, and reused state
are rejected before contacting the token endpoint. A matching transaction is
consumed before network exchange, so a failed exchange requires starting again.
Do not regenerate/destroy the session between `authorize` and `exchange`.
After a successful exchange, regenerate it before attaching authenticated state
or token credentials. Regeneration preserves session data.

`Response.redirect(url)` produces HTTP 302 with a `Location` header across
HTTP/1.1, HTTP/2, and HTTP/3. OAuth callbacks use the same session and query APIs
on each transport.
`request.header(name)` reads the actual case-insensitive HTTP header, returning
an empty string when absent and rejecting duplicate headers. `hasBearerToken()`
checks Bearer header syntax; it does **not** validate a token.

See [examples/oauth2/Application.ube](../examples/oauth2/Application.ube) for a complete callback with
error handling, session rotation, and token cleanup. Replace its placeholder
provider configuration before running. Store access and refresh tokens on the
server, not in browser-readable session cookies. The current session store is
in memory and expires after 30 minutes; see [sessions](sessions.md).

Token requests verify TLS certificates, do not follow redirects, have a
15-second total timeout and 5-second connect timeout, and limit responses to
1 MiB. Calls are synchronous and block the calling Ube thread while the token
endpoint responds. Provider error descriptions, tokens, and secrets are not
included in runtime error messages. Runtime failures raise `GenericError`.
Release claims/token handles when finished, and close clients when no longer
needed. Double-close is harmless; other operations on closed handles fail.

This implements the authorization-code grant with S256 PKCE, query-mode
callbacks, and refresh tokens. It does not implement an authorization server,
OpenID Connect login/ID-token validation, implicit/password/client-credentials
grants, `client_secret_post`, discovery, revocation, or introspection. The design
uses the authorization-code and PKCE guidance in
[RFC 9700](https://www.rfc-editor.org/rfc/rfc9700.html) and
[RFC 7636](https://www.rfc-editor.org/rfc/rfc7636.html).
