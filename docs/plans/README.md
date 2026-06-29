# Plans

Engineering plans for Simulboot: how we close the gaps and in what order.

This folder sits between the other two kinds of doc:

| Document | Question it answers |
| --- | --- |
| `docs/denotation.md` | **Why** — what the substrate *means* (the spec) |
| `SKELETONS.md` (repo root) | **What** — which placeholders exist right now |
| `docs/plans/` (here) | **How / when** — the route from a skeleton to working code |

A plan picks up one or more entries from `SKELETONS.md`, says how to build them,
and is deleted (or marked `Done`) once they land — at which point the matching
`SKELETONS.md` entries go too. Plans are forward-looking and disposable; the
denotation is not.

## Conventions

Riffs on the ADR/RFC convention — small, numbered, status-bearing files.

- **Filename:** `NNNN-slug.md`, zero-padded, allocated in order. `0000` is the
  rolling roadmap.
- **Header:** every plan starts with the block in the template below — `Status`,
  `Skeletons`, `Updated`, and a one-line summary.
- **Scope:** one plan = one coherent piece of work. If it sprawls, split it and
  cross-link.
- **Keep it honest:** record open questions and risks, not just the happy path.

### Status legend

- `Draft` — being written; not agreed.
- `Accepted` — agreed; not started.
- `In progress` — actively being built.
- `Done` — landed; delete the plan and its `SKELETONS.md` entries on the next
  cleanup pass.
- `Superseded by NNNN` — replaced; keep briefly for the trail, then remove.

### Template

```markdown
# NNNN — Title

- **Status:** Draft
- **Skeletons:** <links to the SKELETONS.md entries this closes>
- **Updated:** YYYY-MM-DD
- **Summary:** one sentence.

## Goal
What "done" looks like, in observable terms.

## Approach
The plan of record. Phased if useful.

## Open questions
The things not yet decided.

## Risks
What could make this harder than it looks.

## Acceptance
The check that says it's finished (a test, a demo step, a metric).
```

## Index

- [`0000-roadmap.md`](0000-roadmap.md) — the overall v0→v1 arc.
- [`0001-capture-backends.md`](0001-capture-backends.md) — real per-OS
  `CaptureSource` backends (the top blocking skeleton).
