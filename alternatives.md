# Alternatives to revisit

Deliberate "good enough for now" technology choices that are worth
re-evaluating later — recorded with the **trigger** that should prompt the
re-evaluation, so they aren't silently forgotten. This is not a bug backlog and
not a list of everything we rejected; it's the short list of *close calls* we
expect to come back to.

Format per entry: what we chose, the alternative, why not now, and the signal
that says "now."

---

## Serialization — bitcode (vs. postcard)

- **Chosen:** `postcard` (serde, documented and version-stable wire spec).
- **Alternative:** `bitcode` — bit-packed, materially smaller and faster on the
  wire; also serde-compatible.
- **Why not now:** the host and the compositor are separate binaries that may
  differ in version across the network, so a *stable, documented* wire format
  matters more than raw size at v0. bitcode's binary layout is not guaranteed
  stable across its own major versions.
- **Revisit when:** wire size or (de)serialisation throughput shows up as a
  *measured* bottleneck, and we can either pin both peers to one bitcode version
  or it ships a wire-stability guarantee.

(Background: we left `bincode` because it is unmaintained —
[RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141).)

---

## Content hashing — BLAKE3 (vs. SHA-256)

- **Chosen:** SHA-256 (`sha2`) for `SurfaceId` and `session_id`. Ubiquitous,
  hardware-accelerated (SHA-NI) on modern CPUs, and already written into the
  persistence spec, the XSD examples, and the reference oracle.
- **Alternative:** BLAKE3 — a parallel tree hash with built-in incremental
  hashing and **verified streaming** (Bao: verify *slices* of a large blob
  against the root without the whole thing). Same 32-byte output, so `SurfaceId`
  stays `[u8; 32]`.
- **Why not now:** at v0 we hash only tiny inputs off the hot path — a short
  seed string per surface and a few hundred bytes of session XML per checkpoint.
  BLAKE3's parallelism has nothing to chew on at that size, and SHA-NI SHA-256
  is right there with it. No measurable win, and switching would churn the
  already-merged spec / XSDs / oracle for nothing.
- **Revisit when:** hashing moves onto the line-rate path — content-addressing
  **frames** or streaming large session / commit-log content with incremental
  verification. That is where BLAKE3's tree hash and Bao earn their keep.
- **Safe to defer:** content ids are algorithm-namespaced (`sha256:<hex>`), so
  adding `blake3:<hex>` later is non-breaking — `parse`/`verify` dispatch on the
  prefix, old SHA-256 sessions keep resolving, and the persistence Galois laws
  (about the maps, not the hash) are unaffected.
