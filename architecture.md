# Valori Kernel Architecture

This document outlines the high-level architecture of the `valori-kernel` project, illustrating how the core deterministic logic interacts with external interfaces like the HTTP server (`valori-node`) and Python bindings (`valori-ffi`).

## System Overview

The system is built as a layered architecture with the **Valori Kernel** at its center. This kernel is a pure, deterministic state machine that can be embedded into various runtimes.

```mermaid
graph TD
    %% Style Definitions
    classDef external fill:#e1f5fe,stroke:#01579b,stroke-width:2px,color:#01579b;
    classDef interface fill:#e8f5e9,stroke:#2e7d32,stroke-width:2px,color:#2e7d32;
    classDef core fill:#fff3e0,stroke:#e65100,stroke-width:4px,color:#e65100;
    classDef internal fill:#fff8e1,stroke:#ff6f00,stroke-width:1px,stroke-dasharray: 5 5,color:#ff6f00;

    subgraph External["External Usage"]
        User[User / Client]
        PyScript[Python Scripts / Notebooks]
    end

    subgraph Interface["Interface Layer"]
        NodeService["Values Node (HTTP Server)<br/>(Tokio / Axum)"]
        PythonPkg["Python Package<br/>(valori)"]
        
        subgraph Protocol["Valori Memory Protocol (VMP)"]
            VMP_Py[ProtocolClient / MemoryClient]
            VMP_API[Axum Handlers (/v1/memory)]
        end
    end

    subgraph Core["Core Kernel (Rust)"]
        Kernel["Valori Kernel<br/>(no_std, Deterministic)"]
        
        FXP[Fixed-Point Math]
        Graph[Knowledge Graph]
        Vector[Vector Storage]
        
        subgraph Abstractions["Traits"]
            Index[VectorIndex<br/>(BruteForce / HNSW*)]
            Quant[Quantizer<br/>(None / Scalar*)]
        end
    end

    %% Relationships
    User -->|HTTP / REST| NodeService
    PyScript -->|Import| PythonPkg
    
    PythonPkg --> VMP_Py
    NodeService --> VMP_API
    
    VMP_Py -->|FFI| Kernel
    VMP_Py -->|HTTP| NodeService
    
    NodeService -->|Embeds| Kernel
    VMP_API --> Kernel

    Kernel --- FXP
    Kernel --- Graph
    Kernel --- Vector
    Kernel --- Index
    Kernel --- Quant

    %% Apply Classes
    class User,PyScript external;
    class NodeService,PythonPkg,VMP_Py,VMP_API interface;
    class Kernel core;
    class FXP,Graph,Vector,Index,Quant internal;
```

## Core Components

### 1. Valori Kernel (Bottom Layer)
The foundation of the system.
*   **`no_std` Rust**: Capable of running in embedded environments, WASM, or standard OS processes without relying on the standard library.
*   **Fixed-Point Arithmetic (FXP)**: All numeric operations use fixed-point integers (e.g., Q16.16) instead of floating-point numbers (`f32`/`f64`).
    *   **Why?** Floating-point math can vary slightly across different CPU architectures and compiler optimizations (e.g. FMA instructions).
    *   **Benefit**: This guarantees **Bit-Identical Determinism**. The same sequence of inputs will produce the exact same binary state hash on an Intel laptop, an ARM server, or a WASM browser runtime.
*   **Abstractions (Traits)**:
    *   **`VectorIndex`**: Pluggable indexing strategy. Default is `BruteForceIndex` (exact). Future support for `HNSW` or `IVF`.
    *   **`Quantizer`**: Pluggable vector compression. Default is `NoQuantizer` (full precision). Future support for `Scalar` or `Product` quantization.
*   **State Machine**: The kernel operates as a pure state machine (`State + Command -> New State`).

### 2. Valori Node (Service Layer)
*   Wraps the kernel in an **HTTP Server** using `axum` and `tokio`.
*   **Configurable**: Loads `IndexKind` and `QuantizationKind` from configuration at startup.
*   **VMP Support**: Implements the **Valori Memory Protocol v0** (`/v1/memory/*`), handling high-level operations like creating Document/Chunk nodes and linking them automatically.

### 3. FFI & Python Client
*   **`valori` Package**: A unified Python client that can talk to a local FFI kernel OR a remote HTTP Node transparently.
*   **Protocol Layer**: `ProtocolClient` orchestrates complex flows like text chunking and embedding before sending vectors to the memory layer.

## Why Determinism Matters

In distributed systems and AI memory, reproducibility is critical.

*   **Snapshot & Restore**: Because the state is deterministic, you can take a snapshot of the kernel's memory, move it to another machine, and replay a log of commands to reach the exact same state.
*   **Verification**: You can verify the integrity of the knowledge graph by hashing its state. If two nodes replay the same history, their hashes must match perfectly.
*   **Cross-Platform**: A model trained or a graph built on a Linux server will behave identical on a user's MacBook or edge device.
