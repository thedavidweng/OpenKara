# Streaming Source provenance and transfer rights

Audit date: 2026-10-20. Scope: #552 at application base
`d008d8e5ebf66df29c1bedfebd22f5873221f74c`.
This is a rights inventory, not legal advice or permission to relicense.

## Observed history

`git log --follow` and `git blame --line-porcelain` identify only the author
`Davy` for `sidecars/netease/worker.mjs` and `protocol.mjs` at this base:

| Revision                                                | Material                                                                                                  | License evidence                                                                                                                                           |
| ------------------------------------------------------- | --------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `3ae6873b83319e0b227f3074cf257ba54447d3f0` (2026-09-28) | Initial worker and seven upstream operations; cookie validation, China Client Address, response filtering | `git show 3ae6873b:LICENSE` is Apache-2.0. Preserve the original license and applicable notices for reused material.                                       |
| `d008d8e5ebf66df29c1bedfebd22f5873221f74c` (2026-10-19) | Correlated concurrent requests and direct-call China Client Address handling                              | Root LICENSE is AGPL-3.0-only. No additional transfer grant is established here.                                                                           |
| `b7cd5c2446d904f40fc75548545f0a0cf8f76d44` (2026-10-09) | License change, CLA, LICENSING.md, historical exceptions                                                  | Application AGPL; historical Apache grants remain. CLA states that previously merged contributions need an expressly identified separate signed agreement. |

Author metadata attributes revisions. It does not establish exclusive copyright,
employer clearance, a signed grant, or the absence of copied upstream material.
No signed permission for these exact transfer revisions was supplied or found
in the inspected history, CLA text, and #476 discussion. Do not infer one from
the existence of CLA.md, the maintainer role, or an author name.

The fixture and protocol added by #552 are new application-repository materials.
They do not import or copy the old worker or API modules. They remain
AGPL-3.0-only under the root LICENSE. No MIT exception is declared by this work.

## Production package license

The maintainer authorized an AGPL-3.0-only NetEase publication under #556 when
the transfer rights cannot support MIT. The production implementation lives in
[openkara-sources](https://github.com/thedavidweng/openkara-sources). It ports
the source behavior from application revision
`a933794fb4b7a450e4a2976c87ed23ba83c9d4ad`. The package keeps AGPL terms and
historical Apache notices. Its dependency and file-rights inventories must
accompany the published archive. This choice does not establish an MIT grant.

## Rights checklist before MIT publication

- [x] Identify exact worker/protocol revisions and author attribution above.
- [x] Identify original Apache and later AGPL terms; preserve [NOTICE](../../NOTICE)
      and [Apache license](../../LICENSES/Apache-2.0.txt) for historical reuse.
- [x] Review [CLA.md](../../CLA.md), especially its temporal scope and sections
      3 and 5. It cannot retrospectively authorize every transfer.
- [x] Keep the executable fixture and protocol under application AGPL pending
      actual rights evidence. Do not label a package MIT merely because stdio is used.
- [ ] Obtain a recorded rights-holder declaration for the exact files/revisions
      to transfer, including originality or identified third-party portions,
      employer rights where relevant, and an explicit MIT grant. A signed CLA whose
      actual scope covers those contributions is another route. Record the signer,
      covered revisions, agreement text/revision, and durable evidence URL.
- [ ] At NetEase package publication, inventory the complete locked production
      dependency closure and ship each required license/notice. The historical
      `sidecars/netease/package-lock.json` at the audited revision pins
      `@neteasecloudmusicapienhanced/api` 4.40.1 and transitive packages. That lockfile
      records integrity, not copyright clearance. Do not relabel dependencies MIT.
- [ ] Bind a final file-level rights manifest and attribution bundle to the
      released archive digest in `openkara-sources`. Review any later copied Rust
      conversion/refusal code separately; it is not cleared by this worker audit.

Unchecked items are publication prerequisites, not missing fixture operations.
AGPL development and the independent process contract can proceed now. The
application license and third-party terms remain unchanged. A transport boundary
alone does not determine whether works are combined or derived.

## Reproduce the inventory

```sh
git log --format=fuller --follow -- sidecars/netease/worker.mjs
git log --format=fuller --follow -- sidecars/netease/protocol.mjs
git blame --line-porcelain sidecars/netease/worker.mjs
git blame --line-porcelain sidecars/netease/protocol.mjs
git show 3ae6873b:LICENSE
git show b7cd5c24:CLA.md
```
