//! [`KnowledgeGraph`] — directed multigraph of [`EntityId`] nodes.

use std::collections::HashMap;

use petgraph::algo::astar;
use petgraph::stable_graph::{EdgeIndex, NodeIndex, StableGraph};
use petgraph::visit::{EdgeFiltered, EdgeRef};
use petgraph::Direction;
use ran_domain::{EntityId, RelationSummary};

use crate::edge::EdgeData;

/// Directed multigraph of [`EntityId`] nodes and [`EdgeData`] edges.
///
/// Nodes carry only entity identity; entity *data* lives in the campaign's
/// typed maps.  The graph owns topology and edge metadata only.
///
/// ## Invariants enforced on insert
///
/// - **NoSelfEdge** — source and target must differ (hard reject).
/// - **PodSingleNode** — a pod may carry at most one `runs-on` edge; an
///   incoming one replaces the old one (K8s rescheduling is valid).
#[derive(Debug, Clone)]
pub struct KnowledgeGraph {
    pub(crate) graph: StableGraph<EntityId, EdgeData>,
    /// `O(1)` lookup from entity id to its node index.
    pub(crate) index: HashMap<EntityId, NodeIndex>,
}

impl Default for KnowledgeGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self {
            graph: StableGraph::new(),
            index: HashMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Node management
    // -----------------------------------------------------------------------

    /// Ensure a node for `id` exists. Returns the `NodeIndex` (new or existing).
    pub fn ensure_node(&mut self, id: EntityId) -> NodeIndex {
        if let Some(&idx) = self.index.get(&id) {
            return idx;
        }
        let idx = self.graph.add_node(id.clone());
        self.index.insert(id, idx);
        idx
    }

    /// Remove an entity and all edges connected to it.
    pub fn remove_entity(&mut self, id: &EntityId) {
        if let Some(idx) = self.index.remove(id) {
            self.graph.remove_node(idx);
        }
    }

    /// Merge `discard` into `keep`: retarget every edge that touched `discard`
    /// so it now touches `keep` instead.  Duplicate edges (same relation name)
    /// are dropped.  Entity *data* merging is the caller's responsibility.
    pub fn merge_entities(&mut self, keep: &EntityId, discard: &EntityId) {
        let (Some(&keep_idx), Some(&discard_idx)) = (self.index.get(keep), self.index.get(discard))
        else {
            return;
        };
        if keep_idx == discard_idx {
            return;
        }

        // Snapshot edges before mutation — can't borrow mutably and immutably
        // at the same time.
        let outgoing: Vec<(NodeIndex, EdgeData)> = self
            .graph
            .edges_directed(discard_idx, Direction::Outgoing)
            .map(|e| (e.target(), e.weight().clone()))
            .collect();

        let incoming: Vec<(NodeIndex, EdgeData)> = self
            .graph
            .edges_directed(discard_idx, Direction::Incoming)
            .map(|e| (e.source(), e.weight().clone()))
            .collect();

        // Remove the discard node (also removes all its edges).
        self.graph.remove_node(discard_idx);
        self.index.remove(discard);

        // Re-insert edges, replacing discard with keep.
        for (tgt, data) in outgoing {
            // tgt == discard_idx was a self-edge on discard; skip.
            // After removal the index is gone, so we can only check by value —
            // use the fact that the node was just removed (its weight is gone).
            if self.graph.node_weight(tgt).is_none() {
                continue;
            }
            if !self.has_edge_between(keep_idx, tgt, &data.relation_name) {
                self.graph.add_edge(keep_idx, tgt, data);
            }
        }

        for (src, data) in incoming {
            if self.graph.node_weight(src).is_none() {
                continue;
            }
            if !self.has_edge_between(src, keep_idx, &data.relation_name) {
                self.graph.add_edge(src, keep_idx, data);
            }
        }
    }

    // -----------------------------------------------------------------------
    // Edge management
    // -----------------------------------------------------------------------

    /// Insert a directed edge `src → tgt`.  Creates missing nodes as a side-effect.
    ///
    /// Invariants enforced:
    /// - **NoSelfEdge**: returns `None` if `src == tgt`.
    /// - **PodSingleNode**: any existing `runs-on` edge from `src` is removed
    ///   before the new one is added (K8s rescheduling is valid).
    ///
    /// Parallel edges (same `src`/`tgt`, different `relation_name`) are allowed.
    pub fn insert_edge(
        &mut self,
        src: &EntityId,
        tgt: &EntityId,
        data: EdgeData,
    ) -> Option<EdgeIndex> {
        if src == tgt {
            return None;
        }

        let src_idx = self.ensure_node(src.clone());
        let tgt_idx = self.ensure_node(tgt.clone());

        // PodSingleNode: replace any existing runs-on from this source.
        if data.relation_name == "runs-on" {
            let stale: Vec<EdgeIndex> = self
                .graph
                .edges_directed(src_idx, Direction::Outgoing)
                .filter(|e| e.weight().relation_name == "runs-on")
                .map(|e| e.id())
                .collect();
            for idx in stale {
                self.graph.remove_edge(idx);
            }
        }

        Some(self.graph.add_edge(src_idx, tgt_idx, data))
    }

    /// Remove all edges from `src` to `tgt` with the given relation name.
    pub fn remove_edges(&mut self, src: &EntityId, tgt: &EntityId, relation_name: &str) {
        let (Some(&si), Some(&ti)) = (self.index.get(src), self.index.get(tgt)) else {
            return;
        };
        let to_remove: Vec<EdgeIndex> = self
            .graph
            .edges_connecting(si, ti)
            .filter(|e| e.weight().relation_name == relation_name)
            .map(|e| e.id())
            .collect();
        for idx in to_remove {
            self.graph.remove_edge(idx);
        }
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Whether `id` has a node in the graph (entity may or may not have data).
    pub fn contains(&self, id: &EntityId) -> bool {
        self.index.contains_key(id)
    }

    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Outgoing edges from `id` as `(target_id, &EdgeData)` pairs.
    pub fn outgoing(&self, id: &EntityId) -> Vec<(&EntityId, &EdgeData)> {
        let Some(&idx) = self.index.get(id) else {
            return Vec::new();
        };
        self.graph
            .edges_directed(idx, Direction::Outgoing)
            .filter_map(|e| Some((self.graph.node_weight(e.target())?, e.weight())))
            .collect()
    }

    /// Incoming edges to `id` as `(source_id, &EdgeData)` pairs.
    pub fn incoming(&self, id: &EntityId) -> Vec<(&EntityId, &EdgeData)> {
        let Some(&idx) = self.index.get(id) else {
            return Vec::new();
        };
        self.graph
            .edges_directed(idx, Direction::Incoming)
            .filter_map(|e| Some((self.graph.node_weight(e.source())?, e.weight())))
            .collect()
    }

    /// All entities with a `relation_name` edge pointing at `id`.
    pub fn sources_of(&self, id: &EntityId, relation_name: &str) -> Vec<&EntityId> {
        self.incoming(id)
            .into_iter()
            .filter(|(_, d)| d.relation_name == relation_name)
            .map(|(src, _)| src)
            .collect()
    }

    /// All entities pointed to by `relation_name` edges originating at `id`.
    pub fn targets_of(&self, id: &EntityId, relation_name: &str) -> Vec<&EntityId> {
        self.outgoing(id)
            .into_iter()
            .filter(|(_, d)| d.relation_name == relation_name)
            .map(|(tgt, _)| tgt)
            .collect()
    }

    /// All exec-channel edges in the graph as `(source_id, target_id, &EdgeData)`.
    pub fn exec_edges(&self) -> Vec<(&EntityId, &EntityId, &EdgeData)> {
        self.graph
            .edge_indices()
            .filter_map(|ei| {
                let data = self.graph.edge_weight(ei)?;
                if !data.is_exec_channel {
                    return None;
                }
                let (si, ti) = self.graph.edge_endpoints(ei)?;
                Some((
                    self.graph.node_weight(si)?,
                    self.graph.node_weight(ti)?,
                    data,
                ))
            })
            .collect()
    }

    /// All entity IDs reachable from any of `seeds` following exec-channel edges
    /// (BFS, seeds themselves are not included in the result).
    pub fn reachable_via_exec(&self, seeds: &[EntityId]) -> Vec<EntityId> {
        let mut visited: std::collections::HashSet<NodeIndex> = std::collections::HashSet::new();
        let mut queue: std::collections::VecDeque<NodeIndex> = std::collections::VecDeque::new();

        for seed in seeds {
            if let Some(&idx) = self.index.get(seed) {
                if visited.insert(idx) {
                    queue.push_back(idx);
                }
            }
        }

        let mut result = Vec::new();
        while let Some(nx) = queue.pop_front() {
            for edge in self.graph.edges_directed(nx, Direction::Outgoing) {
                if !edge.weight().is_exec_channel {
                    continue;
                }
                let tgt = edge.target();
                if visited.insert(tgt) {
                    queue.push_back(tgt);
                    if let Some(id) = self.graph.node_weight(tgt) {
                        if !seeds.iter().any(|s| s == id) {
                            result.push(id.clone());
                        }
                    }
                }
            }
        }

        result
    }

    /// Shortest weighted exec-channel path from any of `seeds` to `target`.
    ///
    /// Uses Dijkstra (A* with zero heuristic) over exec-channel edges only.
    /// Returns `(total_cost, path)` where `path` is `[seed, …, target]` inclusive,
    /// or `None` if the target is unreachable from every seed.
    pub fn shortest_exec_path(
        &self,
        seeds: &[EntityId],
        target: &EntityId,
    ) -> Option<(f32, Vec<EntityId>)> {
        let &target_idx = self.index.get(target)?;

        let exec_graph = EdgeFiltered::from_fn(
            &self.graph,
            |e: petgraph::stable_graph::EdgeReference<EdgeData>| e.weight().is_exec_channel,
        );

        let mut best: Option<(f32, Vec<NodeIndex>)> = None;

        for seed in seeds {
            let Some(&seed_idx) = self.index.get(seed) else {
                continue;
            };

            let result = astar(
                &exec_graph,
                seed_idx,
                |n| n == target_idx,
                |e| e.weight().weight,
                |_| 0.0f32,
            );

            if let Some((cost, path)) = result {
                if best.as_ref().is_none_or(|(c, _)| cost < *c) {
                    best = Some((cost, path));
                }
            }
        }

        best.map(|(cost, path)| {
            let ids = path
                .iter()
                .filter_map(|&nx| self.graph.node_weight(nx).cloned())
                .collect();
            (cost, ids)
        })
    }

    // -----------------------------------------------------------------------
    // Serialization view
    // -----------------------------------------------------------------------

    /// Snapshot all edges as [`RelationSummary`] values for serialization.
    pub fn to_relation_summaries(&self) -> Vec<RelationSummary> {
        self.graph
            .edge_indices()
            .filter_map(|ei| {
                let (si, ti) = self.graph.edge_endpoints(ei)?;
                let data = self.graph.edge_weight(ei)?;
                let src = self.graph.node_weight(si)?;
                let tgt = self.graph.node_weight(ti)?;
                Some(RelationSummary {
                    name: data.relation_name.clone(),
                    source_id: src.0.clone(),
                    target_id: tgt.0.clone(),
                    is_exec_channel: data.is_exec_channel,
                    envelope: data.envelope.clone(),
                    output_transform: data.output_transform.clone(),
                    weight: data.weight,
                })
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // Private helpers
    // -----------------------------------------------------------------------

    fn has_edge_between(&self, src: NodeIndex, tgt: NodeIndex, relation_name: &str) -> bool {
        self.graph
            .edges_connecting(src, tgt)
            .any(|e| e.weight().relation_name == relation_name)
    }
}
