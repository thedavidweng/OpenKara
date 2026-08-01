# Remote Provider Fixture

A Node-only HTTP fixture server that mimics WebDAV, Google Drive, and Dropbox for integration tests.

## Start

```bash
node tests/support/remote-provider-fixture/server.mjs
```

Set `PORT` to override the default `9877`.

## Routes

- `/oauth/*` — shared OAuth 2.0 authorize, token, refresh, and revoke endpoints.
- `/webdav/*` — WebDAV-compatible methods with auth, sync, locks, conflicts, and faults.
- `/google-drive/v3/*` — Google Drive API v3-like list, upload, download, and update endpoints.
- `/dropbox/2/*` — Dropbox API-like list, upload, download, and continue endpoints.

## Faults

Most endpoints accept a `?fault=<id>` query parameter:

- `timeout` — never responds.
- `interrupt` — drops the connection mid-body.
- `429` — returns HTTP 429.
- `500` — returns HTTP 500.
- `conflict` — returns 409 on upload when the file already exists.
- `retry-limit` — returns 429 until the `attempt` count reaches the limit.
- `expired-creds` — returns 401 for the request.
- `revoked` — treats the token as revoked.

## OAuth

- `GET /oauth/authorize?client_id=...&redirect_uri=http://localhost:<port>/callback&state=...&response_type=code`
- `POST /oauth/token` with `grant_type=authorization_code&code=...` or `grant_type=refresh_token&refresh_token=...`
- `POST /oauth/revoke` with `token=...`

Tokens are redacted in diagnostic responses by default.
