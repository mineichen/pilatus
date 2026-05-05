# Project Instructions for opencode

## Session Initialization

When starting a session in this project, always load and use the graphify-rs code graph information located in `graphify-out/`:

1. Read `graphify-out/GRAPH_REPORT.md` to understand the codebase structure, including:
   - Node and edge counts
   - God nodes (most connected components)
   - Community structure
   - Surprising connections

2. Reference `graphify-out/.graphify_manifest.json` to understand which files are indexed in the graph.

3. Use `graphify-out/graph.json` for detailed code graph queries when analyzing relationships between components.

4. The interactive graph visualization is available at `graphify-out/graph.html` for reference.

## Code Graph Summary

This Rust workspace contains multiple crates (pilatus, pilatus-rt, pilatus-axum, pilatus-engineering, etc.). The graphify-rs analysis provides a comprehensive map of function calls, type relationships, and module dependencies across the entire codebase.
