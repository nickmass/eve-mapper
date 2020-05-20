use petgraph::Graph;
use std::collections::HashMap;

mod esi;
mod font;
mod gfx;
pub(crate) mod math;
pub(crate) mod shaders;

#[derive(Debug)]
enum Edge {
    Warp {
        system: SystemIndex,
        distance: f64,
    },
    JumpBridge {
        left: SystemIndex,
        right: SystemIndex,
    },
    Wormhole {
        system: SystemIndex,
        wormhole: SystemIndex,
    },
    Jump {
        left: SystemIndex,
        right: SystemIndex,
    },
}

impl Edge {
    fn distance(&self) -> f64 {
        match self {
            Edge::Warp { distance, .. } => 1e3 - distance,
            Edge::Jump { .. } => (2.0f64).powi(30),
            Edge::JumpBridge { .. } => (2.0f64).powi(31),
            Edge::Wormhole { .. } => (2.0f64).powi(32),
        }
    }
}

impl std::fmt::Display for Edge {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Edge::Warp { .. } => write!(f, "Warp"),
            Edge::Jump { .. } => write!(f, "Jump"),
            Edge::JumpBridge { .. } => write!(f, "Jump Bridge"),
            Edge::Wormhole { .. } => write!(f, "Wormhole"),
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct SystemIndex(usize);

pub struct SystemCollection {
    systems: Vec<esi::universe::GetUniverseSystem>,
    map_by_name: HashMap<String, SystemIndex>,
    map_by_id: HashMap<i32, SystemIndex>,
}

impl SystemCollection {
    pub fn new() -> Self {
        SystemCollection {
            systems: Vec::new(),
            map_by_name: HashMap::new(),
            map_by_id: HashMap::new(),
        }
    }

    fn push(&mut self, system: esi::universe::GetUniverseSystem) -> SystemIndex {
        let idx = SystemIndex(self.systems.len());
        self.map_by_id.insert(system.system_id, idx);
        self.map_by_name.insert(system.name.clone(), idx);
        self.systems.push(system);
        idx
    }

    pub fn iter(&self) -> impl Iterator<Item = &esi::universe::GetUniverseSystem> {
        self.systems.iter()
    }

    fn by_id(&self, system_id: i32) -> Option<&esi::universe::GetUniverseSystem> {
        self.by_id_idx(system_id).and_then(|idx| self.by_idx(idx))
    }

    fn by_id_idx(&self, system_id: i32) -> Option<SystemIndex> {
        self.map_by_id.get(&system_id).cloned()
    }

    fn by_name(&self, name: &str) -> Option<&esi::universe::GetUniverseSystem> {
        self.by_name_idx(name).and_then(|idx| self.by_idx(idx))
    }

    fn by_name_idx(&self, name: &str) -> Option<SystemIndex> {
        self.map_by_name.get(name).cloned()
    }

    fn by_idx(&self, idx: SystemIndex) -> Option<&esi::universe::GetUniverseSystem> {
        self.systems.get(idx.0)
    }
}

#[derive(Copy, Clone, Debug)]
pub struct StargateIndex(usize);

pub struct StargateCollection {
    stargates: Vec<esi::universe::GetUniverseStargate>,
    map_by_id: HashMap<i32, StargateIndex>,
}

impl StargateCollection {
    pub fn new() -> Self {
        StargateCollection {
            stargates: Vec::new(),
            map_by_id: HashMap::new(),
        }
    }

    fn push(&mut self, stargate: esi::universe::GetUniverseStargate) -> StargateIndex {
        let idx = StargateIndex(self.stargates.len());
        self.map_by_id.insert(stargate.stargate_id, idx);
        self.stargates.push(stargate);
        idx
    }

    fn by_id(&self, id: i32) -> Option<&esi::universe::GetUniverseStargate> {
        self.by_id_idx(id).and_then(|idx| self.by_idx(idx))
    }

    fn by_id_idx(&self, id: i32) -> Option<StargateIndex> {
        self.map_by_id.get(&id).cloned()
    }

    fn by_idx(&self, idx: StargateIndex) -> Option<&esi::universe::GetUniverseStargate> {
        self.stargates.get(idx.0)
    }
}

#[derive(Copy, Clone, Debug)]
pub enum JumpType {
    System,
    Constellation,
    Region,
}

pub struct DrawJump {
    left: math::V3<f64>,
    right: math::V3<f64>,
    left_sec: f64,
    right_sec: f64,
    on_route: bool,
    jump_type: JumpType,
}

#[tokio::main]
async fn main() {
    let client = esi::Client::new();
    let systems = client.get_universe_systems().await.unwrap();

    let mut system_graph = Graph::new_undirected();
    let mut systems_collection = SystemCollection::new();
    let mut stargates_collection = StargateCollection::new();

    let mut all_stargates = HashMap::new();
    let mut all_stargate_ids = Vec::new();

    for &system_id in systems.iter() {
        let system = client.get_universe_system(system_id).await.unwrap();

        if let Some(stargates) = &system.stargates {
            all_stargate_ids.extend_from_slice(stargates);
        }

        systems_collection.push(system);
    }

    for stargate_id in all_stargate_ids {
        let stargate = client.get_universe_stargate(stargate_id).await.unwrap();

        let gate_idx = stargates_collection.push(stargate);
        let node_id = system_graph.add_node(gate_idx);
        all_stargates.insert(stargate_id, node_id);
    }

    for system in systems_collection.iter() {
        if let Some(stargates) = &system.stargates {
            let system_idx = systems_collection.by_id_idx(system.system_id).unwrap();
            for stargate_id in stargates {
                let stargate = stargates_collection.by_id(*stargate_id).unwrap();
                let stargate_node = all_stargates.get(&stargate_id).unwrap();
                let stargate_position: math::V3<f64> = (&stargate.position).into();
                for stargate_id_inner in stargates {
                    if stargate_id >= stargate_id_inner {
                        continue;
                    }

                    let stargate_inner_node = all_stargates.get(&stargate_id_inner).unwrap();
                    let stargate_inner = stargates_collection.by_id(*stargate_id_inner).unwrap();

                    let edge = Edge::Warp {
                        system: system_idx,
                        distance: stargate_position.distance(&(&stargate_inner.position).into()),
                    };

                    system_graph.add_edge(stargate_node.clone(), stargate_inner_node.clone(), edge);
                }

                if stargate.system_id >= stargate.destination.system_id {
                    continue;
                }

                let destination_node = all_stargates.get(&stargate.destination.stargate_id);
                let destination_idx = systems_collection.by_id_idx(stargate.destination.system_id);

                if let (Some(destination_node), Some(destination_idx)) =
                    (destination_node, destination_idx)
                {
                    let edge = Edge::Jump {
                        left: system_idx,
                        right: destination_idx,
                    };

                    system_graph.add_edge(stargate_node.clone(), destination_node.clone(), edge);
                }
            }
        }
    }

    let start = std::time::Instant::now();
    let goons = systems_collection.by_name("1DQ1-A").unwrap();
    let goons = all_stargates
        .get(goons.stargates.as_ref().unwrap().first().unwrap())
        .unwrap()
        .clone();

    let jita = systems_collection.by_name("Jita").unwrap();

    let route = petgraph::algo::astar(
        &system_graph,
        goons,
        |id| {
            let node = &system_graph[id];
            let gate = stargates_collection.by_idx(*node).unwrap();
            gate.system_id == jita.system_id
        },
        |e| e.weight().distance(),
        |_e| 0.0,
    )
    .unwrap();

    eprintln!("Time: {}", start.elapsed().as_millis());
    eprintln!("Systems {}", route.1.len());
    let mut last_system_id = 0;

    let mut route_systems = Vec::new();

    for gate in route.1 {
        let stargate_idx = system_graph[gate];
        let stargate = stargates_collection.by_idx(stargate_idx).unwrap();

        if stargate.system_id == last_system_id {
            continue;
        } else {
            last_system_id = stargate.system_id;
        }

        let system = systems_collection.by_id(stargate.system_id).unwrap();

        route_systems.push(system);
    }

    let jumps: Vec<_> = system_graph
        .edge_references()
        .filter_map(|e| {
            let e = e.weight();
            match e {
                Edge::Jump { left, right } => {
                    let left_sys = systems_collection.by_idx(*left).unwrap();
                    let right_sys = systems_collection.by_idx(*right).unwrap();

                    let jump_type = if left_sys.constellation_id != right_sys.constellation_id {
                        JumpType::Constellation
                    } else {
                        JumpType::System
                    };

                    let on_route = route_systems
                        .iter()
                        .any(|s| s.system_id == left_sys.system_id)
                        && route_systems
                            .iter()
                            .any(|s| s.system_id == right_sys.system_id);

                    Some(DrawJump {
                        left: (&left_sys.position).into(),
                        right: (&right_sys.position).into(),
                        left_sec: left_sys.security_status,
                        right_sec: right_sys.security_status,
                        on_route,
                        jump_type,
                    })
                }
                _ => None,
            }
        })
        .collect();

    let window = gfx::Window::new(1024, 1024);
    window.run(systems_collection, jumps);
}
