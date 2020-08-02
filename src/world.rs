use futures::future::FutureExt;
use futures::stream::futures_unordered::FuturesUnordered;
use futures::stream::StreamExt;
use glium::glutin::event_loop::EventLoopProxy;
use petgraph::Graph;

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::{Arc, RwLock};

use crate::esi;
use crate::gfx::{DataEvent, UserEvent};
use crate::math;

#[derive(Debug, Clone, Copy)]
pub enum Edge {
    Warp { system: i32, distance: f64 },
    JumpBridge { left: i32, right: i32 },
    Wormhole { system: i32, wormhole: i32 },
    Jump { left: i32, right: i32 },
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

#[derive(Debug, Clone, Copy)]
enum Node {
    Stargate {
        stargate: i32,
        source: i32,
        destination: i32,
    },
    System {
        system: i32,
    },
    JumpGate {
        stargate: i32,
        source: i32,
        destination: i32,
    },
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

#[derive(Debug, Clone, Copy)]
pub struct Sov {
    pub alliance_id: Option<i32>,
    pub corporation_id: Option<i32>,
    pub standing: f64,
}

pub struct World {
    systems: HashMap<i32, esi::GetUniverseSystem>,
    systems_by_name: HashMap<String, i32>,
    stargates: HashMap<i32, esi::GetUniverseStargate>,
    constellations: HashMap<i32, esi::GetUniverseConstellation>,
    regions: HashMap<i32, esi::GetUniverseRegion>,
    graph: Graph<Node, Edge, petgraph::Undirected, u32>,
    route: Vec<i32>,
    route_target: Option<(i32, i32)>,
    route_text: Vec<(i32, String)>,
    system_stats: Arc<RwLock<HashMap<i32, Stats>>>,
    player_system: Arc<RwLock<Option<i32>>>,
    sov: Arc<RwLock<HashMap<i32, Sov>>>,
    alliances: Arc<RwLock<HashMap<i32, esi::GetAlliance>>>,
    corporations: Arc<RwLock<HashMap<i32, esi::GetCorporation>>>,
    event_proxy: EventLoopProxy<UserEvent>,
}

impl World {
    pub fn new(event_proxy: EventLoopProxy<UserEvent>) -> Self {
        World {
            systems: HashMap::new(),
            systems_by_name: HashMap::new(),
            stargates: HashMap::new(),
            constellations: HashMap::new(),
            regions: HashMap::new(),
            graph: Graph::new_undirected(),
            route: Vec::new(),
            route_target: None,
            route_text: Vec::new(),
            system_stats: Arc::new(RwLock::new(HashMap::new())),
            player_system: Arc::new(RwLock::new(None)),
            sov: Arc::new(RwLock::new(HashMap::new())),
            alliances: Arc::new(RwLock::new(HashMap::new())),
            corporations: Arc::new(RwLock::new(HashMap::new())),
            event_proxy,
        }
    }

    pub fn systems(&self) -> impl Iterator<Item = &esi::GetUniverseSystem> {
        self.systems.values()
    }

    pub fn system(&self, system_id: i32) -> Option<&esi::GetUniverseSystem> {
        self.systems.get(&system_id)
    }

    fn system_by_name(&self, name: &str) -> Option<&esi::GetUniverseSystem> {
        self.systems_by_name
            .get(name)
            .and_then(|id| self.system(*id))
    }

    pub fn regions(&self) -> impl Iterator<Item = &esi::GetUniverseRegion> {
        self.regions.values()
    }

    pub fn region(&self, region_id: i32) -> Option<&esi::GetUniverseRegion> {
        self.regions.get(&region_id)
    }

    pub fn constellation(&self, constellation_id: i32) -> Option<&esi::GetUniverseConstellation> {
        self.constellations.get(&constellation_id)
    }

    pub fn alliance(&self, alliance_id: i32) -> Option<esi::GetAlliance> {
        self.alliances.read().unwrap().get(&alliance_id).cloned()
    }

    pub fn corporation(&self, corporation_id: i32) -> Option<esi::GetCorporation> {
        self.corporations
            .read()
            .unwrap()
            .get(&corporation_id)
            .cloned()
    }

    fn push_system(&mut self, system: esi::GetUniverseSystem) {
        self.systems_by_name
            .insert(system.name.clone(), system.system_id);
        self.systems.insert(system.system_id, system);
    }

    fn push_stargate(&mut self, stargate: esi::GetUniverseStargate) {
        self.stargates.insert(stargate.stargate_id, stargate);
    }

    fn push_constellation(&mut self, constellation: esi::GetUniverseConstellation) {
        self.constellations
            .insert(constellation.constellation_id, constellation);
    }

    fn push_region(&mut self, region: esi::GetUniverseRegion) {
        self.regions.insert(region.region_id, region);
    }

    pub fn stats(&self, system_id: i32) -> Option<Stats> {
        let stats = self.system_stats.read().unwrap();
        stats.get(&system_id).cloned()
    }

    pub fn create_route(&mut self, from: i32, to: i32) {
        let route_target = Some((from, to));
        if self.route_target == route_target {
            return;
        }

        self.route_target = route_target;

        let from = self
            .graph
            .node_indices()
            .find(|s| match self.graph[*s] {
                Node::System { system } if system == from => true,
                _ => false,
            })
            .unwrap();

        let route = petgraph::algo::astar(
            &self.graph,
            from,
            |id| {
                let node_id = self.graph[id];
                match node_id {
                    Node::System { system } if system == to => true,
                    _ => false,
                }
            },
            |e| e.weight().distance(),
            |_e| 0.0,
        )
        .unwrap();

        let mut jump_count = 0;

        let mut route_systems = Vec::new();
        let mut route_text = Vec::new();

        let from_system = self.system(self.route_target.unwrap().0).unwrap();
        let to_system = self.system(self.route_target.unwrap().1).unwrap();

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

        let mut visited = HashSet::new();
        for gate in route.1 {
            let node = self.graph[gate];
            match node {
                Node::JumpGate {
                    stargate,
                    source,
                    destination,
                }
                | Node::Stargate {
                    stargate,
                    source,
                    destination,
                } => {
                    let gate = self.stargates.get(&stargate).unwrap();
                    visited.insert(source);
                    if !visited.contains(&destination) {
                        let system = self.system(source).unwrap();
                        let stats = self.stats(gate.system_id).unwrap();
                        route_text.push((
                            system.system_id,
                            format!(
                                "{}: {} - n{}.s{}.p{}.j{}",
                                system.name,
                                gate.name,
                                stats.npc_kills,
                                stats.ship_kills,
                                stats.pod_kills,
                                stats.jumps
                            ),
                        ));
                        route_systems.push(system.system_id);
                        jump_count += 1;
                    }
                }
                Node::System { .. } => (),
            }
        }
        route_systems.push(to);

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
                        let left_sys = self.system(*left).unwrap();
                        let right_sys = self.system(*right).unwrap();

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
                        let left_sys = self.system(*left).unwrap();
                        let right_sys = self.system(*right).unwrap();
                        Some(Jump {
                            left_system_id: left_sys.system_id,
                            right_system_id: right_sys.system_id,
                            jump_type: JumpType::JumpGate,
                        })
                    }
                    Edge::Wormhole { system, wormhole } => {
                        let left_sys = self.system(*system).unwrap();
                        let right_sys = self.system(*wormhole).unwrap();
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

    pub async fn load_sov_standings(
        sov_standings: &Arc<RwLock<HashMap<i32, Sov>>>,
        alliances: &Arc<RwLock<HashMap<i32, esi::GetAlliance>>>,
        corporations: &Arc<RwLock<HashMap<i32, esi::GetCorporation>>>,
        client: &esi::Client,
    ) {
        use tokio::sync::RwLock;

        let character = client.get_character_self().await.unwrap();

        let alliance_standings = Arc::new(RwLock::new(HashMap::new()));
        let corporation_standings = Arc::new(RwLock::new(HashMap::new()));

        let update_alliance_standings = async {
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
                                corporation_standings
                                    .write()
                                    .await
                                    .insert(standing.contact_id, standing.standing);
                            }
                            "alliance" => {
                                alliance_standings
                                    .write()
                                    .await
                                    .insert(standing.contact_id, standing.standing);
                            }
                            _ => (),
                        }
                    }
                }
            }
        };

        let update_corporation_standings = async {
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
                            corporation_standings
                                .write()
                                .await
                                .insert(standing.contact_id, standing.standing);
                        }
                        "alliance" => {
                            alliance_standings
                                .write()
                                .await
                                .insert(standing.contact_id, standing.standing);
                        }
                        _ => (),
                    }
                }
            }
        };

        let update_character_standings = async {
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
                            corporation_standings
                                .write()
                                .await
                                .insert(standing.contact_id, standing.standing);
                        }
                        "alliance" => {
                            alliance_standings
                                .write()
                                .await
                                .insert(standing.contact_id, standing.standing);
                        }
                        _ => (),
                    }
                }
            }
        };

        let (sov_map, _, _, _) = futures::join!(
            client.get_sovereignty_map().map(Result::unwrap),
            update_alliance_standings,
            update_corporation_standings,
            update_character_standings
        );

        {
            let mut sov = sov_standings.write().unwrap();
            sov.clear();
        }

        let mut alliance_ids = Vec::new();
        let mut corporation_ids = Vec::new();

        for system in sov_map {
            let alliance = if let Some(alliance_id) = system.alliance_id {
                alliance_ids.push(alliance_id);
                alliance_standings.read().await.get(&alliance_id).cloned()
            } else {
                None
            };
            let corporation = if let Some(corporation_id) = system.corporation_id {
                corporation_ids.push(corporation_id);
                corporation_standings
                    .read()
                    .await
                    .get(&corporation_id)
                    .cloned()
            } else {
                None
            };

            if let Some(standing) = alliance.or(corporation) {
                let mut sov = sov_standings.write().unwrap();
                sov.insert(
                    system.system_id,
                    Sov {
                        alliance_id: system.alliance_id,
                        corporation_id: system.corporation_id,
                        standing,
                    },
                );
            } else if system.alliance_id.is_some() || system.corporation_id.is_some() {
                let mut sov = sov_standings.write().unwrap();
                sov.insert(
                    system.system_id,
                    Sov {
                        alliance_id: system.alliance_id,
                        corporation_id: system.corporation_id,
                        standing: 0.0,
                    },
                );
            }
        }

        let alliances_fut: FuturesUnordered<_> = alliance_ids
            .iter()
            .map(|alliance_id| client.get_alliance(*alliance_id))
            .collect();

        let corporations_fut: FuturesUnordered<_> = corporation_ids
            .iter()
            .map(|corporation_id| client.get_corporation(*corporation_id))
            .collect();

        let (alliance_res, corporation_res): (Vec<_>, Vec<_>) = futures::join!(
            alliances_fut.map(Result::unwrap).collect(),
            corporations_fut.map(Result::unwrap).collect()
        );

        {
            let mut alls = alliances.write().unwrap();
            for alliance in alliance_res {
                alls.insert(alliance.alliance_id, alliance);
            }
        }

        {
            let mut corps = corporations.write().unwrap();
            for corporation in corporation_res {
                corps.insert(corporation.corporation_id, corporation);
            }
        }
    }

    pub async fn load_system_stats(
        system_stats: &Arc<RwLock<HashMap<i32, Stats>>>,
        client: &esi::Client,
    ) {
        let (system_kills, system_jumps) = futures::join!(
            client.get_universe_system_kills().map(Result::unwrap),
            client.get_universe_system_jumps().map(Result::unwrap)
        );

        let mut stats = system_stats.write().unwrap();
        for sys in system_jumps {
            if let Some(stat) = stats.get_mut(&sys.system_id) {
                stat.jumps = sys.ship_jumps;
            }
        }

        for sys in system_kills {
            if let Some(stat) = stats.get_mut(&sys.system_id) {
                stat.npc_kills = sys.npc_kills;
                stat.ship_kills = sys.ship_kills;
                stat.pod_kills = sys.pod_kills;
            }
        }
    }

    pub async fn load(&mut self, client: &esi::Client) -> Result<(), ()> {
        let regions = client.get_universe_regions();
        let constellations = client.get_universe_constellations();
        let systems = client.get_universe_systems();

        let (regions, constellations, systems) = futures::join!(regions, constellations, systems);

        let regions = regions.unwrap();
        let constellations = constellations.unwrap();
        let systems = systems.unwrap();

        let mut all_systems = HashMap::new();
        let mut all_stargates = HashMap::new();
        let mut all_stargate_ids = Vec::new();

        let regions_fut: FuturesUnordered<_> = regions
            .iter()
            .map(|region_id| client.get_universe_region(*region_id))
            .collect();

        let constellations_fut: FuturesUnordered<_> = constellations
            .iter()
            .map(|constellation_id| client.get_universe_constellation(*constellation_id))
            .collect();

        let systems_fut: FuturesUnordered<_> = systems
            .iter()
            .map(|system_id| client.get_universe_system(*system_id))
            .collect();

        let (regions, constellations, systems): (Vec<_>, Vec<_>, Vec<_>) = futures::join!(
            regions_fut.map(Result::unwrap).collect(),
            constellations_fut.map(Result::unwrap).collect(),
            systems_fut.map(Result::unwrap).collect(),
        );

        for region in regions {
            self.push_region(region);
        }

        for constellation in constellations {
            self.push_constellation(constellation);
        }

        for system in systems {
            if let Some(stargates) = &system.stargates {
                all_stargate_ids.extend_from_slice(stargates);
            }

            let node_id = self.graph.add_node(Node::System {
                system: system.system_id,
            });
            all_systems.insert(system.system_id, node_id);
            {
                let mut stats = self.system_stats.write().unwrap();
                stats.insert(
                    system.system_id,
                    Stats {
                        jumps: 0,
                        npc_kills: 0,
                        ship_kills: 0,
                        pod_kills: 0,
                    },
                );
            }

            self.push_system(system);
        }

        let stargates_fut: FuturesUnordered<_> = all_stargate_ids
            .iter()
            .map(|stargate_id| client.get_universe_stargate(*stargate_id))
            .collect();

        let stargates: Vec<_> = stargates_fut.map(Result::unwrap).collect().await;

        for stargate in stargates {
            let node_id = self.graph.add_node(Node::Stargate {
                stargate: stargate.stargate_id,
                source: stargate.system_id,
                destination: stargate.destination.system_id,
            });
            all_stargates.insert(stargate.stargate_id, node_id);
            self.push_stargate(stargate);
        }

        for system in self.systems.values() {
            let system_node = all_systems.get(&system.system_id).unwrap();
            let system_position: math::V3<f64> =
                math::V3::new(system.position.x, system.position.y, system.position.z);

            if let Some(system_stargates) = &system.stargates {
                for stargate_id in system_stargates {
                    let stargate = self.stargates.get(stargate_id).unwrap();
                    let stargate_node = all_stargates.get(&stargate_id).unwrap();
                    let stargate_position: math::V3<f64> = math::V3::new(
                        stargate.position.x,
                        stargate.position.y,
                        stargate.position.z,
                    );

                    let edge = Edge::Warp {
                        system: system.system_id,
                        distance: system_position.distance(&stargate_position) / 1e12,
                    };

                    self.graph
                        .add_edge(system_node.clone(), stargate_node.clone(), edge);

                    for stargate_id_inner in system_stargates {
                        if stargate_id >= stargate_id_inner {
                            continue;
                        }

                        let stargate_inner_node = all_stargates.get(&stargate_id_inner).unwrap();
                        let stargate_inner = self.stargates.get(stargate_id_inner).unwrap();
                        let stargate_inner_position: math::V3<f64> = math::V3::new(
                            stargate_inner.position.x,
                            stargate_inner.position.y,
                            stargate_inner.position.z,
                        );

                        let edge = Edge::Warp {
                            system: system.system_id,
                            distance: stargate_position.distance(&stargate_inner_position) / 1e12,
                        };

                        self.graph.add_edge(
                            stargate_node.clone(),
                            stargate_inner_node.clone(),
                            edge,
                        );
                    }

                    if stargate.system_id >= stargate.destination.system_id {
                        continue;
                    }

                    let destination_node = all_stargates.get(&stargate.destination.stargate_id);

                    if let Some(destination_node) = destination_node {
                        let edge = Edge::Jump {
                            left: stargate.system_id,
                            right: stargate.destination.system_id,
                        };

                        self.graph
                            .add_edge(stargate_node.clone(), destination_node.clone(), edge);
                    }
                }
            }
        }

        let mut bridges = std::fs::File::open("bridges.tsv").unwrap();
        let mut bridges_tsv = String::new();
        bridges.read_to_string(&mut bridges_tsv).unwrap();

        let mut jb_id = 0;
        for line in bridges_tsv.lines() {
            let line_parts: Vec<_> = line.split('\t').collect();
            let left = line_parts[1].split(' ').next().unwrap();
            let right = line_parts[2].split(' ').next().unwrap();

            let left = self.system_by_name(left).cloned().unwrap();
            let right = self.system_by_name(right).cloned().unwrap();

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

            self.stargates.insert(left_jb_id, left_jb);
            let left_node = Node::JumpGate {
                stargate: left_jb_id,
                source: left.system_id,
                destination: right.system_id,
            };
            let left_node_id = self.graph.add_node(left_node);
            all_stargates.insert(left_jb_id, left_node_id);
            let left_system_node = all_systems.get(&left.system_id).unwrap();

            self.stargates.insert(right_jb_id, right_jb);
            let right_node = Node::JumpGate {
                stargate: right_jb_id,
                source: right.system_id,
                destination: left.system_id,
            };
            let right_node_id = self.graph.add_node(right_node);
            all_stargates.insert(right_jb_id, right_node_id);
            let right_system_node = all_systems.get(&right.system_id).unwrap();

            let left_warp = Edge::Warp {
                system: left.system_id,
                distance: 1.0,
            };

            let right_warp = Edge::Warp {
                system: right.system_id,
                distance: 1.0,
            };

            let edge = Edge::JumpBridge {
                left: left.system_id,
                right: right.system_id,
            };

            self.graph
                .add_edge(left_node_id.clone(), left_system_node.clone(), left_warp);
            self.graph
                .add_edge(right_node_id.clone(), right_system_node.clone(), right_warp);
            self.graph
                .add_edge(left_node_id.clone(), right_node_id.clone(), edge);
        }

        self.spawn_background_updater(client.clone());

        Ok(())
    }

    fn spawn_background_updater(&self, client: esi::Client) {
        let event_proxy = self.event_proxy.clone();
        let player_system = self.player_system.clone();
        let system_stats = self.system_stats.clone();
        let sov_standings = self.sov.clone();
        let alliances = self.alliances.clone();
        let corporations = self.corporations.clone();
        tokio::spawn(async move {
            let mut counter = 0;
            let poll_interval = 10;
            loop {
                if counter % 10 == 0 {
                    let location = client
                        .get_character_location()
                        .await
                        .ok()
                        .map(|l| l.solar_system_id);
                    let mut current_location = player_system.write().unwrap();
                    if location != *current_location {
                        *current_location = location;
                        let _ = event_proxy.send_event(UserEvent::DataEvent(
                            DataEvent::CharacterLocationChanged(location),
                        ));
                    }
                }
                if counter % 300 == 0 {
                    World::load_system_stats(&system_stats, &client).await;
                    World::load_sov_standings(&sov_standings, &alliances, &corporations, &client)
                        .await;
                    let _ = event_proxy
                        .send_event(UserEvent::DataEvent(DataEvent::SovStandingsChanged));
                    let _ =
                        event_proxy.send_event(UserEvent::DataEvent(DataEvent::SystemStatsChanged));
                }
                tokio::time::delay_for(std::time::Duration::from_secs(poll_interval)).await;
                counter += poll_interval;
            }
        });
    }

    pub fn sov_standing(&self, system: i32) -> Option<Sov> {
        let sov = self.sov.read().unwrap();
        sov.get(&system).cloned()
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
        for sys in self.systems.values() {
            let name = sys.name.to_uppercase();
            let name = name.trim();

            if name.starts_with(search) {
                matches.push(sys.system_id);
            }
        }

        matches
    }

    pub fn location(&self) -> Option<i32> {
        *self.player_system.read().unwrap()
    }
}
