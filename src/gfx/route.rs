use crate::math;

use super::{
    font, DataEvent, GraphicsContext, GraphicsState, InputState, QuadVertex, QueryEvent,
    RouteEvent, UserEvent, Widget,
};

use font::TextAnchor;

pub struct RouteBox<'a> {
    context: &'a GraphicsContext,
    window_size: math::V2<f32>,
    player_location: Option<i32>,
    text_spans: Vec<font::PositionedTextSpan>,
    node_bounds: Vec<(i32, math::Rect<i32>)>,
    background_rect: Option<math::Rect<f32>>,
    dirty: bool,
}

impl<'a> RouteBox<'a> {
    pub fn new(context: &'a GraphicsContext) -> Self {
        RouteBox {
            context,
            window_size: math::v2(1024.0, 1024.0),
            player_location: None,
            text_spans: Vec::new(),
            node_bounds: Vec::new(),
            background_rect: None,
            dirty: true,
        }
    }

    pub fn selected_system(&self, input_state: &InputState) {
        let mut matched = false;
        for (system_id, bounds) in &self.node_bounds {
            if bounds.as_f32().contains(input_state.mouse_position) {
                input_state.send_user_event(UserEvent::RouteEvent(
                    RouteEvent::SelectedSystemChanged(Some(*system_id)),
                ));
                matched = true;
            }
        }

        if !matched {
            input_state.send_user_event(UserEvent::RouteEvent(RouteEvent::SelectedSystemChanged(
                None,
            )));
        }
    }
}

impl<'a> Widget for RouteBox<'a> {
    fn update(
        &mut self,
        _dt: std::time::Duration,
        input_state: &InputState,
        world: &crate::world::World,
    ) {
        for event in input_state.user_events() {
            match event {
                UserEvent::QueryEvent(QueryEvent::RouteChanged) => {
                    self.dirty = true;
                }
                UserEvent::DataEvent(DataEvent::SovStandingsChanged) => {
                    self.dirty = true;
                }
                UserEvent::DataEvent(DataEvent::CharacterLocationChanged(location)) => {
                    self.dirty = true;
                    self.player_location = location.clone();
                }
                _ => (),
            }
        }

        if let Some(new_size) = input_state.window_resized() {
            self.window_size = new_size.as_f32();
            self.dirty = true;
        }

        if !self.dirty {
            if input_state.mouse_move_delta() != math::V2::fill(0.0) {
                self.selected_system(input_state);
            }
            return;
        }

        self.text_spans.clear();
        self.node_bounds.clear();
        self.background_rect = None;
        let padding = 30.0;

        if world.route_nodes().len() > 0 {
            let mut background_rect = math::Rect::new(
                math::v2(padding, padding),
                math::v2(padding + 650.0, padding + 360.0),
            );

            let mut cursor = background_rect.min + math::V2::fill(padding);

            let player_on_route = self
                .player_location
                .map(|p| world.is_on_route(p))
                .unwrap_or(false);

            let mut visited = player_on_route;
            let mut last_region = None;
            let mut last_constellation = None;

            let white = math::V4::fill(1.0);

            if let Some((start, end)) = world.route_target() {
                if let (Some(start), Some(end)) = (world.system(start), world.system(end)) {
                    let mut title_text = font::TextSpan::new(50.0, self.context.ui_font, white);
                    title_text.push(format!(
                        "{} » {}: {} Jumps",
                        start.name,
                        end.name,
                        world.route_nodes().len()
                    ));

                    let title_text = self.context.font_cache.layout(
                        title_text,
                        TextAnchor::TopLeft,
                        cursor,
                        false,
                    );
                    cursor.y = title_text.bounds.max.y as f32;
                    self.text_spans.push(title_text);
                }
            }

            for node in world.route_nodes() {
                let system = world.system(node.system_id);

                if system.is_none() {
                    continue;
                }
                let system = system.unwrap();

                let constellation = world.constellation(system.constellation_id);
                let region = constellation
                    .as_ref()
                    .and_then(|c| world.region(c.region_id));
                let sov = world.sov_standing(system.system_id);
                let alliance = sov
                    .as_ref()
                    .and_then(|s| s.alliance_id)
                    .and_then(|a| world.alliance(a));

                let player_system = Some(system.system_id) == self.player_location;
                visited = !(player_system || !visited);

                let system_color = if visited && !player_system {
                    math::V3::fill(0.3).expand(1.0)
                } else {
                    white
                };

                let (jump_color, jump_text) = if player_system {
                    (math::V4::new(1.0, 0.0, 0.0, 1.0), "▶ ")
                } else if node.arrive_jump.is_some() {
                    (
                        super::jump_type_color(node.arrive_jump.as_ref().unwrap()).expand(1.0),
                        //"1·2•3∙4●5⚫6⬤78 ",
                        "● ",
                    )
                } else {
                    (system_color, "● ")
                };

                let system_sec_color = super::sec_status_color(system.security_status).expand(1.0);
                let standings_color =
                    super::standing_color(sov.map(|s| s.standing).unwrap_or(0.0)).expand(1.0);

                let mut node_text = font::TextSpan::new(30.0, self.context.symbol_font, jump_color);
                node_text
                    .push(jump_text)
                    .font(self.context.ui_font)
                    .color(system_color)
                    .push(&system.name)
                    .color(white)
                    .push(" (")
                    .color(system_sec_color)
                    .push(format!("{:.2}", system.security_status))
                    .color(white)
                    .push(") ");

                if let Some(alliance) = alliance {
                    node_text
                        .color(standings_color)
                        .push(format!("[{}] ", alliance.ticker))
                        .color(white);
                }

                if last_region != region.map(|r| r.region_id) {
                    if let (Some(constellation), Some(region)) = (constellation, region) {
                        node_text.push(format!("» {} » {} ", constellation.name, region.name));
                    }
                } else if last_constellation != constellation.map(|c| c.constellation_id) {
                    if let Some(constellation) = constellation {
                        node_text.push(format!("» {} ", constellation.name));
                    }
                }

                let node_text =
                    self.context
                        .font_cache
                        .layout(node_text, TextAnchor::TopLeft, cursor, false);
                cursor.y = node_text.bounds.max.y as f32;

                last_region = region.map(|r| r.region_id);
                last_constellation = constellation.map(|c| c.constellation_id);

                self.node_bounds.push((node.system_id, node_text.bounds));
                self.text_spans.push(node_text);
            }
            background_rect.max.y = cursor.y + padding;

            self.background_rect = Some(background_rect);
        }

        self.selected_system(input_state);
        self.context.request_redraw();
        self.dirty = false;
    }

    fn draw<S: glium::Surface>(&mut self, graphics_state: &GraphicsState, frame: &mut S) {
        if let Some(background) = self.background_rect {
            let draw_params = glium::DrawParameters {
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

            let uniforms = glium::uniform! {
                window_size: self.window_size,
                texture_atlas: self.context.images.sampler(),
                textured: false,
                color: math::v4(0.02, 0.02, 0.02, 0.85)
            };

            let mut rect_buf = Vec::new();
            for v in background.triangle_list_iter() {
                rect_buf.push(QuadVertex {
                    position: v,
                    uv: math::v2(0.0, 0.0),
                })
            }

            let rect_data_buf = glium::VertexBuffer::new(&self.context.display, &rect_buf).unwrap();

            let draw_res = frame.draw(
                &rect_data_buf,
                &glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
                &graphics_state.quad_program,
                &uniforms,
                &draw_params,
            );
            if let Err(error) = draw_res {
                log::error!("route background draw error: {:?}", error);
            }

            if self.text_spans.len() > 0 {
                let uniforms = glium::uniform! {
                    window_size: self.window_size,
                    font_atlas: self.context.font_cache.sampler()
                };

                let mut text_buf = Vec::new();

                for text in &self.text_spans {
                    self.context
                        .font_cache
                        .draw(text, &mut text_buf, self.window_size);
                }

                let text_data_buf =
                    glium::VertexBuffer::new(&self.context.display, &text_buf).unwrap();
                let draw_res = frame.draw(
                    &text_data_buf,
                    &glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
                    &graphics_state.text_program,
                    &uniforms,
                    &draw_params,
                );
                if let Err(error) = draw_res {
                    log::error!("route text draw error: {:?}", error);
                }
            }
        }
    }
}
