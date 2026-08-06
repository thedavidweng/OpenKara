# Domain Docs

How engineering skills should consume this repo's domain documentation.

## Before exploring, read these

- **`CONTEXT.md`** at the repo root (domain glossary and preferred terms)
- **`docs/adr/`** — ADRs that touch the area you will change

If a file is missing, proceed silently. Do not create domain docs up front.
`/domain-modeling` and related skills create them when terms or decisions are
actually resolved.

## Layout

Single-context:

```
/
├── CONTEXT.md
├── docs/adr/
└── src/
```

There is no `CONTEXT-MAP.md`. Do not invent multi-context layout unless the
repo gains monorepo packages with separate domain files.

## Vocabulary

When output names a domain concept (issue title, proposal, test name), use the
term as defined in `CONTEXT.md`. Do not use synonyms the glossary avoids.

If the concept is not in the glossary, either the project does not use that
language (reconsider) or there is a real gap (note it for domain modeling).

## ADR conflicts

If output contradicts an existing ADR, say so explicitly. Do not silently
override the ADR.
