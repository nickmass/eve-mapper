use async_std::sync::RwLock as RwLockAsync;
use async_std::task::sleep;
use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender};
use futures::future::FutureExt;
use futures::stream::futures_unordered::FuturesUnordered;
use futures::stream::StreamExt;
use petgraph::Graph;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use crate::esi;
use crate::gfx::{DataEvent, UserEvent, UserEventSender};
use crate::math;
use crate::platform::{file_exists, read_file, spawn, EventSender};

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

#[derive(Debug, Clone, Copy)]
pub struct RouteNode {
    pub arrive_jump: Option<JumpType>,
    pub leave_jump: Option<JumpType>,
    pub system_id: i32,
}

enum UpdateRequest {
    AllianceLogo(i32),
    SendRouteToClient(Option<i32>, Vec<i32>),
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
    route_nodes: Vec<RouteNode>,
    system_stats: Arc<RwLock<HashMap<i32, Stats>>>,
    player_system: Arc<RwLock<Option<i32>>>,
    sov: Arc<RwLock<HashMap<i32, Sov>>>,
    alliances: Arc<RwLock<HashMap<i32, esi::GetAlliance>>>,
    corporations: Arc<RwLock<HashMap<i32, esi::GetCorporation>>>,
    alliance_logos: Arc<RwLock<HashMap<i32, Arc<Vec<u8>>>>>,
    event_sender: EventSender,
    update_sender: Option<UnboundedSender<UpdateRequest>>,
}

impl World {
    pub fn new(event_sender: EventSender) -> Self {
        World {
            systems: HashMap::new(),
            systems_by_name: HashMap::new(),
            stargates: HashMap::new(),
            constellations: HashMap::new(),
            regions: HashMap::new(),
            graph: Graph::new_undirected(),
            route: Vec::new(),
            route_target: None,
            route_nodes: Vec::new(),
            system_stats: Arc::new(RwLock::new(HashMap::new())),
            player_system: Arc::new(RwLock::new(None)),
            sov: Arc::new(RwLock::new(HashMap::new())),
            alliances: Arc::new(RwLock::new(HashMap::new())),
            corporations: Arc::new(RwLock::new(HashMap::new())),
            alliance_logos: Arc::new(RwLock::new(HashMap::new())),
            event_sender,
            update_sender: None,
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

    pub fn alliance_logo(&self, alliance_id: i32) -> Option<Arc<Vec<u8>>> {
        let logo = self
            .alliance_logos
            .read()
            .unwrap()
            .get(&alliance_id)
            .cloned();
        if logo.is_some() {
            logo
        } else {
            if let Some(sender) = self.update_sender.as_ref() {
                let _ = sender.unbounded_send(UpdateRequest::AllianceLogo(alliance_id));
            }
            None
        }
    }

    pub fn stats(&self, system_id: i32) -> Option<Stats> {
        let stats = self.system_stats.read().unwrap();
        stats.get(&system_id).cloned()
    }

    pub fn distances_from(&self, system_id: i32) -> HashMap<i32, u32> {
        let idx = self
            .graph
            .node_indices()
            .find(|n| {
                if let Node::System { system } = self.graph[*n] {
                    system == system_id
                } else {
                    false
                }
            })
            .unwrap();

        let distances = petgraph::algo::dijkstra(&self.graph, idx, None, |e| match e.weight() {
            Edge::JumpBridge { .. } | Edge::Jump { .. } | Edge::Wormhole { .. } => 1,
            _ => 0,
        });

        distances
            .into_iter()
            .filter_map(|(k, distance)| match self.graph[k] {
                Node::System { system } => Some((system, distance)),
                _ => None,
            })
            .collect()
    }

    pub fn clear_route(&mut self) {
        self.route_target = None;
        self.route_nodes.clear();
        self.route.clear();
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

        let mut route_systems = Vec::new();
        let mut route_nodes = Vec::new();

        let mut visited = HashSet::new();
        let mut arrive_gate = None;
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
                        let source = self.system(source).unwrap();
                        let dest = self.system(destination).unwrap();
                        let source_const = self.constellation(source.constellation_id);
                        let dest_const = self.constellation(dest.constellation_id);

                        route_systems.push(source.system_id);
                        let leave_gate = match node {
                            Node::JumpGate { .. } => Some(JumpType::JumpGate),
                            Node::Stargate { .. } => {
                                if source.constellation_id == dest.constellation_id {
                                    Some(JumpType::System)
                                } else if source_const.map(|c| c.region_id)
                                    == dest_const.map(|c| c.region_id)
                                {
                                    Some(JumpType::Constellation)
                                } else {
                                    Some(JumpType::Region)
                                }
                            }
                            _ => None,
                        };

                        route_nodes.push(RouteNode {
                            system_id: gate.system_id,
                            arrive_jump: arrive_gate,
                            leave_jump: leave_gate,
                        });

                        arrive_gate = leave_gate;
                    }
                }
                Node::System { .. } => (),
            }
        }
        route_nodes.push(RouteNode {
            system_id: to,
            arrive_jump: arrive_gate,
            leave_jump: None,
        });
        route_systems.push(to);

        self.route = route_systems;
        self.route_nodes = route_nodes;
    }

    pub fn is_on_route(&self, system_id: i32) -> bool {
        self.route.iter().any(|&r| r == system_id)
    }

    pub fn route_nodes(&self) -> &[RouteNode] {
        self.route_nodes.as_slice()
    }

    pub fn route_target(&self) -> Option<(i32, i32)> {
        self.route_target
    }

    pub fn send_route_to_client(&self) {
        let route = self.route.clone();
        let player_location = self.location();

        if let Some(sender) = self.update_sender.as_ref() {
            let _ = sender.unbounded_send(UpdateRequest::SendRouteToClient(player_location, route));
        }
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

    pub async fn load_sov_standings(
        sov_standings: &Arc<RwLock<HashMap<i32, Sov>>>,
        alliances: &Arc<RwLock<HashMap<i32, esi::GetAlliance>>>,
        corporations: &Arc<RwLock<HashMap<i32, esi::GetCorporation>>>,
        client: &esi::Client,
    ) {
        let character = client.get_character_self().await.unwrap();

        let alliance_standings = Arc::new(RwLockAsync::new(HashMap::new()));
        let corporation_standings = Arc::new(RwLockAsync::new(HashMap::new()));

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

    pub fn import(&mut self, galaxy: Galaxy) {
        for system_id in galaxy.systems.keys() {
            {
                let mut stats = self.system_stats.write().unwrap();
                stats.insert(
                    *system_id,
                    Stats {
                        jumps: 0,
                        npc_kills: 0,
                        ship_kills: 0,
                        pod_kills: 0,
                    },
                );
            }
        }
        let Galaxy {
            systems,
            systems_by_name,
            stargates,
            constellations,
            regions,
            graph,
            client,
        } = galaxy;

        self.systems = systems;
        self.systems_by_name = systems_by_name;
        self.stargates = stargates;
        self.constellations = constellations;
        self.regions = regions;
        self.graph = graph;

        let _ = self
            .event_sender
            .send_user_event(UserEvent::DataEvent(DataEvent::GalaxyImported));
        let (tx, rx) = unbounded();
        self.update_sender = Some(tx);
        self.spawn_background_updater(client.clone(), rx);
    }

    fn spawn_background_updater(
        &self,
        client: esi::Client,
        mut update_receiver: UnboundedReceiver<UpdateRequest>,
    ) {
        let event_sender = self.event_sender.clone();
        let player_system = self.player_system.clone();
        let system_stats = self.system_stats.clone();
        let sov_standings = self.sov.clone();
        let alliances = self.alliances.clone();
        let corporations = self.corporations.clone();

        let alliance_logos = self.alliance_logos.clone();
        spawn({
            let client = client.clone();
            let event_sender = event_sender.clone();
            async move {
                loop {
                    let update = update_receiver.next().await;
                    match update {
                        Some(UpdateRequest::AllianceLogo(alliance_id)) => {
                            let logo = client.get_alliance_logo(alliance_id, 256).await.unwrap();
                            let logo = Arc::new(logo);

                            alliance_logos.write().unwrap().insert(alliance_id, logo);
                            event_sender
                                .send_user_event(UserEvent::DataEvent(DataEvent::ImageLoaded));
                        }
                        Some(UpdateRequest::SendRouteToClient(player_location, route)) => {
                            if route.len() > 0 {
                                match client.get_character_online().await {
                                    Ok(online) => {
                                        if !online.online {
                                            continue;
                                        }
                                    }
                                    Err(error) => {
                                        log::error!("send route online check failed: {:?}", error);
                                    }
                                }
                                let player_on_route =
                                    route.iter().any(|r| Some(*r) == player_location);
                                let mut send_systems = !player_on_route;

                                let mut first = true;

                                for system in route {
                                    if send_systems {
                                        let result =
                                            client.post_waypoint(false, first, system).await;
                                        if let Err(error) = result {
                                            log::error!("send route failed: {:?}", error);
                                            break;
                                        }
                                        first = false;
                                    } else {
                                        if Some(system) == player_location {
                                            send_systems = true;
                                        }
                                    }
                                }
                            }
                        }
                        None => {
                            break;
                        }
                    }
                }
            }
        });
        spawn(async move {
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
                        event_sender.send_user_event(UserEvent::DataEvent(
                            DataEvent::CharacterLocationChanged(location),
                        ));
                    }
                }
                if counter % 300 == 0 {
                    World::load_system_stats(&system_stats, &client).await;
                    World::load_sov_standings(&sov_standings, &alliances, &corporations, &client)
                        .await;
                    event_sender
                        .send_user_event(UserEvent::DataEvent(DataEvent::SovStandingsChanged));
                    event_sender
                        .send_user_event(UserEvent::DataEvent(DataEvent::SystemStatsChanged));
                }
                sleep(std::time::Duration::from_secs(poll_interval)).await;
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

#[derive(Clone, Debug)]
pub struct Galaxy {
    systems: HashMap<i32, esi::GetUniverseSystem>,
    systems_by_name: HashMap<String, i32>,
    stargates: HashMap<i32, esi::GetUniverseStargate>,
    constellations: HashMap<i32, esi::GetUniverseConstellation>,
    regions: HashMap<i32, esi::GetUniverseRegion>,
    graph: Graph<Node, Edge, petgraph::Undirected, u32>,
    client: crate::esi::Client,
}

impl Galaxy {
    pub async fn load() -> Self {
        let profile = crate::oauth::load_or_authorize().await.unwrap();
        let client = crate::esi::Client::new(profile).await;

        let mut galaxy = Galaxy {
            systems: HashMap::new(),
            systems_by_name: HashMap::new(),
            stargates: HashMap::new(),
            constellations: HashMap::new(),
            regions: HashMap::new(),
            graph: Graph::new_undirected(),
            client: client.clone(),
        };

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
            galaxy.regions.insert(region.region_id, region);
        }

        for constellation in constellations {
            galaxy
                .constellations
                .insert(constellation.constellation_id, constellation);
        }

        for system in systems {
            if let Some(stargates) = &system.stargates {
                all_stargate_ids.extend_from_slice(stargates);
            }

            let node_id = galaxy.graph.add_node(Node::System {
                system: system.system_id,
            });
            all_systems.insert(system.system_id, node_id);

            galaxy
                .systems_by_name
                .insert(system.name.clone(), system.system_id);
            galaxy.systems.insert(system.system_id, system);
        }

        let stargates_fut: FuturesUnordered<_> = all_stargate_ids
            .iter()
            .map(|stargate_id| client.get_universe_stargate(*stargate_id))
            .collect();

        let stargates: Vec<_> = stargates_fut.map(Result::unwrap).collect().await;

        for stargate in stargates {
            let node_id = galaxy.graph.add_node(Node::Stargate {
                stargate: stargate.stargate_id,
                source: stargate.system_id,
                destination: stargate.destination.system_id,
            });
            all_stargates.insert(stargate.stargate_id, node_id);
            galaxy.stargates.insert(stargate.stargate_id, stargate);
        }

        for system in galaxy.systems.values() {
            let system_node = all_systems.get(&system.system_id).unwrap();
            let system_position: math::V3<f64> =
                math::V3::new(system.position.x, system.position.y, system.position.z);

            if let Some(system_stargates) = &system.stargates {
                for stargate_id in system_stargates {
                    let stargate = galaxy.stargates.get(stargate_id).unwrap();
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

                    galaxy
                        .graph
                        .add_edge(system_node.clone(), stargate_node.clone(), edge);

                    for stargate_id_inner in system_stargates {
                        if stargate_id >= stargate_id_inner {
                            continue;
                        }

                        let stargate_inner_node = all_stargates.get(&stargate_id_inner).unwrap();
                        let stargate_inner = galaxy.stargates.get(stargate_id_inner).unwrap();
                        let stargate_inner_position: math::V3<f64> = math::V3::new(
                            stargate_inner.position.x,
                            stargate_inner.position.y,
                            stargate_inner.position.z,
                        );

                        let edge = Edge::Warp {
                            system: system.system_id,
                            distance: stargate_position.distance(&stargate_inner_position) / 1e12,
                        };

                        galaxy.graph.add_edge(
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

                        galaxy.graph.add_edge(
                            stargate_node.clone(),
                            destination_node.clone(),
                            edge,
                        );
                    }
                }
            }
        }

        if file_exists("bridges.tsv") {
            let bridges = read_file("bridges.tsv").await.unwrap();
            let bridges_tsv = String::from_utf8(bridges).unwrap();

            let mut jb_id = 0;
            for line in bridges_tsv.lines() {
                let line_parts: Vec<_> = line.split('\t').collect();
                let left = line_parts[1].split(' ').next().unwrap();
                let right = line_parts[2].split(' ').next().unwrap();

                let left = galaxy
                    .systems_by_name
                    .get(left)
                    .and_then(|id| galaxy.systems.get(id))
                    .cloned()
                    .unwrap();
                let right = galaxy
                    .systems_by_name
                    .get(right)
                    .and_then(|id| galaxy.systems.get(id))
                    .cloned()
                    .unwrap();

                let left_jb_id = jb_id;
                let right_jb_id = jb_id + 1;
                jb_id += 2;
                let left_jb = esi::GetUniverseStargate {
                    stargate_id: left_jb_id,
                    name: format!("{} » {}", left.name, right.name),
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
                    name: format!("{} » {}", right.name, left.name),
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

                galaxy.stargates.insert(left_jb_id, left_jb);
                let left_node = Node::JumpGate {
                    stargate: left_jb_id,
                    source: left.system_id,
                    destination: right.system_id,
                };
                let left_node_id = galaxy.graph.add_node(left_node);
                all_stargates.insert(left_jb_id, left_node_id);
                let left_system_node = all_systems.get(&left.system_id).unwrap();

                galaxy.stargates.insert(right_jb_id, right_jb);
                let right_node = Node::JumpGate {
                    stargate: right_jb_id,
                    source: right.system_id,
                    destination: left.system_id,
                };
                let right_node_id = galaxy.graph.add_node(right_node);
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

                galaxy
                    .graph
                    .add_edge(left_node_id.clone(), left_system_node.clone(), left_warp);
                galaxy
                    .graph
                    .add_edge(right_node_id.clone(), right_system_node.clone(), right_warp);
                galaxy
                    .graph
                    .add_edge(left_node_id.clone(), right_node_id.clone(), edge);
            }
        }

        log::info!("galaxy loaded");

        galaxy
    }
}
