use crate::math;
use crate::world::{JumpType, World};

use super::{
    DataEvent, GraphicsContext, GraphicsState, InputState, LineVertex, MapEvent, MouseButton,
    QueryEvent, SystemData, UserEvent, Widget,
};

use std::collections::{HashMap, HashSet};
use std::time::Duration;

#[derive(Copy, Clone, Debug, PartialEq)]
enum RegionNamesLayer {
    Foreground,
    Background,
}

struct MapSystem {
    system_id: i32,
    name: String,
    position: math::V2<f32>,
    security_status: f64,
    sovereignty_standing: Option<f64>,
}

struct MapJump {
    left_system_id: i32,
    right_system_id: i32,
    jump_type: JumpType,
    on_route: bool,
}

pub struct Map<'a> {
    context: &'a GraphicsContext,
    map_systems: Option<HashMap<i32, MapSystem>>,
    map_jumps: Option<Vec<MapJump>>,
    system_vertexes: Option<Vec<SystemData>>,
    jump_vertexes: Option<Vec<LineVertex>>,
    selected_system: Option<i32>,
    focused_systems: HashSet<i32>,
    systems_vertex_buffer: Option<glium::VertexBuffer<SystemData>>,
    jumps_vertex_buffer: Option<glium::VertexBuffer<LineVertex>>,
    system_draw_params: glium::DrawParameters<'static>,
    jump_draw_params: glium::DrawParameters<'static>,
    current_zoom: f32,
    target_zoom: f32,
    scale_matrix: math::M3<f32>,
    view_matrix: math::M3<f32>,
    window_size: math::V2<f32>,
    map_offset: math::V2<f32>,
    system_magnitude: f64,
    region_names: Vec<super::TextNode>,
    region_names_layer: Option<RegionNamesLayer>,
    system_names: Vec<super::TextNode>,
    player_location: Option<i32>,
}

impl<'a> Map<'a> {
    pub fn new(context: &'a GraphicsContext) -> Self {
        let system_draw_params = glium::DrawParameters {
            blend: glium::Blend {
                color: glium::BlendingFunction::Addition {
                    source: glium::LinearBlendingFactor::SourceAlpha,
                    destination: glium::LinearBlendingFactor::OneMinusSourceAlpha,
                },
                alpha: glium::BlendingFunction::Addition {
                    source: glium::LinearBlendingFactor::Zero,
                    destination: glium::LinearBlendingFactor::One,
                },
                constant_value: (1.0, 1.0, 1.0, 1.0),
            },
            ..Default::default()
        };

        let jump_draw_params = glium::DrawParameters {
            blend: glium::Blend {
                color: glium::BlendingFunction::Addition {
                    source: glium::LinearBlendingFactor::SourceAlpha,
                    destination: glium::LinearBlendingFactor::OneMinusSourceAlpha,
                },
                alpha: glium::BlendingFunction::Addition {
                    source: glium::LinearBlendingFactor::Zero,
                    destination: glium::LinearBlendingFactor::One,
                },
                constant_value: (1.0, 1.0, 1.0, 1.0),
            },
            depth: glium::Depth {
                test: glium::DepthTest::IfMoreOrEqual,
                write: true,
                ..Default::default()
            },
            ..Default::default()
        };
        Map {
            context,
            map_systems: None,
            map_jumps: None,
            system_vertexes: None,
            jump_vertexes: None,
            selected_system: None,
            focused_systems: HashSet::new(),
            systems_vertex_buffer: None,
            jumps_vertex_buffer: None,
            system_draw_params,
            jump_draw_params,
            current_zoom: 1.0,
            target_zoom: 1.0,
            scale_matrix: math::M3::identity(),
            view_matrix: math::M3::identity(),
            window_size: math::v2(1024.0, 1024.0),
            map_offset: math::V2::fill(0.0),
            system_magnitude: 0.0,
            region_names: Vec::new(),
            region_names_layer: Some(RegionNamesLayer::Foreground),
            system_names: Vec::new(),
            player_location: None,
        }
    }
}

impl<'a> Widget for Map<'a> {
    fn update(&mut self, dt: Duration, input_state: &InputState, world: &World) {
        for event in input_state.user_events() {
            match event {
                UserEvent::DataEvent(DataEvent::CharacterLocationChanged(location)) => {
                    self.player_location = location.clone();
                    self.system_vertexes = None;
                }
                UserEvent::QueryEvent(QueryEvent::RouteChanged) => {
                    self.map_jumps = None;
                }
                UserEvent::QueryEvent(QueryEvent::SystemsFocused(systems)) => {
                    self.focused_systems = systems.clone();
                    self.system_vertexes = None;
                }
                _ => (),
            }
        }

        let mut text_dirty = false;

        if let Some(new_size) = input_state.window_resized() {
            self.window_size = new_size.as_f32();
            text_dirty = true;
        }

        let window_scale = if self.window_size.x > self.window_size.y {
            math::v2(self.window_size.x / self.window_size.y, 1.0)
        } else if self.window_size.y > self.window_size.x {
            math::v2(1.0, self.window_size.y / self.window_size.x)
        } else {
            math::v2(1.0, 1.0)
        };

        let window_ratio = if self.window_size.x > self.window_size.y {
            math::v2(self.window_size.y / self.window_size.x, 1.0)
        } else if self.window_size.y > self.window_size.x {
            math::v2(1.0, self.window_size.x / self.window_size.y)
        } else {
            math::v2(1.0, 1.0)
        };

        self.target_zoom += (self.target_zoom * input_state.scroll()) / -20.0;
        if self.target_zoom < 0.25 {
            self.target_zoom = 0.25;
        } else if self.target_zoom > 100.0 {
            self.target_zoom = 100.0;
        }

        let zoom_diff = (self.current_zoom - self.target_zoom).abs() / 10.0;
        if zoom_diff > f32::EPSILON * self.current_zoom {
            if self.target_zoom > self.current_zoom {
                self.current_zoom += zoom_diff.min(self.current_zoom / 20.0);
            } else if self.target_zoom < self.current_zoom {
                self.current_zoom -= zoom_diff.min(self.current_zoom / 20.0);
            }
            text_dirty = true;
        } else if self.current_zoom != self.target_zoom {
            self.current_zoom = self.target_zoom;
            text_dirty = true;
        }

        if input_state.is_mouse_down(MouseButton::Left)
            && input_state.mouse_move_delta() != math::V2::fill(0.0)
        {
            self.map_offset = self.map_offset
                + ((input_state.mouse_move_delta() * 2.0) / self.window_size)
                    / window_ratio
                    / self.current_zoom;
            text_dirty = true;
        }

        self.view_matrix = math::M3::<f32>::identity();
        self.view_matrix.c0.x = self.current_zoom;
        self.view_matrix.c1.y = self.current_zoom;
        self.view_matrix.c2.x = -self.map_offset.x * self.current_zoom;
        self.view_matrix.c2.y = self.map_offset.y * self.current_zoom;

        self.scale_matrix = math::M3::<f32>::identity();
        self.scale_matrix.c0.x = 1.0 / window_scale.x;
        self.scale_matrix.c1.y = 1.0 / window_scale.y;

        let mut text_view_matrix = math::M3::<f32>::identity();
        text_view_matrix.c0.x = self.current_zoom;
        text_view_matrix.c1.y = self.current_zoom;
        text_view_matrix.c2.x = -self.map_offset.x * self.current_zoom;
        text_view_matrix.c2.y = self.map_offset.y * self.current_zoom;

        let mut text_scale_matrix = math::M3::<f32>::identity();
        text_scale_matrix.c0.x = 1.0 / window_scale.x;
        text_scale_matrix.c1.y = 1.0 / window_scale.y;

        let mut text_screen_matrix = math::M3::<f32>::identity();
        text_screen_matrix.c0.x = self.window_size.x / 2.0;
        text_screen_matrix.c1.y = -self.window_size.y / 2.0;
        text_screen_matrix.c2.x = self.window_size.x / 2.0;
        text_screen_matrix.c2.y = self.window_size.y / 2.0;

        let text_transform = text_screen_matrix * text_scale_matrix * text_view_matrix;

        if input_state.mouse_move_delta() != math::V2::fill(0.0) {
            let mut selected_system = None;

            if let Some(systems) = &self.map_systems {
                for system in systems.values() {
                    let position = (text_transform * system.position.expand(1.0)).collapse();
                    if position.distance(&input_state.mouse_position) < self.current_zoom {
                        selected_system = Some(system.system_id);
                        break;
                    }
                }
            }

            if selected_system != self.selected_system {
                self.selected_system = selected_system;
                input_state.send_user_event(UserEvent::MapEvent(MapEvent::SelectedSystemChanged(
                    selected_system,
                )));

                self.system_vertexes = None;
            }
        }

        if self.map_systems.is_none() {
            let max_magnitude = world
                .iter()
                .filter(|s| s.system_id < 30050000)
                .map(|s| math::v3(s.position.x, s.position.z, s.position.y).magnitude())
                .max_by(|a, b| {
                    if a > b {
                        std::cmp::Ordering::Greater
                    } else {
                        std::cmp::Ordering::Less
                    }
                })
                .unwrap_or(1.0);

            let map_systems = world
                .iter()
                .filter(|s| s.system_id < 30050000)
                .map(|s| {
                    let position = math::v2(s.position.x, s.position.z);
                    let position = (position / max_magnitude).as_f32();
                    let sovereignty_standing = world.sov_standing(s.system_id);

                    (
                        s.system_id,
                        MapSystem {
                            system_id: s.system_id,
                            name: s.name.to_string(),
                            position,
                            security_status: s.security_status,
                            sovereignty_standing,
                        },
                    )
                })
                .collect();

            self.system_magnitude = max_magnitude;
            self.map_systems = Some(map_systems);
            self.jump_vertexes = None;
            self.system_vertexes = None;
            text_dirty = true;
        }

        if self.map_jumps.is_none() {
            let map_jumps = world
                .jumps()
                .iter()
                .map(|j| {
                    let on_route =
                        world.is_on_route(j.left_system_id) && world.is_on_route(j.right_system_id);
                    MapJump {
                        left_system_id: j.left_system_id,
                        right_system_id: j.right_system_id,
                        jump_type: j.jump_type,
                        on_route,
                    }
                })
                .collect();
            self.map_jumps = Some(map_jumps);
            self.jump_vertexes = None;
        }

        if text_dirty {
            self.region_names_layer = if self.current_zoom >= 15.0 {
                Some(RegionNamesLayer::Background)
            } else if self.current_zoom > 1.0 {
                Some(RegionNamesLayer::Foreground)
            } else {
                None
            };

            if let Some(layer) = &self.region_names_layer {
                let alpha = if self.current_zoom >= 1.0 && self.current_zoom < 2.0 {
                    1.0 - (2.0 - self.current_zoom)
                } else if self.current_zoom >= 10.0 && self.current_zoom < 15.0 {
                    (15.0 - self.current_zoom) / 5.0
                } else if self.current_zoom >= 15.0 && self.current_zoom < 25.0 {
                    1.0 - (25.0 - self.current_zoom) / 5.0
                } else {
                    1.0
                };

                let (font, scale, color, shadow) = match layer {
                    RegionNamesLayer::Background => (
                        self.context.title_font,
                        110.0,
                        math::V3::fill(0.02).expand(alpha),
                        false,
                    ),
                    RegionNamesLayer::Foreground => (
                        self.context.ui_font,
                        50.0,
                        math::V3::fill(1.0).expand(alpha),
                        true,
                    ),
                };

                self.region_names.clear();
                for region in world.regions.values() {
                    if let Some(constellations) = region.constellations.as_ref() {
                        let (positions, count) = constellations
                            .iter()
                            .filter_map(|c| world.constellations.get(c))
                            .map(|constellation| {
                                let position =
                                    math::v2(constellation.position.x, constellation.position.z);
                                let position = (position / self.system_magnitude).as_f32();
                                position
                            })
                            .fold((math::V2::fill(0.0), 0), |acc, position| {
                                (acc.0 + position, acc.1 + 1)
                            });

                        let position = positions / (count as f32);
                        let position = (text_transform * position.expand(1.0)).collapse();

                        self.region_names.push(super::TextNode {
                            text: region.name.to_string(),
                            font,
                            position,
                            anchor: super::font::TextAnchor::Center,
                            scale,
                            color,
                            shadow,
                        })
                    }
                }
            }

            self.system_names.clear();
            if self.current_zoom > 15.0 {
                let alpha = ((self.current_zoom - 15.0) / (25.0 - 15.0)).min(1.0);

                if let Some(systems) = self.map_systems.as_ref() {
                    for system in systems.values() {
                        let pos = (text_transform * system.position.expand(1.0)).collapse();

                        let min_corner = pos - 50.0;
                        let max_corner = pos + 50.0;

                        if max_corner.x < 0.0
                            || max_corner.y < 0.0
                            || min_corner.x > self.window_size.x
                            || min_corner.y > self.window_size.y
                        {
                            continue;
                        }

                        let color = math::V3::fill(0.6);

                        let pos = pos + math::V2::fill(0.2 * self.current_zoom.min(50.0));

                        let node = super::TextNode {
                            text: system.name.to_string(),
                            font: self.context.ui_font,
                            scale: 25.0,
                            position: pos,
                            anchor: super::font::TextAnchor::TopLeft,
                            color: color.expand(alpha),
                            shadow: true,
                        };

                        self.system_names.push(node);
                    }
                }
            }

            self.context.request_redraw()
        }

        if self.jump_vertexes.is_none() {
            if let (Some(map_jumps), Some(map_systems)) =
                (self.map_jumps.as_ref(), self.map_systems.as_ref())
            {
                let mut jump_vertexes = Vec::with_capacity(world.jumps().len() * 6);
                for jump in map_jumps {
                    let left_system = map_systems.get(&jump.left_system_id);
                    let right_system = map_systems.get(&jump.right_system_id);

                    if left_system.is_none() || right_system.is_none() {
                        continue;
                    }

                    let left_system = left_system.unwrap();
                    let right_system = right_system.unwrap();

                    let (left_color, right_color) = match (
                        jump.jump_type,
                        jump.on_route,
                        left_system.security_status,
                        right_system.security_status,
                    ) {
                        (_, true, left, right) => (
                            super::sec_status_color(left),
                            super::sec_status_color(right),
                        ),
                        (jump, false, _, _) => {
                            (super::jump_type_color(&jump), super::jump_type_color(&jump))
                        }
                    };

                    let level = if jump.on_route { 1.0 } else { 0.0 };

                    let jump_left = left_system.position.expand(level);
                    let jump_right = right_system.position.expand(level);

                    let left_norm =
                        math::v2(-(jump_left.y - jump_right.y), jump_left.x - jump_right.x)
                            .normalize();
                    let right_norm =
                        math::v2(jump_left.y - jump_right.y, -(jump_left.x - jump_right.x))
                            .normalize();

                    jump_vertexes.push(LineVertex {
                        position: jump_left,
                        color: left_color,
                        normal: left_norm,
                    });

                    jump_vertexes.push(LineVertex {
                        position: jump_right,
                        color: right_color,
                        normal: right_norm,
                    });

                    jump_vertexes.push(LineVertex {
                        position: jump_left,
                        color: left_color,
                        normal: right_norm,
                    });

                    jump_vertexes.push(LineVertex {
                        position: jump_right,
                        color: right_color,
                        normal: right_norm,
                    });

                    jump_vertexes.push(LineVertex {
                        position: jump_right,
                        color: right_color,
                        normal: left_norm,
                    });

                    jump_vertexes.push(LineVertex {
                        position: jump_left,
                        color: left_color,
                        normal: left_norm,
                    });
                }

                self.jump_vertexes = Some(jump_vertexes);
                self.jumps_vertex_buffer = None;
            }
        }

        if self.system_vertexes.is_none() {
            if let Some(systems) = self.map_systems.as_ref() {
                let system_vertexes = systems
                    .values()
                    .map(|system| {
                        let is_selected = Some(system.system_id) == self.selected_system;
                        let is_focused = self.focused_systems.contains(&system.system_id);
                        let is_player_system = Some(system.system_id) == self.player_location;
                        let highlight = if is_player_system {
                            math::v4(0.0, 1.0, 1.0, 1.0)
                        } else if is_focused || is_selected {
                            math::v4(1.0, 1.0, 1.0, 1.0)
                        } else {
                            math::V4::fill(0.0)
                        };

                        let alpha = if self.focused_systems.len() == 0 || is_focused || is_selected
                        {
                            1.0
                        } else {
                            0.1
                        };

                        let scale = if is_player_system {
                            3.0
                        } else if is_focused {
                            2.0
                        } else {
                            1.0
                        };

                        let color = if system.security_status <= 0.0 {
                            if let Some(sov) = system.sovereignty_standing {
                                super::standing_color(sov)
                            } else {
                                super::sec_status_color(system.security_status)
                            }
                        } else {
                            super::sec_status_color(system.security_status)
                        };

                        SystemData {
                            center: system.position,
                            highlight,
                            color: color.expand(alpha),
                            system_id: system.system_id,
                            scale,
                            radius: 5.0,
                        }
                    })
                    .collect();

                self.system_vertexes = Some(system_vertexes);
                self.systems_vertex_buffer = None;
            }
        }

        if self.systems_vertex_buffer.is_none() {
            if let Some(vertexes) = self.system_vertexes.as_ref() {
                let system_data = glium::VertexBuffer::new(&self.context.display, &vertexes);
                match system_data {
                    Ok(buf) => self.systems_vertex_buffer = Some(buf),
                    Err(error) => {
                        log::error!("failed to create map systems vertex buffer: {}", error)
                    }
                }

                self.context.request_redraw()
            }
        }

        if self.jumps_vertex_buffer.is_none() {
            if let Some(vertexes) = self.jump_vertexes.as_ref() {
                let jump_data = glium::VertexBuffer::new(&self.context.display, &vertexes);
                match jump_data {
                    Ok(buf) => self.jumps_vertex_buffer = Some(buf),
                    Err(error) => {
                        log::error!("failed to create map jumps vertex buffer: {}", error)
                    }
                }

                self.context.request_redraw()
            }
        }
    }

    fn draw<S: glium::Surface>(&mut self, graphics_state: &GraphicsState, frame: &mut S) {
        let font_atlas_sampler = graphics_state
            .font_cache
            .texture()
            .sampled()
            .magnify_filter(glium::uniforms::MagnifySamplerFilter::Nearest)
            .minify_filter(glium::uniforms::MinifySamplerFilter::Nearest);

        let uniforms = glium::uniform! {
            map_scale_matrix: self.scale_matrix,
            map_view_matrix: self.view_matrix,
            zoom: self.current_zoom,
            window_size: self.window_size,
            font_atlas: font_atlas_sampler
        };

        if self.region_names_layer == Some(RegionNamesLayer::Background)
            && self.region_names.len() > 0
        {
            let mut text_buf = Vec::new();
            for text in self.region_names.iter() {
                graphics_state
                    .font_cache
                    .prepare(
                        text.font,
                        text.text.as_str(),
                        &mut text_buf,
                        text.scale,
                        text.anchor,
                        text.position,
                        text.color,
                        self.window_size,
                        text.shadow,
                    )
                    .unwrap();
            }

            let text_data_buf = glium::VertexBuffer::new(&self.context.display, &text_buf).unwrap();

            let draw_res = frame.draw(
                &text_data_buf,
                &glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
                &graphics_state.text_program,
                &uniforms,
                &self.system_draw_params,
            );

            if let Err(error) = draw_res {
                log::error!("map region names draw error: {:?}", error);
            }
        }

        if let Some(jump_data) = self.jumps_vertex_buffer.as_ref() {
            let draw_res = frame.draw(
                jump_data,
                &glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
                &graphics_state.jump_program,
                &uniforms,
                &self.jump_draw_params,
            );

            if let Err(error) = draw_res {
                log::error!("map jump draw error: {:?}", error);
            }
        }

        if let Some(system_data) = self.systems_vertex_buffer.as_ref() {
            let draw_res = frame.draw(
                (
                    &graphics_state.circle_model,
                    system_data.per_instance().unwrap(),
                ),
                &glium::index::NoIndices(glium::index::PrimitiveType::TriangleFan),
                &graphics_state.system_program,
                &uniforms,
                &self.system_draw_params,
            );

            if let Err(error) = draw_res {
                log::error!("map system draw error: {:?}", error);
            }
        }

        if self.system_names.len() > 0 {
            let mut text_buf = Vec::new();
            for text in self.system_names.iter() {
                graphics_state
                    .font_cache
                    .prepare(
                        text.font,
                        text.text.as_str(),
                        &mut text_buf,
                        text.scale,
                        text.anchor,
                        text.position,
                        text.color,
                        self.window_size,
                        text.shadow,
                    )
                    .unwrap();
            }

            let text_data_buf = glium::VertexBuffer::new(&self.context.display, &text_buf).unwrap();

            let draw_res = frame.draw(
                &text_data_buf,
                &glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
                &graphics_state.text_program,
                &uniforms,
                &self.system_draw_params,
            );

            if let Err(error) = draw_res {
                log::error!("map system names draw error: {:?}", error);
            }
        }

        if self.region_names_layer == Some(RegionNamesLayer::Foreground)
            && self.region_names.len() > 0
        {
            let mut text_buf = Vec::new();
            for text in self.region_names.iter() {
                graphics_state
                    .font_cache
                    .prepare(
                        text.font,
                        text.text.as_str(),
                        &mut text_buf,
                        text.scale,
                        text.anchor,
                        text.position,
                        text.color,
                        self.window_size,
                        text.shadow,
                    )
                    .unwrap();
            }

            let text_data_buf = glium::VertexBuffer::new(&self.context.display, &text_buf).unwrap();

            let draw_res = frame.draw(
                &text_data_buf,
                &glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
                &graphics_state.text_program,
                &uniforms,
                &self.system_draw_params,
            );

            if let Err(error) = draw_res {
                log::error!("map region names draw error: {:?}", error);
            }
        }
    }
}
