// Copyright (c) 2025 Varshith Gudur. Dual-licensed under MIT OR Apache-2.0.
//
// tonic codegen for proto/raft.proto. protoc is vendored
// (protoc-bin-vendored) so neither developers nor CI need a system install.

fn main() {
    std::env::set_var(
        "PROTOC",
        protoc_bin_vendored::protoc_bin_path().expect("vendored protoc available"),
    );
    tonic_build::compile_protos("proto/raft.proto").expect("raft.proto compiles");
    println!("cargo:rerun-if-changed=proto/raft.proto");
}
