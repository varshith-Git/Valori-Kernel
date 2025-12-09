use serde::{Serialize, Deserialize};
use crate::config::{IndexKind, QuantizationKind};
use std::fs::File;
use std::io::Write;
use std::path::Path;
use crc32fast::Hasher;

const MAGIC: u32 = 0x56414C4F; // VALO
const SCHEMA_VERSION: u32 = 2;

#[derive(Serialize, Deserialize, Debug)]
pub struct SnapshotMeta {
    pub version: u32,       
    pub timestamp: u64,     
    pub kernel_len: u64,
    pub metadata_len: u64, // Length of MetadataStore blob
    pub index_len: u64,
    pub index_kind: IndexKind,
    pub quant_kind: QuantizationKind,
}

pub struct SnapshotManager;

impl SnapshotManager {
    pub fn save(
        path: &Path,
        kernel_data: &[u8],
        metadata_data: &[u8], // MetadataStore blob
        meta: &mut SnapshotMeta, // Mutable to update lengths
        index_data: &[u8],
    ) -> Result<(), std::io::Error> {
        let tmp_path = path.with_extension("tmp");
        
        // Update lengths
        meta.kernel_len = kernel_data.len() as u64;
        meta.metadata_len = metadata_data.len() as u64;
        meta.index_len = index_data.len() as u64;

        {
            let mut file = File::create(&tmp_path)?;
            let mut hasher = Hasher::new();

            // Serialize Meta (Header)
            let meta_json = serde_json::to_vec(meta)?;
            let meta_len = meta_json.len() as u32;

            // Write Helper
            let mut write_chunk = |data: &[u8]| -> std::io::Result<()> {
                file.write_all(data)?;
                hasher.update(data);
                Ok(())
            };

            // [MAGIC][VER][META_LEN]
            write_chunk(&MAGIC.to_le_bytes())?;
            write_chunk(&SCHEMA_VERSION.to_le_bytes())?;
            write_chunk(&meta_len.to_le_bytes())?;
            
            // [META_JSON]
            write_chunk(&meta_json)?;
            
            // [KERNEL]
            write_chunk(kernel_data)?;
            
            // [METADATA_STORE]
            write_chunk(metadata_data)?;

            // [INDEX]
            write_chunk(index_data)?;

            // [CRC]
            let checksum = hasher.finalize();
            file.write_all(&checksum.to_le_bytes())?; 
        }

        // ROTATION LOGIC: Keep one previous version
        if path.exists() {
            let prev_path = path.with_extension("bin.prev");
            let _ = std::fs::rename(path, prev_path); // Ignore error if rename fails (e.g. permission)
        }

        std::fs::rename(tmp_path, path)?;
        Ok(())
    }

    pub fn parse(buffer: &[u8]) -> Result<(SnapshotMeta, Vec<u8>, Vec<u8>, Vec<u8>), Box<dyn std::error::Error + Send + Sync>> {
        if buffer.len() < 16 { return Err("Snapshot too short".into()); }

        // Check Trailer
        let split_idx = buffer.len() - 4;
        let (content, trailer) = buffer.split_at(split_idx);
        let stored_crc = u32::from_le_bytes(trailer.try_into().unwrap());
        
        let mut hasher = Hasher::new();
        hasher.update(content);
        if hasher.finalize() != stored_crc {
            return Err("Checksum mismatch".into());
        }

        // Parse Header
        let magic = u32::from_le_bytes(content[0..4].try_into().unwrap());
        if magic != MAGIC { return Err("Invalid MAGIC".into()); }
        
        let version = u32::from_le_bytes(content[4..8].try_into().unwrap());
        if version != SCHEMA_VERSION { return Err("Version mismatch".into()); }

        let meta_len = u32::from_le_bytes(content[8..12].try_into().unwrap()) as usize;
        let meta_end = 12 + meta_len;
        
        // BOUNDS CHECK 1: Meta
        if content.len() < meta_end {
             return Err("Truncated metadata".into());
        }

        // Parse Meta
        let meta: SnapshotMeta = serde_json::from_slice(&content[12..meta_end])?;

        // Extract Blobs
        let k_len = meta.kernel_len as usize;
        let m_len = meta.metadata_len as usize;
        let i_len = meta.index_len as usize;

        // BOUNDS CHECK 2: Body consistency
        let remaining_len = content.len() - meta_end;
        let expected_len = k_len + m_len + i_len;
        
        if remaining_len != expected_len {
            return Err(format!("Snapshot corrupted: Meta claims {} bytes, found {}", expected_len, remaining_len).into());
        }

        let k_start = meta_end;
        let k_end = k_start + k_len;
        
        let m_start = k_end;
        let m_end = m_start + m_len;
        
        let i_start = m_end;
        let i_end = i_start + i_len;
        
        // Redundant but safe final check
        if i_end > content.len() { return Err("Truncated body".into()); }

        let k_data = content[k_start..k_end].to_vec();
        let m_data = content[m_start..m_end].to_vec();
        let i_data = content[i_start..i_end].to_vec();

        Ok((meta, k_data, m_data, i_data))
    }
}
