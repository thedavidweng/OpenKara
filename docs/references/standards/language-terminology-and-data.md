# Language, Terminology, and Data

Use this profile for user-visible text, locale behavior, terms, timestamps,
units, serialized values, and logs.

## Authorities

| Authority                                                                                                                    | Use in OpenKara                                                                                      |
| ---------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| [ISO 24495-1:2023](https://www.iso.org/standard/78907.html)                                                                  | Plain language for labels, help, errors, confirmations, and recovery text                            |
| ASD-STE100 Simplified English                                                                                                | Repository technical prose, as defined in `docs/agents/engineering.md`                               |
| [ISO 704:2022](https://www.iso.org/standard/79077.html) and [ISO 1087:2019](https://www.iso.org/standard/62330.html)         | One concept, one preferred term, stable definitions, controlled synonyms, and terminology vocabulary |
| [BCP 47 / RFC 5646](https://www.rfc-editor.org/rfc/rfc5646)                                                                  | Locale tags in files, HTML, settings, and public data                                                |
| [Unicode LDML / TR35](https://unicode.org/reports/tr35/) and platform `Intl`                                                 | Display names, collation, dates, times, numbers, and plural rules                                    |
| [RFC 3339](https://www.rfc-editor.org/rfc/rfc3339) and [ISO 8601-1:2019](https://www.iso.org/standard/70907.html)            | Interchange timestamps, dates, and machine-readable time values                                      |
| [ISO 80000](https://www.iso.org/standard/76921.html) and the [SI Brochure](https://www.bipm.org/en/publications/si-brochure) | Quantity names, symbols, and explicit base-unit fields                                               |

## Constraints

- Use one preferred term for one concept in each locale. Define a cross-surface
  term before using a new synonym for it.
- Use concise action labels. Error text states the problem, current state, and
  safe recovery action.
- Use canonical BCP 47 tags. Keep the document `lang` value synchronized with
  the selected locale.
- Use `Intl` or locale data for human display. Use RFC 3339 for public wire
  timestamps. Do not use localized display strings as interchange data.
- Name numeric fields with their unit, such as `duration_ms`, `size_bytes`, or
  `gain_db`. Store a defined base unit and convert only at the display edge.

## Required evidence

- Locale-key parity and canonical-tag tests for changed UI copy or locale data.
- Serialization or contract tests for changed timestamps, numeric values, and
  unit-bearing fields.
- Copy review for new user-facing messages and terminology review for a new
  cross-surface term.
