# ADR 0010 — Library sort is mixed-script, locale-aware, case-insensitive

Date: 2026-07-19
Status: accepted

## Context

The library contains songs with titles in Latin, CJK, and mixed scripts.
A naive ASCII sort puts CJK titles after all Latin titles and is
case-sensitive, which is neither useful nor expected. A pure Unicode
collation sort can reorder familiar Latin titles in surprising ways.
Users expect a single stable ordering that groups Latin titles
alphabetically (case-insensitive) and places CJK titles in a predictable
position relative to Latin.

## Decision

The library sort uses a mixed-script, locale-aware, case-insensitive
comparator. Latin-script titles sort case-insensitively. CJK titles sort
after Latin titles in a stable, deterministic order. The comparator is
implemented in `song-sort` (frontend) and mirrored in the Rust library
query sort, so the on-disk query result order and the in-memory frontend
sort agree.

The sort is the single source of truth for the alphabet rail grouping
and the default library list order. The DSU (decorate-sort-undecorate)
pattern is used so the expensive per-row key extraction runs once per
sort, not once per comparison.

## Consequences

- Changing the sort comparator must update both the frontend
  `song-sort` and the Rust library query sort in the same change, or
  the list will reorder when the frontend re-sorts a backend result.
- The alphabet rail's bucket boundaries depend on this comparator; a
  new comparator must produce bucket boundaries that match the rail's
  index or the rail will misgroup.
- The DSU key extraction is load-bearing for performance on large
  libraries; reverting to a per-comparison key extraction is a
  regression on libraries with thousands of songs.
