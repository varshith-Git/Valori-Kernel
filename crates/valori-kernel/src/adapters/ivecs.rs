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
