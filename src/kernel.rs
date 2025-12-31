use crate::error::{KernelError, Result};
use crate::types::{InsertPayload, CMD_DELETE, CMD_INSERT};
use crate::hnsw::ValoriHNSW;
use crc64fast::Digest;

#[derive(Debug, Default)]
pub struct ValoriKernel {
    pub index: ValoriHNSW,
}

impl ValoriKernel {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_count(&self) -> usize {
        if self.index.dim == 0 { 0 }
        else { self.index.vectors.len() / self.index.dim }
    }

    /// Recomputes the hash across the entire Arena and Graph.
    pub fn state_hash(&self) -> u64 {
        let mut digest = Digest::new();
        let dim = self.index.dim;
        if dim == 0 { return 0; } // Empty
        
        let count = self.record_count();
        
        // 1. Data Hash: (ExternalID + Vector + Tag + Metadata) in Internal ID order
        for i in 0..count {
            let ext_id = self.index.external_ids[i];
            digest.write(&ext_id.to_le_bytes());
            
            let start = i * dim;
            let vec_slice = &self.index.vectors[start .. start + dim];
            for val in vec_slice {
                digest.write(&val.to_le_bytes());
            }

            // TAG
            let tag = self.index.tags[i];
            digest.write(&tag.to_le_bytes());
            
            if let Some(meta) = &self.index.metadata[i] {
                digest.write(meta);
            }
        }
        
        // 2. Topology Hash
        for id in 0..count {
            digest.write(&(id as u64).to_le_bytes()); 
            
            // Check layers
            for (l_idx, layer) in self.index.layers.iter().enumerate() {
                if let Some(neighbors) = layer.get(id) {
                     digest.write(&(l_idx as u8).to_le_bytes());
                     for n_id in neighbors {
                         digest.write(&n_id.to_le_bytes());
                     }
                }
            }
        }
        
        digest.sum64()
    }
    
    pub fn apply_event(&mut self, payload: &[u8]) -> Result<()> {
        if payload.is_empty() {
             return Err(KernelError::InvalidPayloadLength { expected: 1, found: 0 });
        }

        let cmd = payload[0];
        match cmd {
            CMD_INSERT => {
                let insert = InsertPayload::from_bytes(payload)?;
                // Pass tag
                self.index.insert(insert.id, insert.values, insert.metadata, insert.tag)?;
            }
            CMD_DELETE => {
                return Err(KernelError::InvalidCommand(cmd)); 
            }
            _ => return Err(KernelError::InvalidCommand(cmd)),
        }

        Ok(())
    }

    pub fn search(&self, query: &[i32], k: usize, filter: Option<u64>) -> Result<Vec<(u64, i64)>> {
        self.index.search(query, k, filter)
    }

    /// Helper to insert a record directly (creates internal event structure if needed? No, just calls index)
    /// Note: In real app, you should go through apply_event with a payload.
    /// This helper is mainly for benchmarks or direct manipulation.
    pub fn insert(&mut self, id: u64, vec: crate::types::FixedPointVector, tag: u64) -> Result<()> {
        self.index.insert(id, vec, None, tag)
    }
    
    pub fn save_snapshot(&self) -> Result<Vec<u8>> {
        // crate::snapshot::serialize(self)
        Ok(Vec::new())
    }

    pub fn load_snapshot(_data: &[u8]) -> Result<Self> {
        // crate::snapshot::deserialize(data)
        Ok(Self::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CMD_INSERT;
    use byteorder::{LittleEndian, WriteBytesExt};

    fn create_insert_payload(id: u64, values: Vec<i32>) -> Vec<u8> {
        let dim = values.len() as u16;
        let mut wtr = Vec::new();
        wtr.write_u8(CMD_INSERT).unwrap();
        wtr.write_u64::<LittleEndian>(id).unwrap();
        wtr.write_u16::<LittleEndian>(dim).unwrap();
        for v in values {
            wtr.write_i32::<LittleEndian>(v).unwrap();
        }
        wtr
    }

    #[test]
    fn test_topological_hash_arena() {
        let mut kernel = ValoriKernel::new();
        
        let p1 = create_insert_payload(1, vec![10, 10]);
        let p2 = create_insert_payload(2, vec![12, 12]);
        let p3 = create_insert_payload(3, vec![20, 20]);
        
        kernel.apply_event(&p1).unwrap();
        kernel.apply_event(&p2).unwrap();
        kernel.apply_event(&p3).unwrap();
        
        let hash_normal = kernel.state_hash();
        
        let mut kernel2 = ValoriKernel::new();
        kernel2.apply_event(&p1).unwrap();
        kernel2.apply_event(&p2).unwrap();
        kernel2.apply_event(&p3).unwrap();
        
        let hash_req = kernel2.state_hash();
        assert_eq!(hash_normal, hash_req, "Hash must be deterministic for same insertion order");
    }

    #[test]
    fn test_search_sorting() {
        let mut kernel = ValoriKernel::new();
        
        let p1 = create_insert_payload(1, vec![10, 10]);
        kernel.apply_event(&p1).unwrap(); 

        let p2 = create_insert_payload(2, vec![0, 0]);
        kernel.apply_event(&p2).unwrap(); 

        let p3 = create_insert_payload(3, vec![5, 5]);
        kernel.apply_event(&p3).unwrap(); 

        let query = vec![0, 0];
        let results = kernel.search(&query, 3, None).unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, 2);
        assert_eq!(results[0].1, 0);

        assert_eq!(results[1].0, 3);
        assert_eq!(results[1].1, 50);

        assert_eq!(results[2].0, 1);
        assert_eq!(results[2].1, 200);
    }
}
