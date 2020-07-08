use glium::glutin::event_loop::EventLoopProxy;
use petgraph::Graph;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::esi;
use crate::gfx::{DataEvent, UserEvent};
use crate::math;

#[derive(Debug)]
pub enum Edge {
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
pub enum JumpType {
    System,
    Constellation,
    Region,
    JumpGate,
    Wormhole,
}

pub struct Jump {
    pub left_system_id: i32,
    pub right_system_id: i32,
    pub jump_type: JumpType,
}

#[derive(Copy, Clone, Debug)]
pub struct Stats {
    pub npc_kills: i32,
    pub ship_kills: i32,
    pub pod_kills: i32,
    pub jumps: i32,
}

#[derive(Copy, Clone, Debug)]
pub struct SystemIndex(usize);

#[derive(Copy, Clone, Debug)]
pub struct StargateIndex(usize);

pub struct StargateCollection {
    stargates: Vec<esi::GetUniverseStargate>,
    map_by_id: HashMap<i32, StargateIndex>,
}

impl StargateCollection {
    pub fn new() -> Self {
        StargateCollection {
            stargates: Vec::new(),
            map_by_id: HashMap::new(),
        }
    }

    fn push(&mut self, stargate: esi::GetUniverseStargate) -> StargateIndex {
        let idx = StargateIndex(self.stargates.len());
        self.map_by_id.insert(stargate.stargate_id, idx);
        self.stargates.push(stargate);
        idx
    }

    fn by_id(&self, id: i32) -> Option<&esi::GetUniverseStargate> {
        self.by_id_idx(id).and_then(|idx| self.by_idx(idx))
    }

    fn by_id_idx(&self, id: i32) -> Option<StargateIndex> {
        self.map_by_id.get(&id).cloned()
    }

    fn by_idx(&self, idx: StargateIndex) -> Option<&esi::GetUniverseStargate> {
        self.stargates.get(idx.0)
    }
}

pub struct World {
    systems: Vec<esi::GetUniverseSystem>,
    pub stargates: StargateCollection,
    pub constellations: HashMap<i32, esi::GetUniverseConstellation>,
    pub regions: HashMap<i32, esi::GetUniverseRegion>,
    map_by_name: HashMap<String, SystemIndex>,
    map_by_id: HashMap<i32, SystemIndex>,
    pub graph: Graph<StargateIndex, Edge, petgraph::Undirected, u32>,
    route: Vec<i32>,
    route_target: Option<(i32, i32)>,
    route_text: Vec<(i32, String)>,
    system_stats: HashMap<i32, Stats>,
    player_system: Arc<Mutex<Option<i32>>>,
    sov: HashMap<i32, f64>,
    event_proxy: EventLoopProxy<UserEvent>,
}

impl World {
    pub fn new(event_proxy: EventLoopProxy<UserEvent>) -> Self {
        World {
            systems: Vec::new(),
            stargates: StargateCollection::new(),
            map_by_name: HashMap::new(),
            map_by_id: HashMap::new(),
            constellations: HashMap::new(),
            regions: HashMap::new(),
            graph: Graph::new_undirected(),
            route: Vec::new(),
            route_target: None,
            route_text: Vec::new(),
            system_stats: HashMap::new(),
            player_system: Arc::new(Mutex::new(None)),
            sov: HashMap::new(),
            event_proxy,
        }
    }

    fn push(&mut self, system: esi::GetUniverseSystem) -> SystemIndex {
        let idx = SystemIndex(self.systems.len());
        self.map_by_id.insert(system.system_id, idx);
        self.map_by_name.insert(system.name.clone(), idx);
        self.systems.push(system);
        idx
    }

    pub fn iter(&self) -> impl Iterator<Item = &esi::GetUniverseSystem> {
        self.systems.iter()
    }

    fn by_id(&self, system_id: i32) -> Option<&esi::GetUniverseSystem> {
        self.by_id_idx(system_id).and_then(|idx| self.by_idx(idx))
    }

    fn by_id_idx(&self, system_id: i32) -> Option<SystemIndex> {
        self.map_by_id.get(&system_id).cloned()
    }

    fn by_name(&self, name: &str) -> Option<&esi::GetUniverseSystem> {
        self.by_name_idx(name).and_then(|idx| self.by_idx(idx))
    }

    fn by_name_idx(&self, name: &str) -> Option<SystemIndex> {
        self.map_by_name.get(name).cloned()
    }

    fn by_idx(&self, idx: SystemIndex) -> Option<&esi::GetUniverseSystem> {
        self.systems.get(idx.0)
    }

    fn push_region(&mut self, region: esi::GetUniverseRegion) {
        self.regions.insert(region.region_id, region);
    }

    fn push_constellation(&mut self, constellation: esi::GetUniverseConstellation) {
        self.constellations
            .insert(constellation.constellation_id, constellation);
    }

    pub fn stats(&self, system_id: i32) -> Option<Stats> {
        self.system_stats.get(&system_id).cloned()
    }

    pub fn create_route(&mut self, from: i32, to: i32) {
        let route_target = Some((from, to));
        if self.route_target == route_target {
            return;
        }

        self.route_target = route_target;

        let from = self.by_id(from).unwrap();
        let from = self
            .stargates
            .by_id_idx(*from.stargates.as_ref().unwrap().first().unwrap())
            .unwrap();
        let from = self
            .graph
            .node_indices()
            .find(|s| self.graph[*s].0 == from.0)
            .unwrap();

        let to = self.by_id(to).unwrap();

        let route = petgraph::algo::astar(
            &self.graph,
            from,
            |id| {
                let node = self.graph[id];
                let gate = self.stargates.by_idx(node).unwrap();
                let sys = self.by_id(gate.system_id).unwrap();
                sys.system_id == to.system_id
            },
            |e| e.weight().distance(),
            |_e| 0.0,
        )
        .unwrap();
        let mut last_system_id = 0;
        let mut last_gate_id = -1;
        let mut jump_count = 0;

        let mut route_systems = Vec::new();
        let mut route_text = Vec::new();

        let from_system = self.by_id(self.route_target.unwrap().0).unwrap();
        let to_system = self.by_id(self.route_target.unwrap().1).unwrap();

        let from_const = self
            .constellations
            .get(&from_system.constellation_id)
            .unwrap();
        let from_region = self.regions.get(&from_const.region_id).unwrap();

        let to_const = self
            .constellations
            .get(&to_system.constellation_id)
            .unwrap();
        let to_region = self.regions.get(&to_const.region_id).unwrap();

        route_text.push((
            0,
            format!(
                "{} ({}) :: {} ({})",
                from_system.name, from_region.name, to_system.name, to_region.name
            ),
        ));

        for gate in route.1 {
            let stargate_idx = self.graph[gate];
            let stargate = self.stargates.by_idx(stargate_idx).unwrap();

            if stargate.system_id == last_system_id {
                last_gate_id = stargate.stargate_id;
                continue;
            }

            let system = self.by_id(stargate.system_id).unwrap();
            if let Some(last_gate) = self.stargates.by_id(last_gate_id) {
                let system = self.by_id(last_system_id).unwrap();
                let stats = self.stats(system.system_id).unwrap();
                route_text.push((
                    system.system_id,
                    format!(
                        "{}: {} - n{}.s{}.p{}.j{}",
                        system.name,
                        last_gate.name,
                        stats.npc_kills,
                        stats.ship_kills,
                        stats.pod_kills,
                        stats.jumps
                    ),
                ));
                jump_count += 1;
            }

            last_system_id = stargate.system_id;

            route_systems.push(system.system_id);
        }

        route_text.push((0, format!("{} Jumps", jump_count)));

        self.route = route_systems;
        self.route_text = route_text;
    }

    pub fn is_on_route(&self, system_id: i32) -> bool {
        self.route.iter().any(|&r| r == system_id)
    }

    pub fn jumps(&self) -> Vec<Jump> {
        self.graph
            .edge_references()
            .filter_map(|e| {
                let e = e.weight();
                match e {
                    Edge::Jump { left, right } => {
                        let left_sys = self.by_idx(*left).unwrap();
                        let right_sys = self.by_idx(*right).unwrap();

                        let jump_type = if left_sys.constellation_id != right_sys.constellation_id {
                            let left_constellation =
                                self.constellations.get(&left_sys.constellation_id);
                            let right_constellation =
                                self.constellations.get(&right_sys.constellation_id);

                            if let (Some(left_constellation), Some(right_constellation)) =
                                (left_constellation, right_constellation)
                            {
                                if left_constellation.region_id != right_constellation.region_id {
                                    JumpType::Region
                                } else {
                                    JumpType::Constellation
                                }
                            } else {
                                JumpType::Constellation
                            }
                        } else {
                            JumpType::System
                        };

                        Some(Jump {
                            left_system_id: left_sys.system_id,
                            right_system_id: right_sys.system_id,
                            jump_type,
                        })
                    }
                    Edge::JumpBridge { left, right } => {
                        let left_sys = self.by_idx(*left).unwrap();
                        let right_sys = self.by_idx(*right).unwrap();
                        Some(Jump {
                            left_system_id: left_sys.system_id,
                            right_system_id: right_sys.system_id,
                            jump_type: JumpType::JumpGate,
                        })
                    }
                    Edge::Wormhole { system, wormhole } => {
                        let left_sys = self.by_idx(*system).unwrap();
                        let right_sys = self.by_idx(*wormhole).unwrap();
                        Some(Jump {
                            left_system_id: left_sys.system_id,
                            right_system_id: right_sys.system_id,
                            jump_type: JumpType::JumpGate,
                        })
                    }
                    _ => None,
                }
            })
            .collect()
    }

    pub fn route_text(&self) -> &[(i32, String)] {
        &self.route_text
    }

    pub async fn load(&mut self, client: &esi::Client) -> Result<(), ()> {
        let mut stargates = StargateCollection::new();
        let mut graph = Graph::new_undirected();
        let regions = client.get_universe_regions().await.unwrap();
        let constellations = client.get_universe_constellations().await.unwrap();
        let systems = client.get_universe_systems().await.unwrap();
        let system_kills = client.get_universe_system_kills().await.unwrap();
        let system_jumps = client.get_universe_system_jumps().await.unwrap();

        let location_client = client.clone();
        let inner_event_proxy = self.event_proxy.clone();
        let inner_player_system = self.player_system.clone();
        tokio::spawn(async move {
            let mut counter = 0;
            let poll_interval = 10;
            loop {
                if counter % 10 == 0 {
                    let location = location_client
                        .get_character_location()
                        .await
                        .ok()
                        .map(|l| l.solar_system_id);
                    let mut current_location = inner_player_system.lock().unwrap();
                    if location != *current_location {
                        *current_location = location;
                        inner_event_proxy.send_event(UserEvent::DataEvent(
                            DataEvent::CharacterLocationChanged(location),
                        ));
                    }
                }
                if counter % 3600 == 0 {
                    //update kills and jumps
                }
                tokio::time::delay_for(std::time::Duration::from_secs(poll_interval)).await;
                counter += poll_interval;
            }
        });

        let character = client.get_character_self().await.unwrap();
        let mut alliance_standings = HashMap::new();
        let mut corporation_standings = HashMap::new();

        if let Some(alliance_id) = character.alliance_id {
            let mut page = 1;
            loop {
                let standings = client
                    .get_alliance_contacts(alliance_id, page)
                    .await
                    .unwrap();

                if standings.len() == 0 {
                    break;
                }

                page += 1;

                for standing in standings {
                    match standing.contact_type.as_str() {
                        "corporation" => {
                            corporation_standings.insert(standing.contact_id, standing.standing);
                        }
                        "alliance" => {
                            alliance_standings.insert(standing.contact_id, standing.standing);
                        }
                        _ => (),
                    }
                }
            }
        }

        let mut page = 1;
        loop {
            let standings = client
                .get_corporation_contacts(character.corporation_id, page)
                .await
                .unwrap();

            if standings.len() == 0 {
                break;
            }

            page += 1;

            for standing in standings {
                match standing.contact_type.as_str() {
                    "corporation" => {
                        corporation_standings.insert(standing.contact_id, standing.standing);
                    }
                    "alliance" => {
                        alliance_standings.insert(standing.contact_id, standing.standing);
                    }
                    _ => (),
                }
            }
        }

        let mut page = 1;
        loop {
            let standings = client.get_character_contacts(page).await.unwrap();

            if standings.len() == 0 {
                break;
            }

            page += 1;

            for standing in standings {
                match standing.contact_type.as_str() {
                    "corporation" => {
                        corporation_standings.insert(standing.contact_id, standing.standing);
                    }
                    "alliance" => {
                        alliance_standings.insert(standing.contact_id, standing.standing);
                    }
                    _ => (),
                }
            }
        }

        let mut sov = HashMap::new();

        let sov_map = client.get_sovereignty_map().await.unwrap();

        for system in sov_map {
            match (system.alliance_id, system.corporation_id) {
                (Some(a), _) => {
                    if let Some(standing) = alliance_standings.get(&a) {
                        sov.insert(system.system_id, *standing);
                    }
                }
                (_, Some(c)) => {
                    if let Some(standing) = corporation_standings.get(&c) {
                        sov.insert(system.system_id, *standing);
                    }
                }
                _ => (),
            }
        }

        let mut all_stargates = HashMap::new();
        let mut all_stargate_ids = Vec::new();

        for &region_id in &regions {
            let region = client.get_universe_region(region_id).await.unwrap();
            self.push_region(region);
        }

        for &constellation_id in &constellations {
            let constellation = client
                .get_universe_constellation(constellation_id)
                .await
                .unwrap();
            self.push_constellation(constellation);
        }

        for &system_id in &systems {
            let system = client.get_universe_system(system_id).await.unwrap();

            if let Some(stargates) = &system.stargates {
                all_stargate_ids.extend_from_slice(stargates);
            }

            self.push(system);
            self.system_stats.insert(
                system_id,
                Stats {
                    jumps: 0,
                    npc_kills: 0,
                    ship_kills: 0,
                    pod_kills: 0,
                },
            );
        }

        for sys in system_jumps {
            if let Some(stat) = self.system_stats.get_mut(&sys.system_id) {
                stat.jumps = sys.ship_jumps;
            }
        }

        for sys in system_kills {
            if let Some(stat) = self.system_stats.get_mut(&sys.system_id) {
                stat.npc_kills = sys.npc_kills;
                stat.ship_kills = sys.ship_kills;
                stat.pod_kills = sys.pod_kills;
            }
        }

        for stargate_id in all_stargate_ids {
            let stargate = client.get_universe_stargate(stargate_id).await.unwrap();

            let gate_idx = stargates.push(stargate);
            let node_id = graph.add_node(gate_idx);
            all_stargates.insert(stargate_id, node_id);
        }

        for system in self.iter() {
            if let Some(system_stargates) = &system.stargates {
                let system_idx = self.by_id_idx(system.system_id).unwrap();
                for stargate_id in system_stargates {
                    let stargate = stargates.by_id(*stargate_id).unwrap();
                    let stargate_node = all_stargates.get(&stargate_id).unwrap();
                    let stargate_position: math::V3<f64> = math::V3::new(
                        stargate.position.x,
                        stargate.position.y,
                        stargate.position.z,
                    );
                    for stargate_id_inner in system_stargates {
                        if stargate_id >= stargate_id_inner {
                            continue;
                        }

                        let stargate_inner_node = all_stargates.get(&stargate_id_inner).unwrap();
                        let stargate_inner = stargates.by_id(*stargate_id_inner).unwrap();
                        let stargate_inner_position: math::V3<f64> = math::V3::new(
                            stargate_inner.position.x,
                            stargate_inner.position.y,
                            stargate_inner.position.z,
                        );

                        let edge = Edge::Warp {
                            system: system_idx,
                            distance: stargate_position.distance(&stargate_inner_position) / 1e12,
                        };

                        graph.add_edge(stargate_node.clone(), stargate_inner_node.clone(), edge);
                    }

                    if stargate.system_id >= stargate.destination.system_id {
                        continue;
                    }

                    let destination_node = all_stargates.get(&stargate.destination.stargate_id);
                    let destination_idx = self.by_id_idx(stargate.destination.system_id);

                    if let (Some(destination_node), Some(destination_idx)) =
                        (destination_node, destination_idx)
                    {
                        let edge = Edge::Jump {
                            left: system_idx,
                            right: destination_idx,
                        };

                        graph.add_edge(stargate_node.clone(), destination_node.clone(), edge);
                    }
                }
            }
        }

        use std::io::Read;
        let mut bridges = std::fs::File::open("bridges.tsv").unwrap();
        let mut bridges_tsv = String::new();
        bridges.read_to_string(&mut bridges_tsv).unwrap();

        let mut jb_id = 0;
        for line in bridges_tsv.lines() {
            let line_parts: Vec<_> = line.split('\t').collect();
            let left = line_parts[1].split(' ').next().unwrap();
            let right = line_parts[2].split(' ').next().unwrap();

            let left = self.by_name(left).unwrap();
            let right = self.by_name(right).unwrap();

            let left_jb_id = jb_id;
            let right_jb_id = jb_id + 1;
            jb_id += 2;
            let left_jb = esi::GetUniverseStargate {
                stargate_id: left_jb_id,
                name: format!("Jump Gate ({} --> {})", left.name, right.name),
                destination: esi::GetUniverseStargateDestination {
                    stargate_id: right_jb_id,
                    system_id: right.system_id,
                },
                position: esi::Position {
                    x: left.position.x,
                    y: left.position.y,
                    z: left.position.z,
                },
                system_id: left.system_id,
            };

            let right_jb = esi::GetUniverseStargate {
                stargate_id: right_jb_id,
                name: format!("Jump Gate ({} --> {})", right.name, left.name),
                destination: esi::GetUniverseStargateDestination {
                    stargate_id: left_jb_id,
                    system_id: left.system_id,
                },
                position: esi::Position {
                    x: right.position.x,
                    y: right.position.y,
                    z: right.position.z,
                },
                system_id: right.system_id,
            };

            let left_idx = stargates.push(left_jb);
            let left_node_id = graph.add_node(left_idx);
            all_stargates.insert(left_jb_id, left_node_id);
            let left_sys_idx = self.by_id_idx(left.system_id).unwrap();

            let right_idx = stargates.push(right_jb);
            let right_node_id = graph.add_node(right_idx);
            all_stargates.insert(right_jb_id, right_node_id);
            let right_sys_idx = self.by_id_idx(right.system_id).unwrap();

            if let Some(gates) = &left.stargates {
                for gate in gates {
                    let edge = Edge::Warp {
                        system: left_sys_idx,
                        distance: 1.0,
                    };
                    let gate = all_stargates.get(&gate).unwrap();
                    graph.add_edge(left_node_id.clone(), gate.clone(), edge);
                }
            }

            if let Some(gates) = &right.stargates {
                for gate in gates {
                    let edge = Edge::Warp {
                        system: right_sys_idx,
                        distance: 1.0,
                    };
                    let gate = all_stargates.get(&gate).unwrap();
                    graph.add_edge(right_node_id.clone(), gate.clone(), edge);
                }
            }

            let edge = Edge::JumpBridge {
                left: left_sys_idx,
                right: right_sys_idx,
            };

            graph.add_edge(left_node_id.clone(), right_node_id.clone(), edge);
        }

        self.graph = graph;
        self.stargates = stargates;
        self.sov = sov;

        Ok(())
    }

    pub fn sov_standing(&self, system: i32) -> Option<f64> {
        self.sov.get(&system).cloned()
    }

    pub fn match_system(&self, search: &str) -> Vec<i32> {
        if search == "@me" {
            if let Some(location) = self.location() {
                return vec![location];
            } else {
                return Vec::new();
            }
        }
        let search = search.to_uppercase();
        let search = search.trim();
        let mut matches = Vec::new();
        for sys in &self.systems {
            let name = sys.name.to_uppercase();
            let name = name.trim();

            if name.starts_with(search) {
                matches.push(sys.system_id);
            }
        }

        matches
    }

    pub fn location(&self) -> Option<i32> {
        *self.player_system.lock().unwrap()
    }
}
