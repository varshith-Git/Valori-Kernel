import os
import shutil
from valoricore.local import LocalClient

def test_delete():
    # Clean up old db if exists
    db_path = "./valoricore_test_delete_db"
    if os.path.exists(db_path):
        shutil.rmtree(db_path)
        
    client = LocalClient(path=db_path)

    # Insert a record
    rid = client.insert([0.5]*16)
    print(f"Inserted record: {rid}")
    
    # Verify it exists
    count_before = client.record_count()
    print(f"Record count before delete: {count_before}")
    assert count_before == 1

    # Delete the record
    print(f"Deleting record {rid}...")
    client.delete(rid)

    # Verify it's gone
    count_after = client.record_count()
    print(f"Record count after delete: {count_after}")
    assert count_after == 0

    print("SUCCESS: FFI Delete operation works and physically removes the record!")

if __name__ == "__main__":
    test_delete()
