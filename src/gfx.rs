use winit::event::{Event, MouseButton, VirtualKeyCode};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowBuilder;

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use crate::font;
use crate::math;
use crate::platform::time::Instant;
use crate::platform::{
    create_event_proxy, spawn, EventReceiver, EventSender, Frame, GraphicsBackend,
    DEFAULT_CONTROL_FLOW,
};
use crate::world::{Galaxy, JumpType, World};

pub mod images;

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

pub struct Window {
    event_loop: EventLoop<UserEvent>,
    user_state: UserState,
    graphics_context: Rc<GraphicsContext>,
}

struct UserState {
    query_string: String,
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

        let mut input_state = InputState::new(event_sender, event_receiver);
        let mut user_state = self.user_state;

        let graphics_context = self.graphics_context.clone();
        let mut map = Map::new(graphics_context.clone());
        let mut info_box = InfoBox::new(graphics_context.clone());
        let mut route_box = RouteBox::new(graphics_context.clone());

        let mut frame_time = Instant::now();

        input_state.window_size = math::v2(
            graphics_context.window_size().x as u32,
            graphics_context.window_size().y as u32,
        );

        self.event_loop.run(move |event, _window, control_flow| {
            use winit::event::*;
            match event {
                Event::NewEvents(_) => {}
                Event::MainEventsCleared => {
                    //exists for wasm-web-sys builds where EventLoopProxys do not work and cannot send events to the main loop directly
                    for event in input_state.event_receiver.user_event_iter() {
                        match event {
                            UserEvent::DataEvent(DataEvent::GalaxyLoaded(galaxy)) => {
                                world.import(galaxy)
                            }
                            event => input_state.user_events.push(event),
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

                    map.draw(&mut frame);
                    route_box.draw(&mut frame);
                    info_box.draw(&mut frame);
                    Window::draw(&mut frame, &graphics_context, &user_state, &input_state);

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
        if input_state.text().len() > 0 {
            user_state.query_string.push_str(input_state.text());
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
                        if input_state.pressed_keys.contains(&VirtualKeyCode::LShift)
                            | input_state.pressed_keys.contains(&VirtualKeyCode::RShift)
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
            graphics_context.request_redraw("query return");
        }

        if input_state.was_key_down(VirtualKeyCode::Back) {
            user_state.query_string.pop();
            graphics_context.request_redraw("query back");
        }

        if input_state.was_key_down(VirtualKeyCode::Escape) {
            world.clear_route();
            input_state.send_user_event(UserEvent::QueryEvent(QueryEvent::SystemsFocused(
                HashSet::new(),
            )));
            input_state.send_user_event(UserEvent::QueryEvent(QueryEvent::RouteChanged))
        }
    }

    fn draw(
        frame: &mut Frame,
        graphics_context: &GraphicsContext,
        user_state: &UserState,
        input_state: &InputState,
    ) {
        let window_size = input_state.window_size.as_f32();
        let mut pos_nodes = Vec::new();

        if user_state.query_string.len() > 0 {
            let mut text_span =
                font::TextSpan::new(30.0, graphics_context.ui_font, math::V4::fill(1.0));
            text_span.push(user_state.query_string.as_str());
            let text_span = graphics_context.font_cache.layout(
                text_span,
                font::TextAnchor::TopLeft,
                math::v2(5.0, window_size.y - 30.0),
                true,
            );
            pos_nodes.push(text_span);
        }

        if pos_nodes.len() > 0 {
            graphics_context.display.draw_text(
                frame,
                &graphics_context.font_cache,
                &pos_nodes,
                graphics_context.ui_scale(),
            );
        }
    }
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

pub struct InputState {
    event_sender: EventSender,
    event_receiver: EventReceiver,
    closed: bool,
    text: String,
    pressed_keys: HashSet<winit::event::VirtualKeyCode>,
    released_keys: HashSet<winit::event::VirtualKeyCode>,
    mouse_wheel_delta: f32,
    window_size: math::V2<u32>,
    window_start_size: math::V2<u32>,
    mouse_position: math::V2<f32>,
    mouse_start_position: math::V2<f32>,
    pressed_mouse: HashSet<winit::event::MouseButton>,
    released_mouse: HashSet<winit::event::MouseButton>,
    user_events: Vec<UserEvent>,
}

impl InputState {
    pub fn new(event_sender: EventSender, event_receiver: EventReceiver) -> InputState {
        InputState {
            event_sender,
            event_receiver,
            closed: false,
            text: String::new(),
            pressed_keys: HashSet::new(),
            released_keys: HashSet::new(),
            mouse_wheel_delta: 0.0,
            window_size: math::V2::fill(1024),
            window_start_size: math::V2::fill(1024),
            mouse_position: math::V2::fill(0.0),
            mouse_start_position: math::V2::fill(0.0),
            pressed_mouse: HashSet::new(),
            released_mouse: HashSet::new(),
            user_events: Vec::new(),
        }
    }

    pub fn send_user_event(&self, event: UserEvent) {
        self.event_sender.send_user_event(event);
    }

    pub fn reset(&mut self) {
        self.mouse_start_position = self.mouse_position;
        self.mouse_wheel_delta = 0.0;
        self.window_start_size = self.window_size;
        self.released_keys.clear();
        self.released_mouse.clear();
        self.text.clear();
        self.user_events.clear();
    }

    pub fn process(&mut self, event: Event<UserEvent>) {
        use winit::event::*;
        match event {
            Event::UserEvent(user_event) => self.user_events.push(user_event),
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                self.closed = true;
            }
            Event::WindowEvent {
                event: WindowEvent::ReceivedCharacter(c),
                ..
            } => {
                if !c.is_control() {
                    self.text.push(c);
                }
            }
            Event::WindowEvent {
                event:
                    WindowEvent::KeyboardInput {
                        input:
                            KeyboardInput {
                                state,
                                virtual_keycode: Some(key),
                                ..
                            },
                        ..
                    },
                ..
            } => match state {
                ElementState::Pressed => {
                    self.released_keys.remove(&key);
                    self.pressed_keys.insert(key);
                }
                ElementState::Released => {
                    self.pressed_keys.remove(&key);
                    self.released_keys.insert(key);
                }
            },
            Event::WindowEvent {
                event: WindowEvent::MouseWheel { delta, .. },
                ..
            } => {
                let delta = match delta {
                    MouseScrollDelta::LineDelta(_x, y) => y * 5.0,
                    MouseScrollDelta::PixelDelta(pos) => pos.y as f32,
                };

                self.mouse_wheel_delta += delta;
            }
            Event::WindowEvent {
                event: WindowEvent::MouseInput { state, button, .. },
                ..
            } => match state {
                ElementState::Pressed => {
                    self.released_mouse.remove(&button);
                    self.pressed_mouse.insert(button);
                }
                ElementState::Released => {
                    self.pressed_mouse.remove(&button);
                    self.released_mouse.insert(button);
                }
            },
            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, .. },
                ..
            } => {
                let position = math::v2(position.x, position.y).as_f32();
                self.mouse_position = position;
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } => {
                self.window_size = math::v2(size.width, size.height);
            }
            _ => (),
        }
    }

    pub fn window_resized(&self) -> Option<math::V2<u32>> {
        if self.window_start_size != self.window_size {
            Some(self.window_size)
        } else {
            None
        }
    }

    pub fn scroll(&self) -> f32 {
        self.mouse_wheel_delta
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn was_key_down(&self, key: VirtualKeyCode) -> bool {
        self.released_keys.contains(&key)
    }

    pub fn user_events(&self) -> impl Iterator<Item = &UserEvent> {
        self.user_events.iter()
    }

    pub fn closed(&self) -> bool {
        self.closed
    }

    pub fn mouse_move_delta(&self) -> math::V2<f32> {
        self.mouse_start_position - self.mouse_position
    }

    pub fn is_mouse_down(&self, button: MouseButton) -> bool {
        self.pressed_mouse.contains(&button)
    }
}

trait Widget {
    fn update(&mut self, dt: Duration, input_state: &InputState, world: &World);
    fn draw(&mut self, frame: &mut Frame);
}

pub trait UserEventSender: Clone {
    fn send_user_event(&self, event: UserEvent);
}

pub trait UserEventReceiver {
    type Iter: Iterator<Item = UserEvent>;
    fn user_event_iter(&self) -> Self::Iter;
}

impl UserEventSender for std::sync::mpsc::Sender<UserEvent> {
    fn send_user_event(&self, event: UserEvent) {
        let _ = self.send(event);
    }
}

impl UserEventSender for EventLoopProxy<UserEvent> {
    fn send_user_event(&self, event: UserEvent) {
        let _ = self.send_event(event);
    }
}

impl UserEventReceiver for std::sync::mpsc::Receiver<UserEvent> {
    type Iter = std::vec::IntoIter<UserEvent>;
    fn user_event_iter(&self) -> Self::Iter {
        let items: Vec<UserEvent> = self.try_iter().collect();
        items.into_iter()
    }
}

impl UserEventReceiver for () {
    type Iter = std::iter::Empty<UserEvent>;
    fn user_event_iter(&self) -> Self::Iter {
        std::iter::empty()
    }
}
