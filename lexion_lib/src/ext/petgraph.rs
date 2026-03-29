use petgraph::{algo, visit};

pub trait PetgraphExt<G: visit::IntoNeighbors + visit::Visitable> {
    #[allow(clippy::wrong_self_convention)]
    fn is_node_in_cycle(self, node: G::NodeId) -> bool;
}

impl<G: visit::IntoNeighbors + visit::Visitable> PetgraphExt<G> for G {
    fn is_node_in_cycle(self, node: G::NodeId) -> bool {
        let mut space = algo::DfsSpace::new(self);
        for neighbour in self.neighbors(node) {
            if algo::has_path_connecting(self, neighbour, node, Some(&mut space)) {
                return true;
            }
        }
        false
    }
}
