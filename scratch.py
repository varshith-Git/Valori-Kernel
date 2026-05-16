import sys
import os
sys.path.append(os.path.abspath("python"))
from sentence_transformers import SentenceTransformer

model = SentenceTransformer("all-MiniLM-L6-v2")
anchors = {
    "materials": model.encode("insulation material saltwater resistance", normalize_embeddings=True).tolist(),
    "maintenance": model.encode("cable repair crew dangerous operations", normalize_embeddings=True).tolist(),
}

texts = [
    "One of the biggest breakthroughs came from the development of gutta-percha insulation. Gutta-percha was a natural latex material collected from trees in Southeast Asia. Unlike rubber available at the time, it resisted saltwater corrosion and prevented electrical leakage. Without gutta-percha, undersea communication would likely have been delayed by decades.",
    "Cable repair missions were extremely dangerous. Engineers aboard repair ships used grappling hooks to locate damaged cables thousands of meters underwater. Storms, strong currents, and inaccurate maps made recovery operations unpredictable. Sometimes repair crews spent weeks searching for a single broken section."
]

for t in texts:
    emb = model.encode(t, normalize_embeddings=True).tolist()
    print("---")
    for name, a in anchors.items():
        sim = sum(x*y for x,y in zip(emb, a))
        print(f"{name}: {sim:.3f}")
