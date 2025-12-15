# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
from typing import Optional, List
import os
from .chunking import naive_paragraph_chunker

def load_text_from_file(path: str) -> str:
    """
    - If extension is .txt: read as UTF-8.
    - If extension is .md: read raw.
    - If extension is .pdf:
        - Try to import pypdf.
        - If missing, raise a clear ImportError telling user to install pypdf.
        - Extract text from all pages, join with \n\n.
    """
    _, ext = os.path.splitext(path)
    ext = ext.lower()

    if ext in ['.txt', '.md']:
        with open(path, 'r', encoding='utf-8') as f:
            return f.read()
    
    elif ext == '.pdf':
        try:
            import pypdf
        except ImportError:
            raise ImportError("pypdf is required to load PDF files. Please install it via `pip install pypdf`.")
        
        reader = pypdf.PdfReader(path)
        text_parts = []
        for page in reader.pages:
            text = page.extract_text()
            if text:
                text_parts.append(text)
        
        return "\n\n".join(text_parts)

    else:
        raise ValueError(f"Unsupported file extension: {ext}")

def chunk_text(text: str, max_chars: int = 512) -> List[str]:
    """
    Use the paragraph chunker from the chunking module.
    Deterministic behavior.
    """
    return naive_paragraph_chunker(text, max_chars)
