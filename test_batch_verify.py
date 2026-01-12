#!/usr/bin/env python3
"""Quick test to verify batch insert endpoint after Event Log configuration"""

import requests
import os
from dotenv import load_dotenv

# Load environment variables from .env file
load_dotenv()

BASE_URL = os.getenv("VALORI_URL")
if not BASE_URL:
    print("❌ ERROR: VALORI_URL environment variable not set")
    print("Please set it in .env file or export it:")
    print("  export VALORI_URL=https://your-deployment.koyeb.app")
    exit(1)

def test_batch_insert():
    """Test batch insert endpoint"""
    print(f"Testing batch insert on {BASE_URL}")
    
    # Prepare batch of 5 vectors
    batch = [
        [0.1, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.2, 0.3, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.3, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.4, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        [0.5, 0.6, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
    ]
    
    payload = {"batch": batch}
    
    try:
        resp = requests.post(f"{BASE_URL}/v1/vectors/batch_insert", json=payload, timeout=10)
        
        print(f"\nStatus Code: {resp.status_code}")
        print(f"Response: {resp.text}\n")
        
        if resp.status_code == 200:
            data = resp.json()
            if "ids" in data:
                print(f"✅ SUCCESS: Batch inserted {len(data['ids'])} vectors")
                print(f"   Assigned IDs: {data['ids']}")
                return True
            else:
                print(f"⚠️  Unexpected response format: {data}")
                return False
        else:
            print(f"❌ FAILED: {resp.text}")
            if "Event Log" in resp.text:
                print("\n💡 Event Log still not initialized. Possible issues:")
                print("   1. Koyeb deployment not restarted yet")
                print("   2. /app/data directory doesn't exist or not writable")
                print("   3. Persistent volume not mounted")
            return False
            
    except Exception as e:
        print(f"❌ ERROR: {e}")
        return False

if __name__ == "__main__":
    print("="*60)
    print("  Valori Batch Insert Verification")
    print("="*60)
    
    # First check if service is up
    print("\n1. Checking health...")
    try:
        health = requests.get(f"{BASE_URL}/health", timeout=5)
        if health.status_code == 200:
            print("   ✅ Service is up")
        else:
            print(f"   ⚠️  Health check returned {health.status_code}")
    except Exception as e:
        print(f"   ❌ Service unreachable: {e}")
        print("\n   Please restart your Koyeb deployment first!")
        exit(1)
    
    # Test batch insert
    print("\n2. Testing batch insert...")
    success = test_batch_insert()
    
    if success:
        print("\n" + "="*60)
        print("  ✅ Batch insert is now working!")
        print("="*60)
        exit(0)
    else:
        print("\n" + "="*60)
        print("  ❌ Batch insert still failing")
        print("="*60)
        print("\nNext steps:")
        print("1. Check Koyeb logs for Event Log initialization message")
        print("2. Verify /app/data exists and is writable")
        print("3. Check if persistent storage is mounted")
        exit(1)
