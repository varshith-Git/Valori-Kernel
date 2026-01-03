# Security Policy

The Valori Kernel is used in analytical, forensic, and compliance-adjacent
contexts. While the kernel itself does not process secrets or user data,
we treat correctness and integrity as security-relevant properties.

## Supported Versions

Only the latest tagged release is supported for security and integrity fixes.

## Reporting Vulnerabilities

If you believe you have discovered:

- a determinism-breaking behavior
- cross-substrate divergence
- arithmetic overflow edge case
- snapshot / WAL integrity weakness

please report privately rather than opening a public issue.

Contact:
varshith.gudur17@gmail.com

Please include:

- Reproduction steps
- Hardware & architecture details
- Input data characteristics

Responsible disclosure is appreciated.

## Determinism as a Security Boundary

Because Valori is used in:

- incident reconstruction
- audit trails
- replay verification pipelines

nondeterministic behavior may affect investigative outcomes.

We treat determinism failures with the same severity as security bugs.
