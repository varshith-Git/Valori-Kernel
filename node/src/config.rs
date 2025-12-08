use std::net::SocketAddr;

pub struct NodeConfig {
    pub max_records: usize,
    pub dim: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub bind_addr: SocketAddr,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            max_records: 1024,
            dim: 16,
            max_nodes: 1024,
            max_edges: 2048,
            bind_addr: "127.0.0.1:3000".parse().unwrap(),
        }
    }
}
