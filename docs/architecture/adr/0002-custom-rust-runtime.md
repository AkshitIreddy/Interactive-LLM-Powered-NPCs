# ADR-0002: Use a custom Rust turn state machine

Status: Accepted  
Date: 2026-08-28

## Context

The live workflow is bounded and latency-critical: listen, transcribe, identify/retrieve, stream a reply, synthesize clauses, play, animate, cancel and commit delivered history. Version 1 uses LangChain around one Cohere prompt, but its failure is not lack of an agent graph; it is synchronous orchestration, implicit state and weak process/data boundaries.

## Decision

Implement the runtime as explicit Tokio state machines for application, session, turn, provider request and worker lifecycle. Do not make LangChain, LangGraph or another agent framework part of the production runtime. Provider adapters implement project-owned traits; schemas and Protobuf are owned by this repository.

## Consequences

- Deadlines, cancellation generations, backpressure and commit semantics remain visible and testable.
- Provider/runtime upgrades cannot redefine product state.
- The project owns more orchestration code and must test every transition/failure.
- Python remains allowed only inside version-pinned inference packs, not orchestration.

## Reconsider when

A framework may be adopted only if a measured prototype reduces complexity without adding latency, hidden persistence, provider coupling, Python/system-runtime requirements or weaker cancellation/typing. Feature count alone is not evidence.

