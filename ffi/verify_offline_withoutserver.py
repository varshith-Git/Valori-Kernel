import valoricore_ffi

# Data from your test run
vector_0 = [1.0] + [0.0]*14
proof_0 = "228a657a05dddc92a16346dd70957ebf3ad34013dd60a3cd6fc84bd7681779ea"

# Offline Verification
# This doesn't need a ValoricoreEngine instance!
is_valid = valoricore_ffi.verify_embedding(vector_0, proof_0)

if is_valid:
    print("🛡️ Integrity Verified: The vector matches the proof exactly.")
else:
    print("🚨 Integrity Failed: The vector or proof has been tampered with.")
