# Copyright (c) 2025 Varshith Gudur. Licensed under AGPLv3.
# Node kinds matching Rust/Semantic layer
NODE_RECORD = 0
NODE_CONCEPT = 1
NODE_AGENT = 2
NODE_USER = 3
NODE_TOOL = 4
NODE_DOCUMENT = 5       # new semantic
NODE_CHUNK = 6          # new semantic

# Edge kinds matching Rust/Semantic layer
EDGE_RELATION = 0
EDGE_FOLLOWS = 1
EDGE_IN_EPISODE = 2
EDGE_BY_AGENT = 3
EDGE_MENTIONS = 4
EDGE_REFERS_TO = 5
EDGE_PARENT_OF = 6      # parent-child, used for document->chunk
