use ahash::AHashSet as HashSet;
use winit::event::{MouseButton, VirtualKeyCode};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use crate::math;
use crate::platform::time::Instant;
use crate::platform::{create_event_proxy, spawn, Frame, GraphicsBackend, DEFAULT_CONTROL_FLOW};
use crate::world::{Galaxy, JumpType, World};

pub mod font;
pub mod images;

pub use crate::input::{InputState, UserEventReceiver, UserEventSender};

mod map;
use map::Map;

mod info;
use info::InfoBox;

mod route;
use route::RouteBox;

#[derive(Clone, Debug)]
pub enum UserEvent {
    DataEvent(DataEvent),
    MapEvent(MapEvent),
    QueryEvent(QueryEvent),
    RouteEvent(RouteEvent),
    FrameDrawn,
}

#[derive(Clone, Debug)]
pub enum DataEvent {
    CharacterLocationChanged(Option<i32>),
    SovStandingsChanged,
    SystemStatsChanged,
    ImageLoaded,
    GalaxyLoaded(Galaxy),
    GalaxyImported,
}

#[derive(Clone, Debug)]
pub enum MapEvent {
    SelectedSystemChanged(Option<i32>),
}

#[derive(Clone, Debug)]
pub enum RouteEvent {
    SelectedSystemChanged(Option<i32>),
}

#[derive(Clone, Debug)]
pub enum QueryEvent {
    SystemsFocused(HashSet<i32>),
    RouteChanged,
}

struct UserState {
    window_size: math::V2<f32>,
    query_string: String,
    text_nodes: Vec<font::PositionedTextSpan>,
}

pub struct GraphicsContext {
    pub display: GraphicsBackend,
    pub ui_font: font::FontId,
    pub title_font: font::FontId,
    pub symbol_font: font::FontId,
    pub font_cache: font::FontCache,
    pub images: images::Images,
    ui_scale: Cell<f32>,
}

impl GraphicsContext {
    pub fn request_redraw(&self, cause: &'static str) {
        log::debug!("requested redraw: {}", cause);
        self.display.request_redraw()
    }

    pub fn set_ui_scale(&self, window_size: math::V2<f32>) {
        self.ui_scale.set(window_size.y / 2160.0);
    }

    pub fn ui_scale(&self) -> f32 {
        self.ui_scale.get()
    }

    pub fn window_size(&self) -> math::V2<f32> {
        self.display.window_size()
    }
}

pub struct Window {
    event_loop: EventLoop<UserEvent>,
    user_state: UserState,
    graphics_context: Rc<GraphicsContext>,
}

impl Window {
    pub fn new(width: u32, height: u32) -> Self {
        let event_loop = EventLoop::with_user_event();
        let w_builder = WindowBuilder::new()
            .with_inner_size(winit::dpi::LogicalSize::new(width, height))
            .with_transparent(false)
            .with_title("EVE Mapper");
        let display = GraphicsBackend::new(w_builder, &event_loop, width, height);

        let mut font_cache = font::FontCache::new(&display, 1024, 1024);
        let ui_font = font_cache.load::<font::EveSansNeue>().unwrap();
        let title_font = font_cache.load::<font::EveSansNeueBold>().unwrap();
        let symbol_font = font_cache.load::<font::NanumGothic>().unwrap();

        let images = images::Images::new(&display, 4096, 4096);

        let graphics_context = Rc::new(GraphicsContext {
            display,
            ui_font,
            title_font,
            symbol_font,
            font_cache,
            images,
            ui_scale: Cell::new(1.0),
        });

        graphics_context.set_ui_scale(math::v2(width, height).as_f32());

        let user_state = UserState {
            query_string: String::new(),
            window_size: math::v2(1024.0, 1024.0),
            text_nodes: Vec::new(),
        };

        Window {
            event_loop,
            graphics_context,
            user_state,
        }
    }

    pub fn run(self) -> ! {
        let (event_sender, event_receiver) = create_event_proxy(&self.event_loop);

        let mut world = World::new(event_sender.clone());
        spawn({
            let event_sender = event_sender.clone();
            async move {
                let galaxy = crate::world::Galaxy::load().await;
                let _ = event_sender
                    .send_user_event(UserEvent::DataEvent(DataEvent::GalaxyLoaded(galaxy)));
            }
        });

        let mut user_state = self.user_state;

        let graphics_context = self.graphics_context.clone();
        let mut map = Map::new(graphics_context.clone());
        let mut info_box = InfoBox::new(graphics_context.clone());
        let mut route_box = RouteBox::new(graphics_context.clone());

        let window_size = math::v2(
            graphics_context.window_size().x as u32,
            graphics_context.window_size().y as u32,
        );
        let mut input_state = InputState::new(event_sender, event_receiver, window_size);

        let mut frame_time = Instant::now();

        self.event_loop.run(move |event, _window, control_flow| {
            use winit::event::*;
            match event {
                Event::NewEvents(_) => {}
                Event::MainEventsCleared => {
                    //exists for wasm-web-sys builds where EventLoopProxys do not work and cannot send events to the main loop directly
                    for event in input_state.received_user_events() {
                        match event {
                            UserEvent::DataEvent(DataEvent::GalaxyLoaded(galaxy)) => {
                                world.import(galaxy)
                            }
                            event => input_state.push_user_event(event),
                        }
                    }

                    let dt = frame_time.elapsed();

                    if let Some(window_size) = input_state.window_resized() {
                        graphics_context.set_ui_scale(window_size.as_f32());
                        graphics_context
                            .display
                            .update_window_size(window_size.as_f32());
                    }

                    Window::update(
                        dt,
                        &input_state,
                        &mut world,
                        &graphics_context,
                        &mut user_state,
                    );
                    info_box.update(dt, &input_state, &world);
                    route_box.update(dt, &input_state, &world);
                    map.update(dt, &input_state, &world);

                    frame_time = Instant::now();

                    *control_flow = if input_state.closed() {
                        ControlFlow::Exit
                    } else {
                        DEFAULT_CONTROL_FLOW
                    };

                    input_state.reset();
                }
                Event::RedrawRequested(..) => {
                    let mut frame = graphics_context.display.begin();
                    frame.clear_color(math::v4(0.0, 0.0, 0.0, 1.0));
                    frame.clear_depth(0.0);

                    graphics_context
                        .font_cache
                        .fill_glyph_cache(&graphics_context.display);

                    map.draw(&mut frame);
                    route_box.draw(&mut frame);
                    info_box.draw(&mut frame);

                    Window::draw(&mut frame, &graphics_context, &user_state);

                    graphics_context.display.end(frame);

                    //Send this event to ensure we run the updates for the next frame to continue any animations that may be ongoing
                    input_state.send_user_event(UserEvent::FrameDrawn);
                }
                Event::UserEvent(UserEvent::DataEvent(DataEvent::GalaxyLoaded(galaxy))) => {
                    world.import(galaxy);
                }
                Event::RedrawEventsCleared => {}
                Event::LoopDestroyed => {}
                event => input_state.process(event),
            }
        })
    }

    fn update(
        _dt: Duration,
        input_state: &InputState,
        world: &mut World,
        graphics_context: &GraphicsContext,
        user_state: &mut UserState,
    ) {
        let mut query_changed = false;

        if input_state.text().len() > 0 {
            user_state.query_string.push_str(input_state.text());
            query_changed = true;
            graphics_context.request_redraw("query text");
        }

        if input_state.was_key_down(VirtualKeyCode::Return) {
            let parts: Vec<_> = user_state.query_string.split(' ').collect();

            if user_state.query_string.len() == 0 {
                input_state.send_user_event(UserEvent::QueryEvent(QueryEvent::SystemsFocused(
                    HashSet::new(),
                )))
            } else if parts.len() == 2 {
                let from = world.match_system(parts[0]).into_iter().next();
                let to = world.match_system(parts[1]).into_iter().next();

                match (from, to) {
                    (Some(from), Some(to)) => {
                        world.create_route(from, to);
                        if input_state.is_key_down(VirtualKeyCode::LShift)
                            | input_state.is_key_down(VirtualKeyCode::RShift)
                        {
                            world.send_route_to_client();
                        }
                        input_state.send_user_event(UserEvent::QueryEvent(QueryEvent::RouteChanged))
                    }
                    _ => (),
                }
            } else if parts.len() == 1 {
                let focus_systems = world.match_system(parts[0]).into_iter().collect();
                input_state.send_user_event(UserEvent::QueryEvent(QueryEvent::SystemsFocused(
                    focus_systems,
                )))
            }
            user_state.query_string = String::new();
            query_changed = true;
            graphics_context.request_redraw("query return");
        }

        if input_state.was_key_down(VirtualKeyCode::Back) {
            user_state.query_string.pop();
            query_changed = true;
            graphics_context.request_redraw("query back");
        }

        if input_state.was_key_down(VirtualKeyCode::Escape) {
            world.clear_route();
            input_state.send_user_event(UserEvent::QueryEvent(QueryEvent::SystemsFocused(
                HashSet::new(),
            )));
            input_state.send_user_event(UserEvent::QueryEvent(QueryEvent::RouteChanged))
        }

        if let Some(window_size) = input_state.window_resized() {
            user_state.window_size = window_size.as_f32();
            query_changed = true;
        }

        if query_changed {
            user_state.text_nodes.clear();
            if user_state.query_string.len() > 0 {
                let mut text_span =
                    font::TextSpan::new(30.0, graphics_context.ui_font, math::V4::fill(1.0));
                text_span.push(user_state.query_string.as_str());
                let text_span = graphics_context.font_cache.layout(
                    text_span,
                    font::TextAnchor::TopLeft,
                    math::v2(5.0, user_state.window_size.y - 30.0),
                    true,
                );
                user_state.text_nodes.push(text_span);
            }
        }
    }

    fn draw(frame: &mut Frame, graphics_context: &GraphicsContext, user_state: &UserState) {
        if user_state.text_nodes.len() > 0 {
            graphics_context.display.draw_text(
                frame,
                &graphics_context.font_cache,
                &user_state.text_nodes,
                graphics_context.ui_scale(),
            );
        }
    }
}

trait Widget {
    fn update(&mut self, dt: Duration, input_state: &InputState, world: &World);
    fn draw(&mut self, frame: &mut Frame);
}

fn sec_status_color(sec: f64) -> math::V3<f32> {
    let sec_status = sec.max(0.0).min(1.0) as f32;
    let blue = if sec_status >= 0.9 { 1.0 } else { 0.0 };
    let green = if sec_status >= 0.5 { 1.0 } else { sec_status };
    let red = if sec_status >= 0.6 {
        1.0 - sec_status
    } else {
        1.0
    };
    math::v3(red, green, blue)
}

fn standing_color(standing: f64) -> math::V3<f32> {
    if standing == 0.0 {
        math::v3(0.5, 0.5, 0.5)
    } else if standing > 0.5 {
        math::v3(0.0, 0.15, 1.0)
    } else if standing > 0.0 {
        math::v3(0.0, 0.5, 1.0)
    } else if standing < -0.5 {
        math::v3(1.0, 0.02, 0.0)
    } else {
        math::v3(1.0, 0.5, 0.0)
    }
}

fn jump_type_color(jump: &JumpType) -> math::V3<f32> {
    match jump {
        JumpType::System => math::v3(0.0, 0.0, 1.0),
        JumpType::Region => math::v3(0.1, 0.0, 0.15),
        JumpType::Constellation => math::v3(0.2, 0.0, 0.0),
        JumpType::JumpGate => math::v3(0.0, 0.2, 0.0),
        JumpType::Wormhole => math::v3(0.1, 0.15, 0.0),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CircleVertex {
    pub position: math::V2<f32>,
}

#[derive(Clone, Copy, Debug)]
pub struct LineVertex {
    pub position: math::V3<f32>,
    pub normal: math::V2<f32>,
    pub color: math::V3<f32>,
}

#[derive(Clone, Copy, Debug)]
pub struct SystemData {
    pub color: math::V4<f32>,
    pub highlight: math::V4<f32>,
    pub center: math::V2<f32>,
    pub system_id: i32,
    pub scale: f32,
    pub radius: f32,
}

#[derive(Debug, Copy, Clone)]
pub struct QuadVertex {
    pub position: math::V2<f32>,
    pub uv: math::V2<f32>,
}

#[derive(Clone, Copy, Debug)]
pub struct TextVertex {
    pub position: math::V2<f32>,
    pub uv: math::V2<f32>,
    pub color: math::V4<f32>,
}
