//! `graph([directed=false])` (§ data structures pass, 2026-09-01) — a real
//! node+edge graph structure, not a chart.
//!
//! Node ids are normalized to a canonical `String` key the same way
//! `Value::Dict`'s own keys already are (`dict_key` in `collections.rs`):
//! a string or number only, numbers canonicalized via `fmt_num` (the same
//! formatter `display_value` uses), so `add_node(1)` and `add_node(1.0)`
//! name the SAME node. `neighbors(...)` always hands back `Value::Str` ids
//! regardless of what kind of value the caller originally passed in —
//! exactly mirroring `dict_keys`'s own "keys always come back as strings"
//! contract, for the same reason (a normalized key has already forgotten
//! its original representation, so there is nothing else honest to return).
//!
//! One real, correctly-verified algorithm proves this is more than a
//! container: `.shortest_path(source, target)`, Dijkstra's algorithm.
//! Dijkstra (over plain unweighted BFS) was picked specifically because
//! `.add_edge` carries a real optional `weight=` — a weighted-shortest-path
//! algorithm is the one that actually exercises that part of the data
//! model, where a pure hop-count BFS would leave `weight` write-only.
//! Implemented as a simple O(V^2) "repeatedly scan for the closest
//! unvisited node" (no binary heap) — correct, and plenty fast for the
//! small, in-script graphs this is for; a priority-queue-based O(E log V)
//! version is a straightforward, purely-internal follow-up if a script
//! ever builds a graph large enough for the difference to matter. See the
//! acceptance test `graph_shortest_path_matches_hand_computed_dijkstra_distance`
//! for the hand-checked verification this pass's own brief asked for.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex};

use crate::{arg0, e, fmt_num, style_num, truthy, EvalError, Value, R};

pub struct GraphState {
    directed: bool,
    /// Insertion order, for deterministic iteration/`neighbors` output —
    /// same "an ordered `Vec`, not just a `HashMap`, for deterministic
    /// order" reasoning `Value::Dict`/`Value::Record` already use.
    nodes: Vec<String>,
    adj: HashMap<String, Vec<(String, f64)>>,
}

impl GraphState {
    /// Used by `lib.rs`'s `value_len` so `len(g)`/`length(g)`/`numel(g)`
    /// report the node count, with no graph-specific `"len"` match arm
    /// needed — same convention `fifo::FifoState::len` uses.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Used by `lib.rs`'s `display_value`.
    pub fn edge_count(&self) -> usize {
        self.adj.values().map(|edges| edges.len()).sum()
    }

    /// Used by `lib.rs`'s `display_value`.
    pub fn directed(&self) -> bool {
        self.directed
    }
}

impl std::fmt::Debug for GraphState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphState")
            .field("directed", &self.directed)
            .field("nodes", &self.nodes.len())
            .finish()
    }
}

fn as_graph(v: &Value) -> R<&Arc<StdMutex<GraphState>>> {
    match v {
        Value::Graph(g) => Ok(g),
        other => e(format!("expected a graph (from graph()), found {}", other.type_name())),
    }
}

/// Normalizes a node-id argument to its canonical `String` key — the exact
/// same rule `collections.rs`'s own `dict_key` uses (string or number
/// only; anything else is a clear, named error).
fn node_key(v: &Value) -> R<String> {
    match v {
        Value::Str(s) => Ok(s.clone()),
        Value::Num(n) => Ok(fmt_num(*n)),
        other => e(format!("graph: node ids must be strings or numbers, found {}", other.type_name())),
    }
}

fn ensure_node(st: &mut GraphState, key: &str) {
    if !st.adj.contains_key(key) {
        st.nodes.push(key.to_string());
        st.adj.insert(key.to_string(), Vec::new());
    }
}

/// `graph([directed=false])`.
fn new(style: &[(String, Value)]) -> R<Value> {
    let directed = crate::style_entry(style, "directed").map(|(_, v)| truthy(v)).unwrap_or(false);
    Ok(Value::Graph(Arc::new(StdMutex::new(GraphState {
        directed,
        nodes: Vec::new(),
        adj: HashMap::new(),
    }))))
}

/// `.add_node(id)` — idempotent: adding an already-present node is a no-op,
/// not an error (matches `mkdir`'s own "assert this exists" convention
/// rather than a strict "error if already present" one).
fn add_node(args: &[Value]) -> R<Value> {
    let g = as_graph(arg0(args)?)?;
    let key = node_key(args.get(1).ok_or_else(|| EvalError { msg: "add_node(g, id) needs a node id".into() })?)?;
    let mut st = g.lock().unwrap();
    ensure_node(&mut st, &key);
    Ok(Value::Nothing)
}

/// `.add_edge(a, b, [weight=1.0])` — auto-creates either endpoint that
/// doesn't already exist (a script building a graph edge-by-edge shouldn't
/// be forced to call `add_node` first for every endpoint). An undirected
/// graph (the default) stores the edge in BOTH adjacency lists; a
/// self-loop (`a == b`) is stored once either way, avoiding a doubled
/// self-edge that `has_edge`/`shortest_path` would otherwise double-count.
fn add_edge(args: &[Value], style: &[(String, Value)]) -> R<Value> {
    let g = as_graph(arg0(args)?)?;
    let a = node_key(args.get(1).ok_or_else(|| EvalError {
        msg: "add_edge(g, a, b, [weight=]) needs node a".into(),
    })?)?;
    let b = node_key(args.get(2).ok_or_else(|| EvalError {
        msg: "add_edge(g, a, b, [weight=]) needs node b".into(),
    })?)?;
    let weight = style_num(style, "weight").unwrap_or(1.0);
    let mut st = g.lock().unwrap();
    ensure_node(&mut st, &a);
    ensure_node(&mut st, &b);
    st.adj.get_mut(&a).unwrap().push((b.clone(), weight));
    if !st.directed && a != b {
        st.adj.get_mut(&b).unwrap().push((a, weight));
    }
    Ok(Value::Nothing)
}

/// `.neighbors(id)` — a `List` of neighbor ids (always `Value::Str`, per
/// this module's own doc comment), in the order their edges were added.
/// Errors if `id` isn't a node in the graph at all — distinguishing "no
/// neighbors" from "not a node", rather than silently returning an empty
/// list for both.
fn neighbors(args: &[Value]) -> R<Value> {
    let g = as_graph(arg0(args)?)?;
    let key = node_key(args.get(1).ok_or_else(|| EvalError { msg: "neighbors(g, id) needs a node id".into() })?)?;
    let st = g.lock().unwrap();
    match st.adj.get(&key) {
        Some(edges) => Ok(Value::List(Arc::new(edges.iter().map(|(n, _)| Value::Str(n.clone())).collect()))),
        None => e(format!("neighbors: `{key}` is not a node in this graph")),
    }
}

/// `.has_edge(a, b)` — `false` (not an error) if either endpoint isn't even
/// a node in the graph — a plain membership question, matching
/// `contains`'s own "absence is the answer, not exceptional" convention.
fn has_edge(args: &[Value]) -> R<Value> {
    let g = as_graph(arg0(args)?)?;
    let a = node_key(args.get(1).ok_or_else(|| EvalError { msg: "has_edge(g, a, b) needs node a".into() })?)?;
    let b = node_key(args.get(2).ok_or_else(|| EvalError { msg: "has_edge(g, a, b) needs node b".into() })?)?;
    let st = g.lock().unwrap();
    let found = st.adj.get(&a).map(|edges| edges.iter().any(|(n, _)| *n == b)).unwrap_or(false);
    Ok(Value::Bool(found))
}

/// `.shortest_path(source, target)` — Dijkstra's algorithm (see this
/// module's own doc comment for why Dijkstra, and why a simple O(V^2) scan
/// is the right scope here). Returns the total weighted distance as a
/// `Num`, or `Value::Nothing` if `target` is unreachable from `source`
/// (composes with `??`, the same "absent" convention used elsewhere in
/// this codebase). Errors if either endpoint isn't a node in the graph at
/// all, and if any edge along the way has a negative weight (Dijkstra's
/// own precondition — silently returning a wrong distance would be worse
/// than refusing).
fn shortest_path(args: &[Value]) -> R<Value> {
    let g = as_graph(arg0(args)?)?;
    let source = node_key(args.get(1).ok_or_else(|| EvalError {
        msg: "shortest_path(g, source, target) needs a source node".into(),
    })?)?;
    let target = node_key(args.get(2).ok_or_else(|| EvalError {
        msg: "shortest_path(g, source, target) needs a target node".into(),
    })?)?;
    let st = g.lock().unwrap();
    if !st.adj.contains_key(&source) {
        return e(format!("shortest_path: `{source}` is not a node in this graph"));
    }
    if !st.adj.contains_key(&target) {
        return e(format!("shortest_path: `{target}` is not a node in this graph"));
    }
    let mut dist: HashMap<&str, f64> = st.nodes.iter().map(|n| (n.as_str(), f64::INFINITY)).collect();
    dist.insert(source.as_str(), 0.0);
    let mut visited: HashSet<&str> = HashSet::new();
    loop {
        let closest = dist
            .iter()
            .filter(|(n, _)| !visited.contains(*n))
            .min_by(|(_, a), (_, b)| a.total_cmp(b))
            .map(|(n, d)| (*n, *d));
        let Some((u, du)) = closest else { break };
        if !du.is_finite() {
            break; // every remaining node is unreachable from `source`
        }
        visited.insert(u);
        if u == target.as_str() {
            break;
        }
        for (v, w) in st.adj.get(u).unwrap() {
            if *w < 0.0 {
                return e(format!(
                    "shortest_path: edge `{u}` -> `{v}` has a negative weight ({w}) — Dijkstra's \
                     algorithm requires non-negative edge weights"
                ));
            }
            let alt = du + w;
            let entry = dist.entry(v.as_str()).or_insert(f64::INFINITY);
            if alt < *entry {
                *entry = alt;
            }
        }
    }
    let d = *dist.get(target.as_str()).unwrap_or(&f64::INFINITY);
    if d.is_finite() {
        Ok(Value::Num(d))
    } else {
        Ok(Value::Nothing)
    }
}

/// Single dispatch entry point, called from `lib.rs`'s big builtin match —
/// see that match's own comment for why this is a combined arm. None of
/// these six names collide with any existing builtin, so every arm here is
/// unconditional.
pub fn call(f: &str, args: &[Value], style: &[(String, Value)]) -> R<Value> {
    match f {
        "graph" => new(style),
        "add_node" => add_node(args),
        "add_edge" => add_edge(args, style),
        "neighbors" => neighbors(args),
        "has_edge" => has_edge(args),
        "shortest_path" => shortest_path(args),
        other => e(format!("graph: internal dispatch error, unhandled `{other}`")),
    }
}
