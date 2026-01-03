# Contributing to the Valori Kernel

Thanks for your interest in contributing.

Valori is a deterministic computation kernel designed to eliminate
substrate-driven divergence across architectures (x86 / ARM / GPU / embedded).
The project prioritizes mathematical correctness, reproducibility, and
cross-platform determinism over performance shortcuts.

## Contribution Philosophy

Contributions should:

- Improve determinism, reproducibility, or verification
- Strengthen fixed-point arithmetic correctness
- Improve cross-substrate consistency guarantees
- Add test coverage or validation proofs

Contributions that optimize performance at the cost of determinism will
not be accepted.

## PR Requirements

Pull requests must include:

- Description of the change
- Explanation of determinism impact
- Reproducible test case
- Cross-architecture notes (where applicable)

## What is Out of Scope

The following belong in private / enterprise tracks:

- forensic analysis pipelines
- incident replay engines
- audit and compliance evaluators
- production ingestion tooling

These systems depend on the kernel but are not part of the open core.

## Development Workflow

1. Open an Issue describing the proposal
2. Discuss approach and determinism implications
3. Submit PR after agreement on direction

Thank you for helping strengthen deterministic computing research.
