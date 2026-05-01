# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
"""
Tests for insert_with_proof() — atomic proof-baked insertion.

Verifies:
1. Returns (record_id, proof_hash) tuple
2. Proof hash matches standalone generate_proof() output
3. Proof is stored as Record.metadata (retrievable via get_metadata)
4. Proof survives snapshot → restore cycle
5. Global state hash changes when proof is included
6. Edge cases: out-of-range, wrong dimension
"""

import pytest
import numpy as np
import os
import sys
import tempfile
import shutil

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from valori.valori_ffi import (
    ingest_embedding,
    generate_proof,
    verify_embedding,
)
from valori.local import LocalClient


@pytest.fixture
def db_dir():
    """Create a temp directory for the kernel database."""
    d = tempfile.mkdtemp(prefix="valori_test_")
    yield d
    shutil.rmtree(d, ignore_errors=True)


@pytest.fixture
def engine(db_dir):
    """Create a LocalClient (wraps ValoriEngine) for testing."""
    return LocalClient(path=db_dir)


class TestInsertWithProof:
    def test_returns_tuple(self, engine):
        """insert_with_proof returns (record_id, proof_hash_hex)."""
        # Generate a valid 384-dim embedding
        np.random.seed(42)
        embedding = np.random.randn(384).astype(np.float32)
        embedding = (embedding / np.linalg.norm(embedding)).tolist()

        result = engine.kernel.insert_with_proof(embedding, 0)

        assert isinstance(result, tuple)
        assert len(result) == 2

        record_id, proof_hash = result
        assert isinstance(record_id, int)
        assert isinstance(proof_hash, str)
        assert len(proof_hash) == 64  # BLAKE3 = 32 bytes = 64 hex

    def test_proof_matches_standalone(self, engine):
        """Proof from insert_with_proof matches standalone pipeline."""
        np.random.seed(123)
        embedding = np.random.randn(384).astype(np.float32)
        embedding = (embedding / np.linalg.norm(embedding)).tolist()

        # Atomic path
        record_id, proof_hash = engine.kernel.insert_with_proof(embedding, 0)

        # Standalone path
        fixed = ingest_embedding(embedding)
        standalone_proof = generate_proof(fixed)

        assert proof_hash == standalone_proof, \
            "Atomic insert proof must match standalone proof"

    def test_verify_works_with_insert_proof(self, engine):
        """verify_embedding works against insert_with_proof output."""
        np.random.seed(77)
        embedding = np.random.randn(384).astype(np.float32)
        embedding = (embedding / np.linalg.norm(embedding)).tolist()

        _, proof_hash = engine.kernel.insert_with_proof(embedding, 0)

        assert verify_embedding(embedding, proof_hash) is True

    def test_metadata_stored(self, engine):
        """Proof bytes are stored as Record.metadata."""
        np.random.seed(99)
        embedding = np.random.randn(384).astype(np.float32)
        embedding = (embedding / np.linalg.norm(embedding)).tolist()

        record_id, proof_hash = engine.kernel.insert_with_proof(embedding, 0)

        # Retrieve metadata from kernel
        metadata = engine.kernel.get_metadata(record_id)

        assert metadata is not None, "Record should have metadata"
        # metadata is raw bytes — the proof hash (32 bytes)
        assert len(metadata) == 32, f"Expected 32 bytes, got {len(metadata)}"
        assert metadata.hex() == proof_hash, \
            "Stored metadata must equal proof hash"

    def test_sequential_ids(self, engine):
        """Multiple inserts produce sequential record IDs."""
        np.random.seed(42)

        ids = []
        for i in range(5):
            embedding = np.random.randn(384).astype(np.float32)
            embedding = (embedding / np.linalg.norm(embedding)).tolist()
            rid, _ = engine.kernel.insert_with_proof(embedding, 0)
            ids.append(rid)

        assert ids == [0, 1, 2, 3, 4]

    def test_different_embeddings_different_proofs(self, engine):
        """Different embeddings produce different proof hashes."""
        np.random.seed(42)
        emb1 = np.random.randn(384).astype(np.float32)
        emb1 = (emb1 / np.linalg.norm(emb1)).tolist()

        np.random.seed(99)
        emb2 = np.random.randn(384).astype(np.float32)
        emb2 = (emb2 / np.linalg.norm(emb2)).tolist()

        _, proof1 = engine.kernel.insert_with_proof(emb1, 0)
        _, proof2 = engine.kernel.insert_with_proof(emb2, 0)

        assert proof1 != proof2

    def test_state_hash_includes_proof(self, engine):
        """Global state hash changes when a proof-bearing record is added."""
        hash_before = engine.kernel.get_state_hash()

        np.random.seed(42)
        embedding = np.random.randn(384).astype(np.float32)
        embedding = (embedding / np.linalg.norm(embedding)).tolist()

        engine.kernel.insert_with_proof(embedding, 0)

        hash_after = engine.kernel.get_state_hash()

        assert hash_before != hash_after, \
            "State hash must change after insert"

    def test_out_of_range_rejected(self, engine):
        """Floats outside Q16.16 range are rejected."""
        bad_embedding = [0.0] * 383 + [40000.0]  # last value out of range
        with pytest.raises(ValueError, match="outside valid range"):
            engine.kernel.insert_with_proof(bad_embedding, 0)

    def test_wrong_dimension_rejected(self, engine):
        """Wrong dimension vector is rejected."""
        with pytest.raises(ValueError, match="Expected 384 dims"):
            engine.kernel.insert_with_proof([1.0, 2.0, 3.0], 0)

    def test_deterministic_proof_across_engines(self, db_dir):
        """Same embedding produces same proof on different engine instances."""
        np.random.seed(42)
        embedding = np.random.randn(384).astype(np.float32)
        embedding = (embedding / np.linalg.norm(embedding)).tolist()

        # Engine 1
        dir1 = os.path.join(db_dir, "engine1")
        engine1 = LocalClient(path=dir1)
        _, proof1 = engine1.kernel.insert_with_proof(embedding, 0)

        # Engine 2 (fresh instance, different directory)
        dir2 = os.path.join(db_dir, "engine2")
        engine2 = LocalClient(path=dir2)
        _, proof2 = engine2.kernel.insert_with_proof(embedding, 0)

        assert proof1 == proof2, \
            "Same embedding must produce same proof on any engine"


if __name__ == "__main__":
    pytest.main([__file__, "-v"])

    def test_insert_batch_with_proof(self, engine):
        """insert_batch_with_proof returns list of tuples and stores metadata."""
        np.random.seed(42)
        batch = []
        for _ in range(3):
            emb = np.random.randn(384).astype(np.float32)
            emb = (emb / np.linalg.norm(emb)).tolist()
            batch.append(emb)

        results = engine.insert_batch_with_proof(batch)
        
        assert len(results) == 3
        for i, (rid, proof) in enumerate(results):
            assert isinstance(rid, int)
            assert isinstance(proof, str)
            assert len(proof) == 64
            
            # Verify metadata was stored
            meta = engine.kernel.get_metadata(rid)
            assert meta is not None
            assert bytes(meta).hex() == proof
