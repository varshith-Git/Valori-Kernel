// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//! Crate smoke test. Real consensus behaviour is covered per sub-phase:
//! type config in `type_config.rs` (2.1); log store, state machine, network,
//! and turmoil partition simulations land with 2.2–2.8.

#[test]
fn crate_links_and_exports_the_type_config() {
    // The public surface every later phase builds on.
    use valori_consensus::{ClientRequest, ClientResponse, NodeId, TypeConfig, ValoriNode};
    fn _assert_type<T>() {}
    _assert_type::<ClientRequest>();
    _assert_type::<ClientResponse>();
    _assert_type::<NodeId>();
    _assert_type::<ValoriNode>();
    _assert_type::<openraft::Raft<TypeConfig>>();
}
