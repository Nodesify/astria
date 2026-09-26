// Package graph provides a simple directed graph implementation
// with topological sorting and cycle detection.
package graph

import (
	"errors"
	"fmt"
)

// ErrCycleDetected is returned when a topological sort encounters a cycle.
var ErrCycleDetected = errors.New("graph contains a cycle")

// Node represents a vertex in the directed graph.
type Node struct {
	ID    string
	Label string
}

// Edge represents a directed connection from Source to Target.
type Edge struct {
	Source string
	Target string
	Weight float64
}

// Graph is a directed graph backed by adjacency lists.
type Graph struct {
	nodes map[string]*Node
	edges map[string][]Edge // keyed by source node ID
}

// NewGraph creates an empty directed graph.
func NewGraph() *Graph {
	return &Graph{
		nodes: make(map[string]*Node),
		edges: make(map[string][]Edge),
	}
}

// AddNode inserts a new node. Returns an error if the ID already exists.
func (g *Graph) AddNode(id, label string) error {
	if _, exists := g.nodes[id]; exists {
		return fmt.Errorf("node already exists: %s", id)
	}
	g.nodes[id] = &Node{ID: id, Label: label}
	return nil
}

// AddEdge creates a directed edge from src to dst.
// WHY: We validate both endpoints exist before adding the edge so that
// downstream traversal never needs to handle dangling references.
func (g *Graph) AddEdge(src, dst string, weight float64) error {
	if _, ok := g.nodes[src]; !ok {
		return fmt.Errorf("source node not found: %s", src)
	}
	if _, ok := g.nodes[dst]; !ok {
		return fmt.Errorf("target node not found: %s", dst)
	}
	g.edges[src] = append(g.edges[src], Edge{Source: src, Target: dst, Weight: weight})
	return nil
}

// Neighbors returns all edges outgoing from the given node.
func (g *Graph) Neighbors(nodeID string) []Edge {
	return g.edges[nodeID]
}

// TopologicalSort returns an ordering of nodes such that every edge
// goes from an earlier to a later node in the slice.
func (g *Graph) TopologicalSort() ([]string, error) {
	// NOTE: Kahn's algorithm is used instead of DFS-based sort because
	// it naturally detects cycles without an extra pass.
	inDegree := make(map[string]int)
	for id := range g.nodes {
		inDegree[id] = 0
	}
	for _, edges := range g.edges {
		for _, e := range edges {
			inDegree[e.Target]++
		}
	}

	var queue []string
	for id, deg := range inDegree {
		if deg == 0 {
			queue = append(queue, id)
		}
	}

	var order []string
	for len(queue) > 0 {
		current := queue[0]
		queue = queue[1:]
		order = append(order, current)
		for _, e := range g.edges[current] {
			inDegree[e.Target]--
			if inDegree[e.Target] == 0 {
				queue = append(queue, e.Target)
			}
		}
	}

	if len(order) != len(g.nodes) {
		return nil, ErrCycleDetected
	}
	return order, nil
}

// Describe returns a human-readable summary of the graph.
func (g *Graph) Describe() string {
	return fmt.Sprintf("Graph(%d nodes, %d edges)", len(g.nodes), g.countEdges())
}

// countEdges returns the total number of directed edges.
// HACK: Iterating over the adjacency map each time is O(E). Acceptable
// for diagnostics but should not be called in a hot loop.
func (g *Graph) countEdges() int {
	total := 0
	for _, edges := range g.edges {
		total += len(edges)
	}
	return total
}
