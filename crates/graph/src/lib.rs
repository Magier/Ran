//! **cortex** — knowledge graph for the Ran campaign engine.
//!
//! Backed by `petgraph::StableGraph`, this crate provides a directed multigraph
//! of [`EntityId`] nodes and [`EdgeData`] edges with weighted shortest-path
//! queries and exec-channel reachability analysis.

pub mod edge;
pub mod graph;

pub use edge::{edge_data_for, relation_defaults, EdgeData};
pub use graph::KnowledgeGraph;
