use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct Graph {
    pub adjacency: HashMap<String, Vec<String>>,
}

pub struct TopoSortResult {
    pub order: Vec<String>,
}

impl Graph {
    pub fn new() -> Self {
        Self {
            adjacency: HashMap::new(),
        }
    }

    pub fn add_node(&mut self, name: &str) {
        self.adjacency.entry(name.to_string()).or_default();
    }

    pub fn add_dep(&mut self, node: &str, dep: &str) {
        self.add_node(node);
        self.add_node(dep);
        let deps = self.adjacency.get_mut(node).unwrap();
        if !deps.contains(&dep.to_string()) {
            deps.push(dep.to_string());
        }
    }

    pub fn topological_sort(&self) -> Result<TopoSortResult, String> {
        let mut visited = HashSet::new();
        let mut in_stack = HashSet::new();
        let mut order = Vec::new();

        let all_nodes: Vec<String> = self.adjacency.keys().cloned().collect();
        for node in &all_nodes {
            if !visited.contains(node) {
                self.dfs(node, &mut visited, &mut in_stack, &mut order)?;
            }
        }

        Ok(TopoSortResult { order })
    }

    fn dfs(
        &self,
        node: &str,
        visited: &mut HashSet<String>,
        in_stack: &mut HashSet<String>,
        order: &mut Vec<String>,
    ) -> Result<(), String> {
        if in_stack.contains(node) {
            let cycle: Vec<&str> = in_stack.iter().map(|s| s.as_str()).collect();
            return Err(format!(
                "Cycle detected: {} is part of a cycle involving: {:?}",
                node, cycle
            ));
        }
        if visited.contains(node) {
            return Ok(());
        }

        in_stack.insert(node.to_string());
        visited.insert(node.to_string());

        if let Some(deps) = self.adjacency.get(node) {
            for dep in deps {
                self.dfs(dep, visited, in_stack, order)?;
            }
        }

        in_stack.remove(node);
        order.push(node.to_string());
        Ok(())
    }

    pub fn dot_format(&self) -> String {
        let mut out = String::from("digraph DAG {\n");
        for (node, deps) in &self.adjacency {
            for dep in deps {
                out.push_str(&format!("  \"{}\" -> \"{}\";\n", dep, node));
            }
        }
        out.push_str("}\n");
        out
    }
}
