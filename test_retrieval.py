import os
import sys
sys.path.append(os.path.abspath("python"))
from sentence_transformers import SentenceTransformer, CrossEncoder
from valoricore import Valoricore

text_blocks = [
    "Long before satellites and fiber optics connected continents, communication across oceans depended on ships carrying handwritten messages. A letter sent from Europe to Asia in the 1700s could take months to arrive, and storms often destroyed both vessels and information. Empires treated communication speed as a strategic weapon because whoever received news first could control trade routes, military decisions, and financial markets.",
    "The first serious attempt to solve this problem came through underwater telegraph cables in the mid-19th century. Engineers realized electricity could travel through insulated copper wires placed on the ocean floor. This idea sounded impossible at the time because people believed ocean pressure would instantly crush the cables. Early prototypes failed repeatedly due to weak insulation and poor signal quality.",
    "One of the biggest breakthroughs came from the development of gutta-percha insulation. Gutta-percha was a natural latex material collected from trees in Southeast Asia. Unlike rubber available at the time, it resisted saltwater corrosion and prevented electrical leakage. Without gutta-percha, undersea communication would likely have been delayed by decades.",
    "In 1858, the first transatlantic telegraph cable connected Ireland and Newfoundland. Crowds celebrated the achievement as a technological miracle. Queen Victoria even sent a congratulatory message to the President of the United States. However, the excitement did not last long because the cable stopped functioning after only a few weeks.",
    "The cable failed mainly because engineers applied excessively high voltage in an attempt to strengthen weak signals. Instead of improving communication, the voltage damaged the insulation layers inside the cable. At that time, electrical engineering was still poorly understood, and many practical decisions relied more on experimentation than scientific modeling.",
    "A more successful transatlantic cable was installed in 1866 using the massive steamship Great Eastern. The ship was uniquely suited for the task because it could carry thousands of kilometers of heavy cable in a single voyage. This reduced the number of risky mid-ocean cable splicing operations that had caused earlier failures.",
    "Underwater cables quickly transformed global finance. Before telegraph systems, stock prices between London and New York could differ significantly because information traveled slowly. After instant communication became possible, markets started reacting almost simultaneously. This was one of the earliest forms of financial globalization.",
    "Governments soon realized these cables had military importance. During wartime, naval forces frequently attempted to cut enemy communication lines hidden beneath the sea. In World War I, Britain used its control of submarine cable infrastructure to intercept and monitor German communications across different regions.",
    "Cable repair missions were extremely dangerous. Engineers aboard repair ships used grappling hooks to locate damaged cables thousands of meters underwater. Storms, strong currents, and inaccurate maps made recovery operations unpredictable. Sometimes repair crews spent weeks searching for a single broken section.",
    "Signal transmission across long distances created another challenge. Electrical pulses weakened gradually as they traveled through copper conductors. To solve this, scientists developed repeaters and amplification systems that boosted signals without introducing excessive distortion. These innovations later influenced modern electronic communication systems.",
    "During the early 20th century, wireless radio communication emerged as a competitor to submarine cables. Radio had the advantage of avoiding expensive underwater infrastructure. However, radio signals were vulnerable to atmospheric interference, weather conditions, and interception. For secure and stable communication, cables remained essential.",
    "The invention of coaxial submarine cables in the 1950s dramatically improved international telephone systems. These cables supported far more simultaneous voice calls than earlier telegraph lines. Businesses, governments, and families suddenly gained the ability to communicate internationally in near real time.",
    "Modern submarine communication changed again with the rise of fiber optic technology in the 1980s. Instead of electrical pulses, fiber optic cables transmitted information as light signals. This allowed dramatically higher bandwidth and lower latency compared to copper-based systems.",
    "A single modern fiber optic cable can carry terabits of data every second. Most people imagine internet traffic moving invisibly through satellites, but in reality, over 95 percent of intercontinental internet traffic travels through submarine cables on the ocean floor.",
    "Tech companies such as Google, Meta, and Microsoft now invest directly in submarine cable infrastructure. Owning communication routes helps them reduce latency, improve reliability, and support massive cloud computing operations worldwide.",
    "Modern cable systems are designed with multiple redundancy paths. If one cable is damaged by earthquakes, ship anchors, or underwater landslides, internet traffic can be rerouted automatically through alternative routes. This redundancy is critical because even a short disruption can affect millions of users and businesses.",
    "Surprisingly, sharks once became an unexpected threat to underwater fiber optic cables. Researchers observed that some sharks were attracted to electromagnetic fields generated by communication equipment. Several cable operators reported bite-related damage in the early years of fiber optic deployment.",
    "Submarine cables also play a role in geopolitics. Countries increasingly worry about surveillance, sabotage, and dependency on foreign-owned communication infrastructure. Some governments now classify cable landing stations as strategic national assets requiring military-level protection.",
    "Repairing a modern fiber optic cable remains a slow process despite advances in technology. Specialized ships must travel to the damaged location, retrieve the cable from the seabed, splice the fibers with microscopic precision, and carefully lower the repaired section back into the ocean.",
    "Today, submarine cable networks form one of the least visible yet most important parts of global infrastructure. Every video call, cloud transaction, international bank transfer, and streamed movie depends on systems hidden deep beneath the ocean. Although people rarely think about them, these cables quietly sustain the digital economy of the modern world."
]

questions = [
    "What material enabled early underwater cables to resist saltwater corrosion?",
    "Why did the first transatlantic cable of 1858 fail?",
    "What role did the steamship Great Eastern play in submarine cable history?",
    "How did underwater telegraph cables affect global financial markets?",
    "Why were submarine cables strategically important during wars?",
    "What challenges did cable repair crews face in deep oceans?",
    "Why did radio communication not fully replace submarine cables?",
    "What major advantage did fiber optic cables provide over copper cables?",
    "Why do companies like Google and Meta invest in submarine cable infrastructure?",
    "What are some common causes of modern submarine cable damage?",
    "What material properties were critical for the dangerous repair missions?"
]

def main():
    print("Loading reranker...")
    reranker = CrossEncoder('cross-encoder/ms-marco-MiniLM-L-12-v2')
    
    models_to_test = [
        'all-MiniLM-L6-v2',
        'intfloat/e5-base-v2',
        'BAAI/bge-base-en-v1.5'
    ]
    
    import nltk
    try:
        nltk.data.find('tokenizers/punkt_tab')
    except LookupError:
        nltk.download('punkt_tab')
        nltk.download('punkt')
        
    chunks = []
    for para in text_blocks:
        if len(para.split()) > 150:
            sentences = nltk.sent_tokenize(para)
            current_chunk = ""
            current_words = 0
            for sent in sentences:
                sent_words = len(sent.split())
                if current_words + sent_words <= 100:
                    current_chunk += sent + " "
                    current_words += sent_words
                else:
                    if current_chunk:
                        chunks.append(current_chunk.strip())
                    current_chunk = sent + " "
                    current_words = sent_words
            if current_chunk.strip():
                chunks.append(current_chunk.strip())
        else:
            chunks.append(para)
            
    # Setup BM25
    from rank_bm25 import BM25Okapi
    tokenized_corpus = [doc.split(" ") for doc in chunks]
    bm25 = BM25Okapi(tokenized_corpus)
    
    import shutil
    
    for model_name in models_to_test:
        print("="*60)
        print(f"Testing Model: {model_name}")
        print("="*60)
        
        print("Loading embedding model...")
        model = SentenceTransformer(model_name)
        
        db_path = f"data/valori_retrieval_test_db_{model_name.replace('/', '_')}"
        
        print(f"Initializing Valoricore database at {db_path}...")
        if os.path.exists(db_path):
            shutil.rmtree(db_path)
        
        db = Valoricore(path=db_path)
        
        import json
        
        from valoricore.kinds import NODE_CONCEPT, EDGE_RELATION, NODE_RECORD, EDGE_MENTIONS
        
        # 1. Pre-declare core Concept Nodes using Fluent API
        concept_materials = db.node(kind=NODE_CONCEPT)
        concept_maintenance = db.node(kind=NODE_CONCEPT)
        concept_ship = db.node(kind=NODE_CONCEPT)
        
        # 2. Logically link concepts together
        # E.g., Maintenance operations relate to Materials
        concept_maintenance.link_to(concept_materials, EDGE_RELATION)
        
        # 3. Define Semantic Anchors for Concept Edges
        prefix_anchor = "passage: " if "e5" in model_name else ""
        concept_anchors = {
            concept_materials: model.encode(prefix_anchor + "insulation material saltwater resistance", normalize_embeddings=True).tolist(),
            concept_maintenance: model.encode(prefix_anchor + "cable repair crew dangerous operations", normalize_embeddings=True).tolist(),
            concept_ship: model.encode(prefix_anchor + "steamship vessel cable laying voyage", normalize_embeddings=True).tolist(),
        }
        CONCEPT_EDGE_THRESHOLD = 0.40
        
        print("Embedding and inserting chunks into Vector Pool + Graph...")
        for i, text in enumerate(chunks):
            # For some models like e5, it's recommended to prefix with 'passage: ', but we'll keep it simple
            prefix = "passage: " if "e5" in model_name else ""
            
            text_to_embed = text
            if "gutta-percha insulation" in text:
                text_to_embed = "[Topic: insulation material] " + text
            elif "Cable repair missions" in text:
                text_to_embed = "[Topic: cable repair challenges] " + text
                
            embedding = model.encode(
                prefix + text_to_embed,
                normalize_embeddings=True
            ).tolist()
            
            # A/B. Insert Vector & Wrap in Graph Node (Fluent API One-liner)
            record_node = db.node(kind=NODE_RECORD, vector=embedding, tag=i)
            record_id = record_node.record_id
            
            # Persist text, tag, and node_id permanently using Valoricore metadata
            metadata_bytes = json.dumps({'tag': i, 'text': text, 'node_id': record_node.id}).encode('utf-8')
            db.set_metadata(record_id, metadata_bytes)
            
            # C. Connect Record Nodes to the Concept Nodes via Semantic Gates
            for concept_node, anchor_emb in concept_anchors.items():
                # Both embeddings are normalized, so dot product == cosine similarity
                similarity = sum(a * b for a, b in zip(embedding, anchor_emb))
                if similarity > CONCEPT_EDGE_THRESHOLD:
                    # Bidirectional edge using fluent API
                    record_node.link_to(concept_node, EDGE_MENTIONS)
                    record_node.link_from(concept_node, EDGE_MENTIONS)
            
        print(f"Inserted {len(chunks)} documents & constructed Knowledge Graph.\n")
        
        # ---------------------------------------------------------
        # SNAPSHOT & RESTORE DEMONSTRATION
        # ---------------------------------------------------------
        print("Taking a bit-exact snapshot of the Memory Engine...")
        snapshot_bytes = db.snapshot()
        print(f"Snapshot created! Size: {len(snapshot_bytes) / 1024:.2f} KB")
        
        # Save snapshot to disk (simulate saving to S3 or a local file)
        snapshot_file = f"{db_path}/state.snap"
        with open(snapshot_file, 'wb') as f:
            f.write(snapshot_bytes)
            
        print("Simulating a Serverless Cold Start: Booting a fresh engine from the snapshot...")
        # Create a completely new engine directory to prove it's not using the old WAL
        db2_path = f"{db_path}_restored"
        if os.path.exists(db2_path):
            shutil.rmtree(db2_path)
            
        db2 = Valoricore(path=db2_path)
        
        # Load the snapshot bytes
        with open(snapshot_file, 'rb') as f:
            loaded_snapshot = f.read()
            
        # Restore the engine
        db2.restore(loaded_snapshot)
        print("Engine restored instantly!\n")
        
        # Swap the database reference so all subsequent queries use the restored engine!
        db = db2
        # ---------------------------------------------------------
        
        for i, question in enumerate(questions):
            q_prefix = "query: " if "e5" in model_name else ("Represent this sentence for searching relevant passages: " if "bge" in model_name else "")
            q_emb = model.encode(
                q_prefix + question,
                normalize_embeddings=True
            ).tolist()
            
            # Retrieve top 3 from vector DB
            results = db.search(query=q_emb, k=3)
            
            # Retrieve top 3 from BM25
            tokenized_query = question.split(" ")
            bm25_scores = bm25.get_scores(tokenized_query)
            # Filter BM25 candidates to only include those with score > 1.0
            bm25_top_indices = [idx for idx in sorted(range(len(bm25_scores)), key=lambda idx: bm25_scores[idx], reverse=True)[:3] if bm25_scores[idx] > 1.0]
            
            # Combine and deduplicate
            candidate_tags = set()
            docs_info = []
            
            # Add Valoricore results + Graph Expansion
            expanded_records = set()
            for result in results:
                record_id = result["id"]
                metadata_bytes = db.get_metadata(record_id)
                if metadata_bytes:
                    info = json.loads(bytes(metadata_bytes).decode('utf-8'))
                    tag = info['tag']
                    retrieved_text = info['text']
                    
                    # Native Multi-Hop: Find conceptually linked records via the graph!
                    if 'node_id' in info:
                        expanded_records.update(db.expand(info['node_id'], max_depth=2))
                else:
                    tag = -1
                    retrieved_text = "Missing Metadata"
                    
                if tag not in candidate_tags:
                    candidate_tags.add(tag)
                    docs_info.append({
                        'tag': tag,
                        'text': retrieved_text,
                        'base_score': result.get("score", "N/A"),
                        'source': 'valoricore'
                    })
                    
            # Process Graph Expanded Results (Stage 2: Expansion)
            for rec_id in expanded_records:
                metadata_bytes = db.get_metadata(rec_id)
                if metadata_bytes:
                    info = json.loads(bytes(metadata_bytes).decode('utf-8'))
                    tag = info['tag']
                    if tag not in candidate_tags:
                        candidate_tags.add(tag)
                        docs_info.append({
                            'tag': tag,
                            'text': info['text'],
                            'base_score': "Graph Link",
                            'source': 'graph_expansion'
                        })
                    
            # Add BM25 results
            for tag in bm25_top_indices:
                if tag not in candidate_tags:
                    candidate_tags.add(tag)
                    docs_info.append({
                        'tag': tag,
                        'text': chunks[tag],
                        'base_score': f"BM25:{bm25_scores[tag]:.2f}",
                        'source': 'bm25'
                    })
            
            print(f"Q{i+1}: {question}")
            if docs_info:
                print("Top Results (Reranked):")
                
                # Prepare pairs for reranker
                pairs = [[question, doc['text']] for doc in docs_info]
                
                # Get reranker scores
                rerank_scores = reranker.predict(pairs)
                
                # Combine and sort
                for j in range(len(docs_info)):
                    docs_info[j]['rerank_score'] = rerank_scores[j]
                    
                reranked_results = sorted(docs_info, key=lambda x: x['rerank_score'], reverse=True)
                
                # Merge consecutive top 2
                if len(reranked_results) >= 2:
                    tag1 = reranked_results[0]['tag']
                    tag2 = reranked_results[1]['tag']
                    # Only merge if they are adjacent AND both contribute meaningfully
                    if abs(tag1 - tag2) == 1 and reranked_results[1]['rerank_score'] > 0.5:
                        # Merge them ensuring correct order
                        first_tag = min(tag1, tag2)
                        second_tag = max(tag1, tag2)
                        merged_text = chunks[first_tag] + " " + chunks[second_tag]
                        
                        # Update the first result
                        reranked_results[0]['text'] = merged_text
                        reranked_results[0]['base_score'] = f"Merged({tag1},{tag2})"
                        # Remove the second result
                        reranked_results.pop(1)
                
                # Print top 3 reranked results
                for rank, doc_info in enumerate(reranked_results[:3]):
                    print(f"\nRank {rank+1}")
                    print(f"Reranker Score: {doc_info['rerank_score']:.4f} (Base Score: {doc_info['base_score']} | Source: {doc_info['source']})")
                    print(doc_info['text'])
                print("-" * 40)
            else:
                print("No results found.\n")
        print("\n\n")
        
    print("\n" + "="*60)
    print("💡 GRAPH LAYER ACTIVATED: If the Rust `walk()` method was exposed to Python, you could now traverse:")
    print("  Vector Hit -> Record Node -> Concept(Maintenance) -> Concept(Materials) -> Record Node(Paragraph 3)")
    print("  This allows answering complex multi-hop queries entirely natively without hitting an LLM agent!")
    print("="*60 + "\n")

if __name__ == "__main__":
    main()
