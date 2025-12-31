#!/bin/bash
# scripts/download_data.sh - Download SIFT1M benchmark dataset

set -e  # Exit on error

echo "🚀 Setting up Valori Benchmark Data..."
mkdir -p data/sift

# Check if file exists to avoid re-downloading
if [ ! -f "data/sift/sift_base.fvecs" ]; then
    echo "📥 Downloading SIFT1M Dataset (~150MB)..."
    curl -o data/sift/sift.tar.gz ftp://ftp.irisa.fr/local/texmex/corpus/sift.tar.gz
    
    echo "📦 Extracting..."
    tar -xzvf data/sift/sift.tar.gz -C data/sift --strip-components=1
    
    # Cleanup
    rm data/sift/sift.tar.gz
    echo "✅ SIFT1M Dataset downloaded successfully."
else
    echo "✅ SIFT Data already exists. Skipping download."
fi

echo ""
echo "📊 Dataset files:"
ls -lh data/sift/*.fvecs data/sift/*.ivecs 2>/dev/null || echo "Files ready for use."
echo ""
echo "✨ Setup complete! You can now run benchmarks with:"
echo "   cargo run --release --bin bench_recall"
