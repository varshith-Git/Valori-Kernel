use std::fs::File;
use std::io::{self, BufReader};
use byteorder::{ReadBytesExt, LittleEndian};

pub struct IvecsLoader {
    reader: BufReader<File>,
}

impl IvecsLoader {
    pub fn new(path: &str) -> io::Result<Self> {
        let f = File::open(path)?;
        Ok(Self { reader: BufReader::new(f) })
    }
}

impl Iterator for IvecsLoader {
    type Item = Vec<u32>; // The ground truth IDs

    fn next(&mut self) -> Option<Self::Item> {
        // Format: [dim (4 bytes)] [id 1] [id 2] ...
        let dim = match self.reader.read_i32::<LittleEndian>() {
            Ok(d) => d as usize,
            Err(_) => return None,
        };

        let mut ids = vec![0u32; dim];
        // Read the integers
        if let Err(_) = self.reader.read_u32_into::<LittleEndian>(&mut ids) {
            return None;
        }
        Some(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_ivecs(path: &str, rows: &[Vec<u32>]) {
        let mut f = std::fs::File::create(path).unwrap();
        for row in rows {
            let dim = row.len() as i32;
            f.write_all(&dim.to_le_bytes()).unwrap();
            for &v in row {
                f.write_all(&v.to_le_bytes()).unwrap();
            }
        }
    }

    #[test]
    fn reads_single_row() {
        let path = "/tmp/test_ivecs_single.ivecs";
        write_ivecs(path, &[vec![10u32, 20, 30]]);
        let mut loader = IvecsLoader::new(path).unwrap();
        assert_eq!(loader.next(), Some(vec![10, 20, 30]));
        assert_eq!(loader.next(), None);
    }

    #[test]
    fn reads_multiple_rows() {
        let path = "/tmp/test_ivecs_multi.ivecs";
        write_ivecs(path, &[vec![1u32, 2], vec![3u32, 4, 5]]);
        let rows: Vec<Vec<u32>> = IvecsLoader::new(path).unwrap().collect();
        assert_eq!(rows, vec![vec![1, 2], vec![3, 4, 5]]);
    }

    #[test]
    fn empty_file_returns_none() {
        let path = "/tmp/test_ivecs_empty.ivecs";
        std::fs::write(path, b"").unwrap();
        let mut loader = IvecsLoader::new(path).unwrap();
        assert_eq!(loader.next(), None);
    }

    #[test]
    fn zero_dim_row_is_empty_vec() {
        let path = "/tmp/test_ivecs_zerodim.ivecs";
        let dim: i32 = 0;
        std::fs::write(path, dim.to_le_bytes()).unwrap();
        let mut loader = IvecsLoader::new(path).unwrap();
        assert_eq!(loader.next(), Some(vec![]));
        assert_eq!(loader.next(), None);
    }
}
