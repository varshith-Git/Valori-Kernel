# Phase 2.10b — Mutual TLS on the Raft Channel

**Status:** done · on `multinode`
**Roadmap:** Phase 2 (cluster mode), sub-phase 10 of 10, part b of d

## Goal

The Phase 1.6 contract, implemented: a peer that cannot present a
certificate signed by **this cluster's CA** is refused at the TLS
handshake — it never reaches the Raft layer. Two clusters sharing a
network, or an attacker with any self-signed certificate, cannot exchange
a single Raft RPC.

## Delivered

**`RaftTlsConfig`** (valori-consensus, network.rs): PEM material for both
directions — cluster CA (the single trust root), node leaf cert + key, and
the shared DNS name certs are issued for (identity is the CA signature,
not the hostname). Its `Debug` impl **redacts the private key** — TLS
config must be safe to log.

**Both directions authenticate:**
- `serve_raft_tls` — server presents its identity AND sets
  `client_ca_root`, making the TLS *mutual*: clients must present a
  cluster-CA-signed cert, not merely speak TLS.
- `ValoriNetworkFactory::with_tls` — every outbound channel carries the
  node's identity and verifies the server against the same CA.

**Boot wiring** (`cluster.rs`): `VALORI_TLS_CA` / `VALORI_TLS_CERT` /
`VALORI_TLS_KEY` (PEM file paths) + `VALORI_TLS_DOMAIN`. All three set →
mTLS everywhere; none → plaintext; **partially set → boot error** — a
half-configured TLS setup silently running plaintext would defeat the
point (same philosophy as the cluster-topology config errors).

## Validation

`tests/mtls.rs` — certificates generated in-test with rcgen (a cluster CA,
per-node leaves, and a rogue CA for the negative cases), 5× stable:

1. **`two_node_mtls_cluster_elects_and_replicates`** — election and
   replication over the encrypted channel; both kernels converge to one
   BLAKE3 hash. The SMR invariant holds under TLS.
2. **`peer_from_a_different_ca_is_refused_at_the_handshake`** — a node
   with a *valid* certificate from the *wrong* CA: `add_learner` cannot
   complete and the rogue never receives a byte of Raft state.
3. **`plaintext_client_cannot_reach_a_tls_server`** — downgrade is
   impossible, not just discouraged: a plain-HTTP gRPC client fails to
   carry a single RPC against a TLS port.

Full workspace: **251 passing, 0 failures.**

## Findings

- `ValoriNetworkFactory` grew from a unit struct to a field struct —
  every `Raft::new(…, ValoriNetworkFactory, …)` call site became
  `::default()`. Mechanical, compiler-enumerated.
- rcgen 0.13's API (`CertificateParams::signed_by(&key, &issuer_cert,
  &issuer_key)`) makes in-test CA hierarchies a ten-line affair — worth
  remembering when `valori cluster init` CLI tooling needs the same
  generation logic.

## Follow-ups

- CLI tooling (`valori cluster init` / `rotate-certs`) generating the CA
  and leaf certs operators deploy — with the `CertRotated` admin event
  (Phase 1.6 schema) emitted into the chain. Phase 3 scope.
- 2.10c: Prometheus metrics. 2.10d: partition harness + gRPC decode cap.
- Compose file: mount cert volume + TLS env vars once cluster mode is the
  default deployment.
