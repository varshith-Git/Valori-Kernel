# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from typing import List

def split_by_sentences(text: str, max_chars: int = 512) -> List[str]:
    """
    Deterministic, simple chunker:
    - Split on '.', '?', '!'
    - Each sentence normally becomes its own chunk.
    - If a single sentence is longer than max_chars, we hard-split it into pieces.
    """
    if not text:
        return []

    delimiters = {'.', '?', '!'}
    sentences: List[str] = []
    current = ""

    for ch in text:
        current += ch
        if ch in delimiters:
            sent = current.strip()
            if sent:
                sentences.append(sent)
            current = ""
    # trailing text without punctuation
    if current.strip():
        sentences.append(current.strip())

    chunks: List[str] = []
    for sent in sentences:
        if len(sent) <= max_chars:
            chunks.append(sent)
        else:
            # Hard-split very long sentence
            start = 0
            while start < len(sent):
                chunks.append(sent[start:start + max_chars].strip())
                start += max_chars

    return chunks

def naive_paragraph_chunker(text: str, max_chars: int = 512) -> List[str]:
    """
    Split on double newlines, then further break long paragraphs into max_chars chunks 
    (using sentence splitter for sub-chunking if needed, or simple char slice).
    """
    paragraphs = [p.strip() for p in text.split('\n\n') if p.strip()]
    chunks = []
    
    for para in paragraphs:
        if len(para) <= max_chars:
            chunks.append(para)
        else:
            # Recursively use sentence splitter for big paragraphs
            sub_chunks = split_by_sentences(para, max_chars)
            chunks.extend(sub_chunks)
            
    return chunks
