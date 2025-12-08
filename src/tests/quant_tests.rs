#[cfg(test)]
mod tests {
    use crate::quant::{Quantizer, NoQuantizer};
    use crate::types::vector::FxpVector;
    use crate::types::scalar::FxpScalar;
    
    // Helper to create a dummy vector
    fn make_vec<const D: usize>(val: i32) -> FxpVector<D> {
        let mut v = FxpVector::<D> { data: [FxpScalar(0); D] };
        for i in 0..D {
            v.data[i] = FxpScalar(val + i as i32);
        }
        v
    }

    #[test]
    fn test_no_quantizer_identity() {
        let q = NoQuantizer;
        let v_original = make_vec::<16>(100);
        
        let code = q.encode(&v_original);
        let v_decoded = q.decode(&code);
        
        assert_eq!(v_original, code, "NoQuantizer code must match input");
        assert_eq!(v_original, v_decoded, "NoQuantizer decode must match input");
    }

    #[test]
    fn test_no_quantizer_determinism() {
        let q = NoQuantizer;
        let v = make_vec::<16>(12345);
        
        for _ in 0..10 {
            let code = q.encode(&v);
            let decoded = q.decode(&code);
            assert_eq!(v, decoded);
        }
    }
}
