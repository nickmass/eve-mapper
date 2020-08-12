use crate::math;

use super::{
    font, images, DataEvent, GraphicsContext, InputState, MapEvent, RouteEvent, UserEvent, Widget,
};
use crate::platform::Frame;

use font::TextAnchor;

pub struct InfoBox<'a> {
    context: &'a GraphicsContext,
    window_size: math::V2<f32>,
    map_system: Option<i32>,
    route_system: Option<i32>,
    text_spans: Vec<font::PositionedTextSpan>,
    background_rect: Option<math::Rect<f32>>,
    image: Option<(images::Image, math::Rect<f32>)>,
    dirty: bool,
}

impl<'a> InfoBox<'a> {
    pub fn new(context: &'a GraphicsContext) -> Self {
        InfoBox {
            context,
            window_size: math::v2(1024.0, 1024.0),
            route_system: None,
            map_system: None,
            text_spans: Vec::new(),
            background_rect: None,
            image: None,
            dirty: true,
        }
    }
}

impl<'a> Widget for InfoBox<'a> {
    fn update(
        &mut self,
        _dt: std::time::Duration,
        input_state: &InputState,
        world: &crate::world::World,
    ) {
        for event in input_state.user_events() {
            match event {
                UserEvent::MapEvent(MapEvent::SelectedSystemChanged(system)) => {
                    self.map_system = system.clone();
                    self.dirty = true;
                }
                UserEvent::RouteEvent(RouteEvent::SelectedSystemChanged(system)) => {
                    self.route_system = system.clone();
                    self.dirty = true;
                }
                UserEvent::DataEvent(DataEvent::SovStandingsChanged) => {
                    self.dirty = true;
                }
                UserEvent::DataEvent(DataEvent::ImageLoaded) => {
                    self.dirty = true;
                }
                _ => (),
            }
        }

        if let Some(new_size) = input_state.window_resized() {
            self.window_size = new_size.as_f32();
            self.dirty = true;
        }

        if !self.dirty {
            return;
        }

        let ui_scale = self.context.ui_scale();
        self.text_spans.clear();
        self.background_rect = None;
        let padding = 30.0 * ui_scale;

        let selected_system = self.route_system.or(self.map_system);
        if let Some(system) = selected_system.and_then(|id| world.system(id)) {
            let constellation = world.constellation(system.constellation_id);
            let region = constellation
                .as_ref()
                .and_then(|c| world.region(c.region_id));
            let sov = world.sov_standing(system.system_id);
            let alliance = sov
                .as_ref()
                .and_then(|s| s.alliance_id)
                .and_then(|a| world.alliance(a));
            let corporation = sov
                .as_ref()
                .and_then(|s| s.corporation_id)
                .and_then(|c| world.corporation(c));
            let stats = world.stats(system.system_id);

            let image = if let Some(alliance) = alliance.as_ref() {
                let image = images::Image::AllianceLogo(alliance.alliance_id);
                if !self.context.images.contains(image) {
                    if let Some(data) = world.alliance_logo(alliance.alliance_id) {
                        match self
                            .context
                            .images
                            .load(&self.context.display, image, &data)
                        {
                            Err(e) => {
                                log::error!("image load error {:?}: {:?}", image, e);
                                None
                            }
                            Ok(_) => Some(image),
                        }
                    } else {
                        None
                    }
                } else {
                    Some(image)
                }
            } else {
                None
            };

            let system_sec_color = super::sec_status_color(system.security_status).expand(1.0);

            let mut background_rect = math::Rect::new(
                math::v2(self.window_size.x - padding - (650.0 * ui_scale), padding),
                math::v2(self.window_size.x - padding, padding + (360.0 * ui_scale)),
            );
            let image_rect = math::Rect::new(
                background_rect.min + math::V2::fill(padding),
                background_rect.min + math::V2::fill(padding + (128.0 * ui_scale)),
            );

            let system_name_pos = if let Some(_) = image.as_ref() {
                math::v2(padding + image_rect.max.x, padding + background_rect.min.y)
            } else {
                background_rect.min + math::V2::fill(padding)
            };

            let white = math::V4::fill(1.0);

            let mut system_name =
                font::TextSpan::new(90.0 * ui_scale, self.context.title_font, white);
            system_name.push(&system.name);
            let system_name = self.context.font_cache.layout(
                system_name,
                TextAnchor::TopLeft,
                system_name_pos,
                false,
            );

            let mut system_sec = font::TextSpan::new(40.0 * ui_scale, self.context.ui_font, white);
            system_sec
                .push(" (")
                .color(system_sec_color)
                .push(format!("{:.2}", system.security_status))
                .color(white)
                .push(")");
            let system_sec = self.context.font_cache.layout(
                system_sec,
                TextAnchor::TopLeft,
                math::v2(
                    system_name.bounds.max.x as f32,
                    system_name.bounds.min.y as f32,
                ),
                false,
            );

            let mut cursor = if image.is_some() {
                math::v2(background_rect.min.x + padding, image_rect.max.y as f32)
            } else {
                math::v2(
                    background_rect.min.x + padding,
                    system_name.bounds.max.y as f32,
                )
            };

            let region_name = if let (Some(region), Some(constellation)) = (region, constellation) {
                let mut region_span =
                    font::TextSpan::new(30.0 * ui_scale, self.context.ui_font, white);
                region_span.push(format!("{} « {}", region.name, constellation.name));
                let region = self.context.font_cache.layout(
                    region_span,
                    TextAnchor::TopLeft,
                    math::v2(
                        system_name.bounds.min.x as f32,
                        system_name.bounds.max.y as f32,
                    ),
                    false,
                );

                cursor.y = cursor.y.max(region.bounds.max.y as f32);

                Some(region)
            } else {
                None
            };

            let standing_color =
                super::standing_color(sov.map(|s| s.standing).unwrap_or(0.0)).expand(1.0);

            let alliance_name = if let Some(alliance) = alliance {
                let mut alliance_span =
                    font::TextSpan::new(30.0 * ui_scale, self.context.symbol_font, standing_color);
                alliance_span
                    .push("● ")
                    .color(white)
                    .font(self.context.ui_font)
                    .push(format!("{} [{}]", alliance.name, alliance.ticker));
                let alliance = self.context.font_cache.layout(
                    alliance_span,
                    TextAnchor::TopLeft,
                    cursor,
                    false,
                );

                cursor.y = alliance.bounds.max.y as f32;

                Some(alliance)
            } else {
                None
            };

            let corporation_name = if let Some(corporation) = corporation {
                let mut corporation_span =
                    font::TextSpan::new(30.0 * ui_scale, self.context.symbol_font, standing_color);
                corporation_span
                    .push("● ")
                    .color(white)
                    .font(self.context.ui_font)
                    .push(format!("{} [{}]", corporation.name, corporation.ticker));
                let corporation = self.context.font_cache.layout(
                    corporation_span,
                    TextAnchor::TopLeft,
                    cursor,
                    false,
                );

                cursor.y = corporation.bounds.max.y as f32;

                Some(corporation)
            } else {
                None
            };

            let stats = if let Some(stats) = stats {
                cursor.y = cursor.y + padding;
                let mut jumps = font::TextSpan::new(30.0 * ui_scale, self.context.ui_font, white);
                let mut ships = font::TextSpan::new(30.0 * ui_scale, self.context.ui_font, white);
                let mut pods = font::TextSpan::new(30.0 * ui_scale, self.context.ui_font, white);
                let mut npcs = font::TextSpan::new(30.0 * ui_scale, self.context.ui_font, white);

                jumps.push(format!("Jumps: {}", stats.jumps));
                ships.push(format!("Ship Kills: {}", stats.ship_kills));
                pods.push(format!("Pod Kills: {}", stats.pod_kills));
                npcs.push(format!("NPC Kills: {}", stats.npc_kills));

                let right_column_offset = math::v2(background_rect.width() / 2.0, 0.0);

                let jumps =
                    self.context
                        .font_cache
                        .layout(jumps, TextAnchor::TopLeft, cursor, false);
                let pods = self.context.font_cache.layout(
                    pods,
                    TextAnchor::TopLeft,
                    cursor + right_column_offset,
                    false,
                );

                cursor.y = jumps.bounds.max.y as f32;

                let ships =
                    self.context
                        .font_cache
                        .layout(ships, TextAnchor::TopLeft, cursor, false);
                let npcs = self.context.font_cache.layout(
                    npcs,
                    TextAnchor::TopLeft,
                    cursor + right_column_offset,
                    false,
                );

                cursor.y = ships.bounds.max.y as f32;

                vec![jumps, pods, ships, npcs]
            } else {
                Vec::new()
            };

            cursor.y = cursor.y + padding;
            background_rect.max.y = cursor.y;

            self.background_rect = Some(background_rect);
            self.image = image.map(|i| (i, image_rect));
            self.text_spans.push(system_name);
            self.text_spans.push(system_sec);
            if let Some(region) = region_name {
                self.text_spans.push(region);
            };
            if let Some(alliance) = alliance_name {
                self.text_spans.push(alliance);
            };
            if let Some(corporation) = corporation_name {
                self.text_spans.push(corporation);
            };
            for stat in stats {
                self.text_spans.push(stat);
            }
        }

        self.context.request_redraw("info dirty");
        self.dirty = false;
    }

    fn draw(&mut self, frame: &mut Frame) {
        if let Some(background) = self.background_rect {
            self.context.display.draw_quad(
                frame,
                &self.context.images,
                math::v4(0.02, 0.02, 0.02, 0.85),
                background,
            );

            if let Some((image, position)) = self.image {
                self.context
                    .display
                    .draw_image(frame, &self.context.images, image, position);
            }

            if self.text_spans.len() > 0 {
                self.context.display.draw_text(
                    frame,
                    &self.context.font_cache,
                    &self.text_spans,
                    self.context.ui_scale(),
                );
            }
        }
    }
}
