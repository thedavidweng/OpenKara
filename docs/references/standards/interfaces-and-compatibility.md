# Interfaces and Compatibility

Use this profile for IPC, remote sync, authentication, public HTTP APIs, event
interfaces, schemas, and compatibility decisions.

## Authorities

| Authority                                                                                                                                                                                         | Use in OpenKara                                                           |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| [HTTP Semantics, RFC 9110](https://www.rfc-editor.org/rfc/rfc9110) and [WebDAV, RFC 4918](https://www.rfc-editor.org/rfc/rfc4918)                                                                 | Remote library methods, conditional writes, status handling, and recovery |
| [OAuth 2.0 Security BCP, RFC 9700](https://www.rfc-editor.org/rfc/rfc9700) and [PKCE, RFC 7636](https://www.rfc-editor.org/rfc/rfc7636)                                                           | Public desktop OAuth redirect, PKCE, token use, and reauthorization       |
| [Semantic Versioning 2.0.0](https://semver.org/spec/v2.0.0.html)                                                                                                                                  | Versioned public contracts, plugins, SDKs, and compatibility commitments  |
| [OpenAPI 3.1.1](https://spec.openapis.org/oas/v3.1.1.html), [JSON Schema 2020-12](https://json-schema.org/draft/2020-12/json-schema-core), and [RFC 9457](https://www.rfc-editor.org/rfc/rfc9457) | A public HTTP API, its schemas, and its problem responses                 |
| [AsyncAPI 3.0.0](https://www.asyncapi.com/docs/reference/specification/v3.0.0) and [CloudEvents 1.0](https://github.com/cloudevents/spec/blob/main/cloudevents/spec.md)                           | A public event or message interface                                       |

## Constraints

OpenKara currently exposes Tauri IPC contracts and remote WebDAV providers.
The matching contract document changes with any public IPC command, payload,
event, or source enum.

Use OpenAPI, JSON Schema, and RFC 9457 only when a change exposes a public HTTP
API. Use AsyncAPI and CloudEvents only when a change exposes a public event
interface. A change that introduces either surface adds an ADR before it
commits the contract.

Use Semantic Versioning when the project makes a versioned public compatibility
commitment. Define a migration and a compatibility window before a breaking
change.

## Required evidence

- Contract and provider integration tests for changed IPC, WebDAV, or OAuth
  behavior.
- Schema and problem-response tests for a public HTTP API.
- Compatibility, migration, and consumer evidence for a breaking change.
