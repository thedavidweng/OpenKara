# Fault Server

A Node-only HTTP fixture server that simulates download and archive faults.

## Start

```bash
node tests/support/fault-server/server.mjs
```

Set `PORT` to override the default `9876`.

## Endpoints

- `GET /health` — returns `ok`.
- `GET /manifest?checksum=<sha256>` — returns archive metadata. Responds with `409` if the supplied checksum does not match the fixture.
- `GET /download?fault=<id>` — serves the fixture archive with the selected fault.
- `GET /range` — serves the fixture with `Range` / `Accept-Ranges: bytes` support.

## Fault IDs

- `delayed` — waits before responding.
- `dropped` — writes a few bytes then closes the socket.
- `partial` — sends a truncated body.
- `incorrect-content-length` — sends a `Content-Length` larger than the body.
- `http-429` — returns HTTP 429.
- `http-500` — returns HTTP 500.
- `redirect-loop` — redirects to the same URL.
- `invalid-archive` — returns a valid TAR archive containing invalid extracted content.
- `checksum-mismatch` — serves a modified payload while claiming the original checksum.
- `none` — serves the raw fixture.

`GET /range` supports `bytes=start-end` requests and returns `206` for valid ranges or `416` for invalid ones.
