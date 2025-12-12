use std::collections::HashMap;

pub trait VectorIndex {
    fn build(&mut self, records: &[(u32, Vec<f32>)]);
    fn search(&self, query: &[f32], k: usize) -> Vec<(u32, f32)>;
    fn insert(&mut self, id: u32, vec: &[f32]);
    fn snapshot(&self) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>;
    fn restore(&mut self, data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

pub struct BruteForceIndex {
    vectors: std::collections::HashMap<u32, Vec<f32>>,
}
impl BruteForceIndex {
    pub fn new() -> Self { Self { vectors: std::collections::HashMap::new() } }
}
impl VectorIndex for BruteForceIndex {
    fn build(&mut self, records: &[(u32, Vec<f32>)]) {
        for (id, vec) in records { self.vectors.insert(*id, vec.clone()); }
    }
    fn insert(&mut self, id: u32, vec: &[f32]) { self.vectors.insert(id, vec.to_vec()); }
    fn search(&self, query: &[f32], k: usize) -> Vec<(u32, f32)> {
        let mut scores: Vec<(u32, f32)> = self.vectors.iter()
            .map(|(id, vec)| { let dist = l2_distance_sq(query, vec); (*id, dist) }).collect();
        scores.sort_by(|a, b| { a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.cmp(&b.0)) });
        scores.truncate(k);
        scores
    }
    fn snapshot(&self) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> { Ok(Vec::new()) }
    fn restore(&mut self, _data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> { Ok(()) }
}

fn l2_distance_sq(a: &[f32], b: &[f32]) -> f32 { a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum() }
