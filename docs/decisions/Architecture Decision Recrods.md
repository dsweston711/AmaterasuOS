# ADR-001: Use bootloader crate for initial boot path

**Date:** 2026-04-21

**Status:** Accepted

**Milestone:** v0.0.1

## Context

Need to get a kernel booting in QEMU. Two main options:
- Write kernel as a UEFI application directly (fully from scratch)
- Use the `bootloader` crate as a stepping stone

## Decision

Use `bootloader` crate initially. Plan to migrate to UEFI-direct around
milestone v0.3 once core kernel functionality is in place.

## Consequences

- **Positive:** Fast path to first visible output. Strong tutorial support
  (Phil Opp's series). Early momentum.
- **Negative:** Small boot-time overhead from bootloader stage. Dependency
  that doesn't match "from scratch" framing. Migration work later.
- **Risk:** Never actually migrating. Mitigated by tracking it as an
  explicit future milestone.

## Notes on process

Decision made after discussion with Claude comparing the two options.
The tradeoff analysis was AI-generated; the choice to prioritize
momentum over purity was mine, based on my own assessment of motivation
risk and project timeline.