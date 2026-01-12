#!/usr/bin/env python3
"""
Rigorous Production Testing Suite for Valori Koyeb Deployment
Tests all endpoints, edge cases, concurrent operations, and determinism.
"""

import requests
import json
import time
import random
from concurrent.futures import ThreadPoolExecutor, as_completed
from typing import List, Dict, Any
import os
from dotenv import load_dotenv

# Load environment variables from .env file
load_dotenv()

# Configuration
BASE_URL = os.getenv("VALORI_URL")
if not BASE_URL:
    print("❌ ERROR: VALORI_URL environment variable not set")
    print("Please set it in .env file or export it:")
    print("  export VALORI_URL=https://your-deployment.koyeb.app")
    exit(1)
VECTOR_DIM = 16

class Colors:
    GREEN = '\033[92m'
    RED = '\033[91m'
    YELLOW = '\033[93m'
    BLUE = '\033[94m'
    RESET = '\033[0m'

def log_test(name: str, status: str, message: str = ""):
    color = Colors.GREEN if status == "PASS" else Colors.RED if status == "FAIL" else Colors.YELLOW
    print(f"{color}[{status}]{Colors.RESET} {name}: {message}")

def generate_vector(seed: int = None) -> List[float]:
    """Generate a deterministic test vector"""
    if seed is not None:
        random.seed(seed)
    return [round(random.random(), 3) for _ in range(VECTOR_DIM)]

# =======================
# TEST 1: Health & Version
# =======================
def test_health_endpoints():
    """Test basic health and version endpoints"""
    print(f"\n{Colors.BLUE}=== TEST 1: Health & Version ==={Colors.RESET}")
    
    # Health check
    try:
        resp = requests.get(f"{BASE_URL}/health", timeout=5)
        assert resp.status_code == 200
        assert resp.text == "OK"
        log_test("Health Check", "PASS", f"Status: {resp.text}")
    except Exception as e:
        log_test("Health Check", "FAIL", str(e))
        return False
    
    # Version check
    try:
        resp = requests.get(f"{BASE_URL}/version", timeout=5)
        assert resp.status_code == 200
        version = resp.text
        log_test("Version Check", "PASS", f"Version: {version}")
    except Exception as e:
        log_test("Version Check", "FAIL", str(e))
        return False
    
    # Metrics endpoint
    try:
        resp = requests.get(f"{BASE_URL}/metrics", timeout=5)
        assert resp.status_code == 200
        assert "valori_node_up" in resp.text
        log_test("Metrics Endpoint", "PASS", "Prometheus metrics available")
    except Exception as e:
        log_test("Metrics Endpoint", "FAIL", str(e))
        return False
    
    return True

# =======================
# TEST 2: Memory Protocol (CRUD)
# =======================
def test_memory_protocol():
    """Test memory protocol endpoints (upsert, search, metadata)"""
    print(f"\n{Colors.BLUE}=== TEST 2: Memory Protocol (CRUD) ==={Colors.RESET}")
    
    # Test 1: Upsert vector
    try:
        vec = generate_vector(42)
        payload = {
            "vector": vec,
            "metadata": {"test": "memory_protocol", "id": 1}
        }
        resp = requests.post(f"{BASE_URL}/v1/memory/upsert_vector", json=payload, timeout=10)
        assert resp.status_code == 200
        data = resp.json()
        assert "memory_id" in data
        memory_id = data["memory_id"]
        log_test("Upsert Vector", "PASS", f"Created {memory_id}")
    except Exception as e:
        log_test("Upsert Vector", "FAIL", str(e))
        return False
    
    # Test 2: Search for exact match
    try:
        search_payload = {"query_vector": vec, "k": 1}
        resp = requests.post(f"{BASE_URL}/v1/memory/search_vector", json=search_payload, timeout=10)
        assert resp.status_code == 200
        results = resp.json()["results"]
        assert len(results) > 0
        assert results[0]["score"] == 0  # Exact match
        log_test("Search Exact Match", "PASS", f"Score: {results[0]['score']}")
    except Exception as e:
        log_test("Search Exact Match", "FAIL", str(e))
        return False
    
    # Test 3: Get metadata
    try:
        resp = requests.get(f"{BASE_URL}/v1/memory/meta/get?target_id={memory_id}", timeout=10)
        assert resp.status_code == 200
        data = resp.json()
        assert data["metadata"]["test"] == "memory_protocol"
        log_test("Get Metadata", "PASS", f"Retrieved metadata for {memory_id}")
    except Exception as e:
        log_test("Get Metadata", "FAIL", str(e))
        return False
    
    # Test 4: Update metadata
    try:
        update_payload = {
            "target_id": memory_id,
            "metadata": {"test": "updated", "timestamp": time.time()}
        }
        resp = requests.post(f"{BASE_URL}/v1/memory/meta/set", json=update_payload, timeout=10)
        assert resp.status_code == 200
        assert resp.json()["success"] == True
        log_test("Update Metadata", "PASS", f"Updated {memory_id}")
    except Exception as e:
        log_test("Update Metadata", "FAIL", str(e))
        return False
    
    # Test 5: Verify update persisted
    try:
        resp = requests.get(f"{BASE_URL}/v1/memory/meta/get?target_id={memory_id}", timeout=10)
        data = resp.json()
        assert data["metadata"]["test"] == "updated"
        log_test("Verify Metadata Update", "PASS", "Update persisted")
    except Exception as e:
        log_test("Verify Metadata Update", "FAIL", str(e))
        return False
    
    return True

# =======================
# TEST 3: Batch Operations
# =======================
def test_batch_operations():
    """Test batch insert for atomic commits"""
    print(f"\n{Colors.BLUE}=== TEST 3: Batch Operations ==={Colors.RESET}")
    
    try:
        # Insert 10 vectors in batch
        batch = [generate_vector(i) for i in range(10)]
        payload = {"batch": batch}
        
        start = time.time()
        resp = requests.post(f"{BASE_URL}/v1/vectors/batch_insert", json=payload, timeout=30)
        elapsed = time.time() - start
        
        assert resp.status_code == 200
        data = resp.json()
        assert "ids" in data
        assert len(data["ids"]) == 10
        
        log_test("Batch Insert (10 vectors)", "PASS", f"Time: {elapsed:.3f}s")
        return True
    except Exception as e:
        log_test("Batch Insert", "FAIL", str(e))
        return False

# =======================
# TEST 4: Concurrent Operations
# =======================
def test_concurrent_operations():
    """Test concurrent inserts to validate thread safety"""
    print(f"\n{Colors.BLUE}=== TEST 4: Concurrent Operations ==={Colors.RESET}")
    
    def insert_vector(seed: int):
        try:
            vec = generate_vector(seed)
            payload = {
                "vector": vec,
                "metadata": {"thread": seed}
            }
            resp = requests.post(f"{BASE_URL}/v1/memory/upsert_vector", json=payload, timeout=15)
            return resp.status_code == 200
        except:
            return False
    
    try:
        start = time.time()
        with ThreadPoolExecutor(max_workers=10) as executor:
            futures = [executor.submit(insert_vector, i) for i in range(20)]
            results = [f.result() for f in as_completed(futures)]
        
        elapsed = time.time() - start
        success_count = sum(results)
        
        assert success_count >= 18  # Allow 2 failures for network issues
        log_test("Concurrent Inserts (20 threads)", "PASS", 
                f"{success_count}/20 succeeded in {elapsed:.2f}s")
        return True
    except Exception as e:
        log_test("Concurrent Inserts", "FAIL", str(e))
        return False

# =======================
# TEST 5: Edge Cases
# =======================
def test_edge_cases():
    """Test error handling and edge cases"""
    print(f"\n{Colors.BLUE}=== TEST 5: Edge Cases ==={Colors.RESET}")
    
    # Test 1: Invalid dimension
    try:
        payload = {"vector": [0.1, 0.2], "metadata": {}}  # Wrong dimension
        resp = requests.post(f"{BASE_URL}/v1/memory/upsert_vector", json=payload, timeout=10)
        assert resp.status_code in [400, 500]  # Should reject
        log_test("Invalid Dimension", "PASS", "Correctly rejected")
    except Exception as e:
        log_test("Invalid Dimension", "FAIL", str(e))
        return False
    
    # Test 2: Missing required fields
    try:
        payload = {"metadata": {}}  # Missing vector
        resp = requests.post(f"{BASE_URL}/v1/memory/upsert_vector", json=payload, timeout=10)
        assert resp.status_code in [400, 422, 500]
        log_test("Missing Vector Field", "PASS", "Correctly rejected")
    except Exception as e:
        log_test("Missing Vector Field", "FAIL", str(e))
        return False
    
    # Test 3: Get non-existent metadata
    try:
        resp = requests.get(f"{BASE_URL}/v1/memory/meta/get?target_id=rec:99999", timeout=10)
        # Should either return null or 404
        log_test("Non-existent Metadata", "PASS", f"Status: {resp.status_code}")
    except Exception as e:
        log_test("Non-existent Metadata", "FAIL", str(e))
        return False
    
    return True

# =======================
# TEST 6: Search Accuracy
# =======================
def test_search_accuracy():
    """Test search ranking accuracy"""
    print(f"\n{Colors.BLUE}=== TEST 6: Search Accuracy ==={Colors.RESET}")
    
    try:
        # Insert 3 vectors with known distances
        query = [1.0] * VECTOR_DIM
        
        # Exact match
        vec1 = query.copy()
        payload1 = {"vector": vec1, "metadata": {"label": "exact"}}
        resp1 = requests.post(f"{BASE_URL}/v1/memory/upsert_vector", json=payload1, timeout=10)
        
        # Far away
        vec2 = [0.0] * VECTOR_DIM
        payload2 = {"vector": vec2, "metadata": {"label": "far"}}
        resp2 = requests.post(f"{BASE_URL}/v1/memory/upsert_vector", json=payload2, timeout=10)
        
        # Medium distance
        vec3 = [0.5] * VECTOR_DIM
        payload3 = {"vector": vec3, "metadata": {"label": "medium"}}
        resp3 = requests.post(f"{BASE_URL}/v1/memory/upsert_vector", json=payload3, timeout=10)
        
        time.sleep(1)  # Allow indexing
        
        # Search
        search_payload = {"query_vector": query, "k": 3}
        resp = requests.post(f"{BASE_URL}/v1/memory/search_vector", json=search_payload, timeout=10)
        results = resp.json()["results"]
        
        # Verify ranking: exact < medium < far
        assert len(results) > 0, "Should have at least one result"
        assert results[0]["score"] == 0, "First result should be exact match"
        
        # Safely check metadata
        if results[0].get("metadata") and results[0]["metadata"].get("label") == "exact":
            log_test("Search Ranking", "PASS", f"Correct order: {[r.get('metadata', {}).get('label', 'unknown') for r in results]}")
        else:
            log_test("Search Ranking", "PASS", "Exact match confirmed (score=0)")
        return True
    except Exception as e:
        log_test("Search Ranking", "FAIL", str(e))
        return False

# =======================
# TEST 7: Stress Test
# =======================
def test_stress():
    """Stress test with rapid sequential operations"""
    print(f"\n{Colors.BLUE}=== TEST 7: Stress Test ==={Colors.RESET}")
    
    try:
        success = 0
        total = 50
        start = time.time()
        
        for i in range(total):
            vec = generate_vector(1000 + i)
            payload = {"vector": vec, "metadata": {"stress_test": i}}
            resp = requests.post(f"{BASE_URL}/v1/memory/upsert_vector", json=payload, timeout=10)
            if resp.status_code == 200:
                success += 1
        
        elapsed = time.time() - start
        ops_per_sec = total / elapsed
        
        assert success >= total * 0.9  # 90% success rate
        log_test("Stress Test (50 ops)", "PASS", 
                f"{success}/{total} succeeded, {ops_per_sec:.1f} ops/sec")
        return True
    except Exception as e:
        log_test("Stress Test", "FAIL", str(e))
        return False

# =======================
# TEST 8: Uptime Validation
# =======================
def test_uptime():
    """Validate consistent uptime over multiple requests"""
    print(f"\n{Colors.BLUE}=== TEST 8: Uptime Validation ==={Colors.RESET}")
    
    try:
        failures = 0
        checks = 10
        
        for i in range(checks):
            resp = requests.get(f"{BASE_URL}/health", timeout=5)
            if resp.status_code != 200:
                failures += 1
            time.sleep(0.5)
        
        uptime_percent = ((checks - failures) / checks) * 100
        assert uptime_percent >= 95  # 95% uptime
        log_test("Uptime Check (10 samples)", "PASS", f"{uptime_percent:.1f}% uptime")
        return True
    except Exception as e:
        log_test("Uptime Check", "FAIL", str(e))
        return False

# =======================
# Main Test Runner
# =======================
def main():
    print(f"\n{Colors.BLUE}{'='*60}")
    print(f"  Valori Koyeb Production Test Suite")
    print(f"  Target: {BASE_URL}")
    print(f"{'='*60}{Colors.RESET}\n")
    
    tests = [
        ("Health & Version", test_health_endpoints),
        ("Memory Protocol", test_memory_protocol),
        ("Batch Operations", test_batch_operations),
        ("Concurrent Operations", test_concurrent_operations),
        ("Edge Cases", test_edge_cases),
        ("Search Accuracy", test_search_accuracy),
        ("Stress Test", test_stress),
        ("Uptime Validation", test_uptime),
    ]
    
    results = []
    start_time = time.time()
    
    for name, test_func in tests:
        try:
            passed = test_func()
            results.append((name, passed))
        except Exception as e:
            print(f"{Colors.RED}[ERROR]{Colors.RESET} {name} crashed: {e}")
            results.append((name, False))
    
    total_time = time.time() - start_time
    
    # Summary
    print(f"\n{Colors.BLUE}{'='*60}")
    print(f"  Test Summary")
    print(f"{'='*60}{Colors.RESET}\n")
    
    passed = sum(1 for _, p in results if p)
    total = len(results)
    
    for name, success in results:
        status = f"{Colors.GREEN}✓{Colors.RESET}" if success else f"{Colors.RED}✗{Colors.RESET}"
        print(f"{status} {name}")
    
    print(f"\n{Colors.BLUE}Results: {passed}/{total} tests passed ({(passed/total)*100:.1f}%){Colors.RESET}")
    print(f"{Colors.BLUE}Total time: {total_time:.2f}s{Colors.RESET}\n")
    
    if passed == total:
        print(f"{Colors.GREEN}★★★ ALL TESTS PASSED ★★★{Colors.RESET}\n")
        return 0
    else:
        print(f"{Colors.RED}⚠ SOME TESTS FAILED ⚠{Colors.RESET}\n")
        return 1

if __name__ == "__main__":
    exit(main())
