# Streaming Source host API

The sandboxed host API of a protocol 2.0 Streaming Source. The embedded
QuickJS engine gives each source exactly one global `openkara` object plus
`setTimeout` and `clearTimeout`. There is no other capability: no
filesystem, no process, no sockets, no module loader, no `eval`, and no
`Function` constructor. See the [protocol reference](streaming-source-protocol.md)
for the package format and the method contract, and the
[source host](source-host.md) for installation and media boundaries.

Binary inputs accept a `Uint8Array`, an `ArrayBuffer`, or a string (encoded
as UTF-8). Binary outputs are `Uint8Array`. Invalid crypto or encoding
input throws a `TypeError`.

## openkara.request

`openkara.request({url, method?, headers?, body?, responseType?})` returns
a promise of `{status, url, headers, setCookies, body}`.

- `url` must be HTTPS, and its exact origin must appear in the manifest's
  `resource_origins` or `media_origins`. No wildcard, subdomain, port, or
  userinfo variation is accepted.
- `method` is `GET` (default), `HEAD`, `POST`, `PUT`, `PATCH`, or `DELETE`.
- `responseType` is `text` (default), `json`, or `bytes`. A `text` body is
  lossy UTF-8 decoded; a `json` body is parsed; a `bytes` body is a
  `Uint8Array`. Response headers exclude `Set-Cookie`; those values arrive
  separately in the `setCookies` array. A repeated response header joins
  its values with `, `. Any HTTP status resolves; an unsuccessful status is
  not a rejection.
- `body` accepts a binary input and is at most 1 MiB. A response is at
  most 8 MiB.
- A source may not set `Host`, `Content-Length`, `Transfer-Encoding`,
  `Connection`, `Keep-Alive`, `Upgrade`, `TE`, `Trailer`, `Expect`, or any
  `Proxy-*` or `Sec-*` header. `Referer` and `Origin` values must be HTTPS
  URLs without userinfo. Header values must not contain CR, LF, or NUL, and
  header names must not repeat.
- The host follows at most 5 redirects. It checks the HTTPS scheme, the
  declared origin, and a public DNS destination at every hop. A redirect to
  an undeclared origin or a private address fails. A cross-origin redirect
  drops the `Cookie` and `Authorization` headers. A 303 redirect, or a 301
  or 302 redirect of a POST, becomes a GET without a body.
- At most 8 requests per source are in flight. More reject with `busy`.

A rejection is an `Error` whose `code` is the host error code:
`invalid_resource` (URL, origin, header, or size rule), `network`
(connection, DNS, redirect limit), `busy` (concurrency), or `cancelled`
(source stopped or work cancelled). A source that rethrows such an error
returns the matching protocol error, so catch, inspect `code`, and rethrow
instead of inventing a new error.

Each source has its own cookie jar. The host stores `Set-Cookie` response
values and sends the matching cookies on later requests from the same
source. An explicit `Cookie` header goes before the stored cookies. The jar
lives in memory for the running engine instance: it ends when the source
stops, and `auth.sign_out` discards it. A package that needs durable
cookies stores them through `session.restore` credentials or the key-value
store.

## openkara.crypto

- `digest(algorithm, data)` and `hmac(algorithm, key, data)` with
  `algorithm` one of `md5`, `sha1`, `sha256`, `sha384`, or `sha512`.
- `aesEncrypt(options, data)` and `aesDecrypt(options, data)` with
  `options` `{mode, key, iv?, padding?, aad?}`. `mode` is `ecb`, `cbc`,
  `ctr` (128-bit big-endian counter), or `gcm` (12-byte IV, 16-byte tag
  appended to the ciphertext; 128- or 256-bit keys). `padding` is `pkcs7`
  (default) or `none` for ECB and CBC. `aad` applies to GCM.
- `rsaPublic(key, data)` computes the raw RSA public operation without
  padding. `key` is either `{pem}` (a `PUBLIC KEY` or `RSA PUBLIC KEY`
  document) or `{modulus, exponent}` as big-endian bytes. Moduli are 512 to
  8192 bits. The output is left-padded to the modulus length.
- `randomBytes(length)` returns at most 64 KiB of secure random bytes.

## openkara.encoding

- `base64Encode(data, {urlSafe?, padding?})`: standard alphabet by default;
  `urlSafe` selects the URL-safe alphabet; `padding` defaults to true.
- `base64Decode(text)`: accepts the standard or URL-safe alphabet, with or
  without padding.
- `hexEncode(data)` and `hexDecode(text)`.
- `utf8Encode(text)` returns a `Uint8Array`; `utf8Decode(data)` returns a
  lossy UTF-8 string.

## openkara.storage

A small key-value store for source state that is not Streaming Credentials.

- `get(key)` returns the string value or `undefined`.
- `set(key, value)`, `remove(key)`, and `clear()` write through to disk.
- `keys()` returns all keys.

Keys are strings of 1 to 256 bytes. Values are strings. Keys plus values
are capped at 1 MiB per package. A write over the cap throws and changes
nothing. Storage persists at
`<data_root>/streaming-sources/source-storage/<source_id>.json`, bound to
the package's owner binding: another package that later takes the same
source id starts with an empty store. Uninstall leaves the file. Storage
errors throw an `Error` without a protocol `code`; do not rethrow them as
domain results.

## openkara.log

`openkara.log.debug(message, fields?)` and the `info`, `warn`, and `error`
variants write a structured record to the host log with target
`openkara::streaming_source` and the source id. The message is a string;
any other value is JSON-encoded. `fields` is a JSON-compatible object.

Every record is redacted before it leaves the engine. The host replaces
with `[redacted]`: credential values seen in `session.restore` params and
sign-in results, `Cookie` and `Authorization` request header values,
received cookie values, values under sensitive field names (a name that
contains `authorization`, `cookie`, `credential`, `csrf`, `key`, `pass`,
`secret`, `session`, `sign`, or `token`), and URL queries and fragments.
Messages are cut at 1,024 characters and fields at 4,096. Each source may
write 100 records a minute; later records in that minute are dropped after
one warning record.

## setTimeout and clearTimeout

`setTimeout(callback, delay)` returns a numeric id; `clearTimeout(id)`
cancels it. A non-positive or non-finite delay becomes zero. The delay is
capped at 60 seconds. At most 256 timers are pending per source; more throw
a `RangeError`. Timers run only while the source has a call in progress:
a source cannot schedule work that runs on its own between calls, and
pending timers end when the engine instance stops.

## What a source cannot reach

`session.restore` receives only the credentials saved for the package's own
owner binding. A source has no API to read the host keychain, another
source's credentials, cookies, storage, or logs, the user's library, or
local files. `openkara.request` reaches only the declared origins over
public HTTPS. Media downloads, artwork loads, authentication windows, and
library writes stay with the host.
