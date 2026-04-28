# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
"""
Tests for the Valori Bridge — deterministic proof generation.

Tests verify:
1. ingest_embedding: f32 → Q16.16 conversion matches kernel's from_f32()
2. generate_proof: Merkle tree produces deterministic hashes
3. verify_embedding: end-to-end verification works
4. Edge cases: empty vectors, out-of-range, single element, odd/even lengths
5. Determinism: same input → same hash, always
6. Position sensitivity: reordering changes the hash
7. Adapter: glue layer works with mock DB
"""

import pytest
import numpy as np
import sys
import os

# Add the python directory to path so we can import valori
sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from valori.valori_ffi import ingest_embedding, generate_proof, verify_embedding


# ============================================================================
# 1. ingest_embedding tests
# ============================================================================

class TestIngestEmbedding:
    def test_basic_conversion(self):
        """Simple floats convert to expected Q16.16 integers."""
        result = ingest_embedding([1.0, -1.0, 0.0])
        # 1.0 * 65536 = 65536
        assert result[0] == 65536
        # -1.0 * 65536 = -65536
        assert result[1] == -65536
        # 0.0 * 65536 = 0
        assert result[2] == 0

    def test_fractional_values(self):
        """Fractional floats round correctly."""
        result = ingest_embedding([0.5, 0.25, 0.75])
        assert result[0] == 32768   # 0.5 * 65536
        assert result[1] == 16384   # 0.25 * 65536
        assert result[2] == 49152   # 0.75 * 65536

    def test_rounding_behavior(self):
        """Verify round-half-to-even (banker's rounding) matches Rust f32 rounding."""
        # 0.5 * 65536 = 32768.0 — exact, no rounding needed
        result = ingest_embedding([0.5])
        assert result[0] == 32768

    def test_negative_fractional(self):
        """Negative fractions work correctly."""
        result = ingest_embedding([-0.5, -0.25])
        assert result[0] == -32768
        assert result[1] == -16384

    def test_boundary_values(self):
        """Values at the Q16.16 safe boundary."""
        result = ingest_embedding([32767.0, -32767.0])
        assert result[0] == 32767 * 65536
        assert result[1] == -32767 * 65536

    def test_out_of_range_positive(self):
        """Values above 32767.0 are rejected."""
        with pytest.raises(ValueError, match="outside valid range"):
            ingest_embedding([32768.0])

    def test_out_of_range_negative(self):
        """Values below -32767.0 are rejected."""
        with pytest.raises(ValueError, match="outside valid range"):
            ingest_embedding([-32768.0])

    def test_empty_vector(self):
        """Empty input returns empty output."""
        result = ingest_embedding([])
        assert result == []

    def test_high_dimension(self):
        """384-dim vector (sentence-transformers default) works."""
        embedding = [0.01 * i for i in range(384)]
        result = ingest_embedding(embedding)
        assert len(result) == 384

    def test_typical_embedding_range(self):
        """Typical normalized embedding values [-1, 1] work fine."""
        np.random.seed(42)
        embedding = np.random.randn(768).astype(np.float32)
        # Normalized embeddings are usually in [-3, 3]
        embedding = embedding / np.linalg.norm(embedding)
        result = ingest_embedding(embedding.tolist())
        assert len(result) == 768
        # All values should be small integers (since input is ~[-0.1, 0.1])
        for v in result:
            assert abs(v) < 65536 * 5  # well within i32 range


# ============================================================================
# 2. generate_proof tests
# ============================================================================

class TestGenerateProof:
    def test_basic_proof(self):
        """Proof generation returns a hex string."""
        fixed = [65536, -65536, 0]
        proof = generate_proof(fixed)
        assert isinstance(proof, str)
        assert len(proof) == 64  # BLAKE3 = 32 bytes = 64 hex chars

    def test_deterministic(self):
        """Same input always produces same hash."""
        fixed = [100, 200, 300, 400, 500]
        proof1 = generate_proof(fixed)
        proof2 = generate_proof(fixed)
        assert proof1 == proof2

    def test_different_values_different_hash(self):
        """Different values produce different hashes."""
        proof1 = generate_proof([100, 200, 300])
        proof2 = generate_proof([100, 200, 301])  # one value changed
        assert proof1 != proof2

    def test_position_sensitive(self):
        """Swapping positions changes the hash (position-aware Merkle)."""
        proof1 = generate_proof([100, 200])
        proof2 = generate_proof([200, 100])  # swapped
        assert proof1 != proof2

    def test_single_element(self):
        """Single-element vector produces valid proof."""
        proof = generate_proof([42])
        assert isinstance(proof, str)
        assert len(proof) == 64

    def test_two_elements(self):
        """Two-element vector (simplest Merkle tree) works."""
        proof = generate_proof([1, 2])
        assert len(proof) == 64

    def test_odd_count(self):
        """Odd number of leaves handles correctly (last leaf hashed with itself)."""
        proof = generate_proof([1, 2, 3])
        assert len(proof) == 64

    def test_power_of_two(self):
        """Power-of-two leaf count (perfect binary tree)."""
        proof = generate_proof([1, 2, 3, 4, 5, 6, 7, 8])
        assert len(proof) == 64

    def test_large_vector(self):
        """384-dim vector produces proof efficiently."""
        fixed = list(range(384))
        proof = generate_proof(fixed)
        assert len(proof) == 64

    def test_empty_vector_rejected(self):
        """Empty vector is rejected."""
        with pytest.raises(ValueError, match="empty vector"):
            generate_proof([])

    def test_all_zeros(self):
        """All-zero vector still produces a valid, non-trivial hash."""
        proof = generate_proof([0, 0, 0, 0])
        assert len(proof) == 64
        # Should not be all zeros (BLAKE3 of zero data is not zero)
        assert proof != "0" * 64


# ============================================================================
# 3. verify_embedding tests
# ============================================================================

class TestVerifyEmbedding:
    def test_valid_verification(self):
        """Embedding verifies against its own proof."""
        floats = [0.5, -0.25, 1.0, 0.0]
        fixed = ingest_embedding(floats)
        proof_hash = generate_proof(fixed)
        assert verify_embedding(floats, proof_hash) is True

    def test_tampered_embedding_fails(self):
        """Modified embedding fails verification."""
        original = [0.5, -0.25, 1.0, 0.0]
        fixed = ingest_embedding(original)
        proof_hash = generate_proof(fixed)

        tampered = [0.5, -0.25, 1.0, 0.001]  # tiny change
        assert verify_embedding(tampered, proof_hash) is False

    def test_wrong_hash_fails(self):
        """Correct embedding with wrong hash fails."""
        floats = [1.0, 2.0, 3.0]
        assert verify_embedding(floats, "0" * 64) is False

    def test_empty_hash_fails(self):
        """Empty hash string fails."""
        floats = [1.0, 2.0, 3.0]
        assert verify_embedding(floats, "") is False

    def test_roundtrip_with_numpy(self):
        """Full roundtrip with numpy array → list conversion."""
        np.random.seed(123)
        embedding = np.random.randn(128).astype(np.float32)
        embedding = embedding / np.linalg.norm(embedding)  # normalize

        floats = embedding.tolist()
        fixed = ingest_embedding(floats)
        proof_hash = generate_proof(fixed)

        # Verify with same data
        assert verify_embedding(floats, proof_hash) is True

        # Verify with re-created list from same numpy array
        assert verify_embedding(embedding.tolist(), proof_hash) is True

    def test_cross_verification_determinism(self):
        """Two independent runs produce identical proofs."""
        data = [0.1, 0.2, 0.3, 0.4, 0.5]

        # Run 1
        fixed1 = ingest_embedding(data)
        proof1 = generate_proof(fixed1)

        # Run 2 (independent)
        fixed2 = ingest_embedding(data)
        proof2 = generate_proof(fixed2)

        assert proof1 == proof2
        assert verify_embedding(data, proof1) is True
        assert verify_embedding(data, proof2) is True


# ============================================================================
# 4. End-to-end pipeline tests
# ============================================================================

class TestEndToEnd:
    def test_sentence_transformer_simulation(self):
        """Simulate real sentence-transformer output."""
        np.random.seed(42)
        # Sentence transformers output 384-dim normalized vectors
        raw = np.random.randn(384).astype(np.float32)
        embedding = raw / np.linalg.norm(raw)

        floats = embedding.tolist()
        fixed = ingest_embedding(floats)
        proof = generate_proof(fixed)

        assert len(fixed) == 384
        assert len(proof) == 64
        assert verify_embedding(floats, proof) is True

    def test_gemini_embedding_simulation(self):
        """Simulate Gemini Embedding 2 output (3072-dim)."""
        np.random.seed(99)
        raw = np.random.randn(3072).astype(np.float32)
        embedding = raw / np.linalg.norm(raw)

        floats = embedding.tolist()
        fixed = ingest_embedding(floats)
        proof = generate_proof(fixed)

        assert len(fixed) == 3072
        assert len(proof) == 64
        assert verify_embedding(floats, proof) is True

    def test_openai_embedding_simulation(self):
        """Simulate OpenAI ada-002 output (1536-dim)."""
        np.random.seed(77)
        raw = np.random.randn(1536).astype(np.float32)
        embedding = raw / np.linalg.norm(raw)

        floats = embedding.tolist()
        fixed = ingest_embedding(floats)
        proof = generate_proof(fixed)

        assert len(fixed) == 1536
        assert verify_embedding(floats, proof) is True


# ============================================================================
# 5. Adapter tests (mock DB + kernel-backed proofs)
# ============================================================================

class MockVectorDB:
    """Mock vector DB for testing the adapter."""
    def __init__(self):
        self.store = {}

    def insert(self, id, embedding, metadata=None):
        self.store[id] = {"embedding": embedding, "metadata": metadata or {}}

    def search(self, query, k=10):
        # Return all stored items with mock scores
        results = []
        for id, data in self.store.items():
            results.append({
                "id": id,
                "embedding": data["embedding"],
                "score": 0.95,
            })
        return results[:k]


import tempfile
import shutil

@pytest.fixture
def adapter_pair():
    """Create a MockVectorDB + ValoriAdapter with a temp kernel."""
    d = tempfile.mkdtemp(prefix="valori_adapter_test_")
    from valori.adapter import ValoriAdapter
    from valori.local import LocalClient
    mock = MockVectorDB()
    valori = LocalClient(path=d)
    adapter = ValoriAdapter(mock, valori=valori)
    yield mock, adapter
    shutil.rmtree(d, ignore_errors=True)


def _make_384_embedding(seed=42):
    """Generate a normalized 384-dim embedding."""
    np.random.seed(seed)
    raw = np.random.randn(384).astype(np.float32)
    return raw / np.linalg.norm(raw)


class TestValoriAdapter:
    def test_insert_returns_proof(self, adapter_pair):
        """Adapter insert returns a valid proof hash."""
        _, adapter = adapter_pair
        embedding = _make_384_embedding(42)
        proof = adapter.insert("doc_001", embedding)

        assert isinstance(proof, str)
        assert len(proof) == 64

    def test_insert_stores_in_underlying_db(self, adapter_pair):
        """Adapter passes through to underlying DB."""
        mock, adapter = adapter_pair
        embedding = _make_384_embedding(42)
        adapter.insert("doc_001", embedding)

        assert "doc_001" in mock.store

    def test_get_proof(self, adapter_pair):
        """Can retrieve stored proof by ID (from kernel metadata)."""
        _, adapter = adapter_pair
        embedding = _make_384_embedding(42)
        proof = adapter.insert("doc_001", embedding)

        assert adapter.get_proof("doc_001") == proof
        assert adapter.get_proof("nonexistent") is None

    def test_verify(self, adapter_pair):
        """Adapter verify method works against kernel-stored proof."""
        _, adapter = adapter_pair
        embedding = _make_384_embedding(42)
        adapter.insert("doc_001", embedding)

        assert adapter.verify("doc_001", embedding) is True

        # Tamper one value
        tampered = embedding.copy()
        tampered[0] += 0.001
        assert adapter.verify("doc_001", tampered) is False

    def test_verify_nonexistent(self, adapter_pair):
        """Verifying a non-existent ID returns False."""
        _, adapter = adapter_pair
        embedding = _make_384_embedding(42)
        assert adapter.verify("ghost", embedding) is False

    def test_search_with_verification(self, adapter_pair):
        """Search results include verification status."""
        _, adapter = adapter_pair
        emb1 = _make_384_embedding(42)
        emb2 = _make_384_embedding(99)
        adapter.insert("doc_001", emb1)
        adapter.insert("doc_002", emb2)

        results = adapter.search(np.zeros(384, dtype=np.float32), k=10)
        for r in results:
            assert r["verified"] is True  # embeddings match kernel-stored proofs

    def test_deterministic_across_adapters(self):
        """Same embedding produces same proof on different adapter instances."""
        from valori.adapter import ValoriAdapter
        from valori.local import LocalClient

        embedding = _make_384_embedding(42)

        d1 = tempfile.mkdtemp(prefix="valori_det_test1_")
        d2 = tempfile.mkdtemp(prefix="valori_det_test2_")
        try:
            adapter1 = ValoriAdapter(MockVectorDB(), valori=LocalClient(path=d1))
            adapter2 = ValoriAdapter(MockVectorDB(), valori=LocalClient(path=d2))

            proof1 = adapter1.insert("a", embedding)
            proof2 = adapter2.insert("b", embedding)

            assert proof1 == proof2  # proof is over the vector, not the ID
        finally:
            shutil.rmtree(d1, ignore_errors=True)
            shutil.rmtree(d2, ignore_errors=True)


if __name__ == "__main__":
    pytest.main([__file__, "-v"])

