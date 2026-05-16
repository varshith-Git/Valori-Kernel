import sys
import os
import shutil
sys.path.append(os.path.abspath("python"))
from valoricore import Valoricore

db_path = "data/test_auto_snapshot"
if os.path.exists(db_path):
    shutil.rmtree(db_path)

db = Valoricore(path=db_path)
db.snapshot(auto_interval=5)

for i in range(12):
    db.insert([1.0] + [0.0]*383, i)

print("Files in db directory:")
print(os.listdir(db_path))
