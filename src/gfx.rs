use glium::glutin;
use glium::Surface;
use winit::event::{Event, MouseButton, VirtualKeyCode};
use winit::event_loop::{ControlFlow, EventLoop, EventLoopProxy};
use winit::window::WindowBuilder;

use std::cell::Cell;
use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::font;
use crate::math;
use crate::shaders;
use crate::world::{JumpType, World};

mod images;

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
    graphics_state: GraphicsState,
    graphics_context: GraphicsContext,
}

struct UserState {
    query_string: String,
}

pub struct GraphicsContext {
    pub display: glium::Display,
    pub ui_font: font::FontId,
    pub title_font: font::FontId,
    pub symbol_font: font::FontId,
    pub font_cache: font::FontCache,
    pub images: images::Images,
    ui_scale: Cell<f32>,
}

impl GraphicsContext {
    pub fn request_redraw(&self) {
        self.display.gl_window().window().request_redraw()
    }

    pub fn set_ui_scale(&self, window_size: math::V2<f32>) {
        self.ui_scale.set(window_size.y / 2160.0)
    }

    pub fn ui_scale(&self) -> f32 {
        self.ui_scale.get()
    }
}

struct GraphicsState {
    circle_model: glium::VertexBuffer<CircleVertex>,
    system_program: glium::Program,
    jump_program: glium::Program,
    text_program: glium::Program,
    quad_program: glium::Program,
    shader_collection: shaders::ShaderCollection,
    shader_version: usize,
}

impl Window {
    pub fn new(width: u32, height: u32) -> Self {
        let event_loop = EventLoop::with_user_event();
        let w_builder = WindowBuilder::new()
            .with_inner_size(winit::dpi::LogicalSize::new(width, height))
            .with_transparent(false)
            .with_title("EVE Mapper");
        let c_builder = glutin::ContextBuilder::new()
            .with_vsync(true)
            .with_gl_profile(glutin::GlProfile::Core)
            .with_gl(glutin::GlRequest::Specific(glutin::Api::OpenGl, (4, 2)));

        let display = glium::Display::new(w_builder, c_builder, &event_loop).unwrap();

        let mut circle_verts = Vec::new();
        circle_verts.push(CircleVertex {
            position: math::v2(0.0, 0.0),
        });

        for i in 0..17 {
            let n = ((2.0 * std::f32::consts::PI) / 16.0) * i as f32;
            circle_verts.push(CircleVertex {
                position: math::v2(n.sin(), n.cos()),
            });
        }

        let circle_model = glium::VertexBuffer::new(&display, &circle_verts).unwrap();

        let shader_collection = shaders::ShaderCollection::new();
        let shader_version = shader_collection.version();

        let systems_vert = shader_collection.get("systems_vert").unwrap();
        let systems_frag = shader_collection.get("systems_frag").unwrap();

        let jumps_vert = shader_collection.get("jumps_vert").unwrap();
        let jumps_frag = shader_collection.get("jumps_frag").unwrap();

        let text_vert = shader_collection.get("text_vert").unwrap();
        let text_frag = shader_collection.get("text_frag").unwrap();

        let quad_vert = shader_collection.get("quad_vert").unwrap();
        let quad_frag = shader_collection.get("quad_frag").unwrap();

        let system_program =
            glium::Program::from_source(&display, &systems_vert, &systems_frag, None).unwrap();

        let jump_program =
            glium::Program::from_source(&display, &jumps_vert, &jumps_frag, None).unwrap();

        let text_program =
            glium::Program::from_source(&display, &text_vert, &text_frag, None).unwrap();

        let quad_program =
            glium::Program::from_source(&display, &quad_vert, &quad_frag, None).unwrap();

        let mut font_cache = font::FontCache::new(&display, 1024, 1024);
        let ui_font = font_cache.load("evesans", font::EVE_SANS_NEUE).unwrap();
        let title_font = font_cache
            .load("evesans-bold", font::EVE_SANS_NEUE_BOLD)
            .unwrap();
        let symbol_font = font_cache.load("nanumgothic", font::NANUMGOTHIC).unwrap();

        let images = images::Images::new(&display, 4096, 4096);

        let graphics_state = GraphicsState {
            circle_model,
            system_program,
            jump_program,
            text_program,
            quad_program,
            shader_collection,
            shader_version,
        };

        let graphics_context = GraphicsContext {
            display,
            ui_font,
            title_font,
            symbol_font,
            font_cache,
            images,
            ui_scale: Cell::new(1.0),
        };

        let user_state = UserState {
            query_string: String::new(),
        };

        Window {
            event_loop,
            graphics_context,
            graphics_state,
            user_state,
        }
    }

    pub fn run(self) -> ! {
        let runtime = async_std::task::Builder::new();
        let mut graphics_state = self.graphics_state;

        let event_proxy = self.event_loop.create_proxy();

        let mut world = World::new(event_proxy.clone());
        runtime.blocking(async {
            let profile = crate::oauth::load_or_authorize().await.unwrap();
            let client = crate::esi::Client::new(profile).await;
            world.load(&client).await.unwrap();
        });

        let mut user_state = self.user_state;
        let graphics_context = unsafe {
            std::mem::transmute::<&GraphicsContext, &'static GraphicsContext>(
                &self.graphics_context,
            )
        };

        let mut input_state = InputState::new(event_proxy);
        let mut map = Map::new(&graphics_context);
        let mut info_box = InfoBox::new(&graphics_context);
        let mut route_box = RouteBox::new(&graphics_context);
        let mut frame_time = Instant::now();
        let mut render_time = Instant::now();

        self.event_loop.run(move |event, _window, control_flow| {
            use winit::event::*;
            match event {
                Event::NewEvents(_) => {}
                Event::MainEventsCleared => {
                    let dt = frame_time.elapsed();

                    if let Some(window_size) = input_state.window_resized() {
                        graphics_context.set_ui_scale(window_size.as_f32());
                    }

                    Window::update(
                        dt,
                        &input_state,
                        &mut world,
                        graphics_context,
                        &mut user_state,
                    );
                    info_box.update(dt, &input_state, &world);
                    route_box.update(dt, &input_state, &world);
                    map.update(dt, &input_state, &world);

                    frame_time = Instant::now();

                    *control_flow = if input_state.closed() {
                        ControlFlow::Exit
                    } else {
                        ControlFlow::Wait
                    };

                    input_state.reset();
                }
                Event::RedrawRequested(..) => {
                    Window::update_shaders(graphics_context, &mut graphics_state);

                    let mut frame = graphics_context.display.draw();
                    frame.clear_color(0.0 / 255.0, 0.0 / 255.0, 0.0 / 255.0, 1.0);
                    frame.clear_depth(0.0);

                    map.draw(&graphics_state, &mut frame);
                    route_box.draw(&graphics_state, &mut frame);
                    info_box.draw(&graphics_state, &mut frame);
                    Window::draw(
                        &mut frame,
                        &graphics_context,
                        &mut graphics_state,
                        &mut user_state,
                        &input_state,
                    );

                    if let Err(e) = frame.finish() {
                        log::error!("gl swap buffer error: {:?}", e);
                    }

                    if render_time.elapsed().as_millis() > 1000 {
                        render_time = Instant::now();
                    }

                    //Send this event to ensure we run the updates for the next frame to continue any animations that may be ongoing
                    input_state.send_user_event(UserEvent::FrameDrawn);
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
            graphics_context.request_redraw();
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
            graphics_context.request_redraw();
        }

        if input_state.was_key_down(VirtualKeyCode::Back) {
            user_state.query_string.pop();
            graphics_context.request_redraw();
        }

        if input_state.was_key_down(VirtualKeyCode::Escape) {
            world.clear_route();
            input_state.send_user_event(UserEvent::QueryEvent(QueryEvent::SystemsFocused(
                HashSet::new(),
            )));
            input_state.send_user_event(UserEvent::QueryEvent(QueryEvent::RouteChanged))
        }
    }

    fn draw<S: Surface>(
        frame: &mut S,
        graphics_context: &GraphicsContext,
        graphics_state: &mut GraphicsState,
        user_state: &mut UserState,
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
            let uniforms = glium::uniform! {
                window_size: window_size,
                font_atlas: graphics_context.font_cache.sampler()
            };

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

            let mut text_buf = Vec::new();

            for text in pos_nodes {
                graphics_context
                    .font_cache
                    .draw(&text, &mut text_buf, window_size);
            }

            let text_data_buf =
                glium::VertexBuffer::new(&graphics_context.display, &text_buf).unwrap();
            frame
                .draw(
                    &text_data_buf,
                    &glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
                    &graphics_state.text_program,
                    &uniforms,
                    &draw_params,
                )
                .unwrap();
        }
    }

    fn update_shaders(graphics_context: &GraphicsContext, graphics_state: &mut GraphicsState) {
        let new_version = graphics_state.shader_collection.version();
        if new_version != graphics_state.shader_version {
            let systems_vert = graphics_state
                .shader_collection
                .get("systems_vert")
                .unwrap();
            let systems_frag = graphics_state
                .shader_collection
                .get("systems_frag")
                .unwrap();

            let jumps_vert = graphics_state.shader_collection.get("jumps_vert").unwrap();
            let jumps_frag = graphics_state.shader_collection.get("jumps_frag").unwrap();

            let text_vert = graphics_state.shader_collection.get("text_vert").unwrap();
            let text_frag = graphics_state.shader_collection.get("text_frag").unwrap();

            let quad_vert = graphics_state.shader_collection.get("quad_vert").unwrap();
            let quad_frag = graphics_state.shader_collection.get("quad_frag").unwrap();

            let systems_program = glium::Program::from_source(
                &graphics_context.display,
                &systems_vert,
                &systems_frag,
                None,
            );

            let jumps_program = glium::Program::from_source(
                &graphics_context.display,
                &jumps_vert,
                &jumps_frag,
                None,
            );

            let text_program = glium::Program::from_source(
                &graphics_context.display,
                &text_vert,
                &text_frag,
                None,
            );

            let quad_program = glium::Program::from_source(
                &graphics_context.display,
                &quad_vert,
                &quad_frag,
                None,
            );

            match systems_program {
                Ok(program) => graphics_state.system_program = program,
                Err(err) => log::error!("error creating systems shader: {:?}", err),
            }

            match jumps_program {
                Ok(program) => graphics_state.jump_program = program,
                Err(err) => log::error!("error creating jumps shader: {:?}", err),
            }

            match text_program {
                Ok(program) => graphics_state.text_program = program,
                Err(err) => log::error!("error creating text shader: {:?}", err),
            }

            match quad_program {
                Ok(program) => graphics_state.quad_program = program,
                Err(err) => log::error!("error creating quad shader: {:?}", err),
            }

            graphics_state.shader_version = new_version;

            log::info!("shaders re-loaded");
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
struct CircleVertex {
    position: math::V2<f32>,
}
glium::implement_vertex!(CircleVertex, position);

#[derive(Clone, Copy, Debug)]
struct LineVertex {
    position: math::V3<f32>,
    normal: math::V2<f32>,
    color: math::V3<f32>,
}
glium::implement_vertex!(LineVertex, position, normal, color);

#[derive(Clone, Copy, Debug)]
struct SystemData {
    color: math::V4<f32>,
    highlight: math::V4<f32>,
    center: math::V2<f32>,
    system_id: i32,
    scale: f32,
    radius: f32,
}
glium::implement_vertex!(SystemData, color, highlight, center, scale, radius);

#[derive(Debug, Copy, Clone)]
pub struct QuadVertex {
    pub position: math::V2<f32>,
    pub uv: math::V2<f32>,
}

glium::implement_vertex!(QuadVertex, position, uv);

unsafe impl glium::vertex::Attribute for math::V2<f32> {
    fn get_type() -> glium::vertex::AttributeType {
        glium::vertex::AttributeType::F32F32
    }
}

unsafe impl glium::vertex::Attribute for math::V3<f32> {
    fn get_type() -> glium::vertex::AttributeType {
        glium::vertex::AttributeType::F32F32F32
    }
}

unsafe impl glium::vertex::Attribute for math::V4<f32> {
    fn get_type() -> glium::vertex::AttributeType {
        glium::vertex::AttributeType::F32F32F32F32
    }
}

unsafe impl glium::vertex::Attribute for math::M3<f32> {
    fn get_type() -> glium::vertex::AttributeType {
        glium::vertex::AttributeType::F32x3x3
    }
}

unsafe impl glium::vertex::Attribute for math::M4<f32> {
    fn get_type() -> glium::vertex::AttributeType {
        glium::vertex::AttributeType::F32x4x4
    }
}

impl glium::uniforms::AsUniformValue for math::V2<f32> {
    fn as_uniform_value(&self) -> glium::uniforms::UniformValue {
        glium::uniforms::UniformValue::Vec2([self.x, self.y])
    }
}

impl glium::uniforms::AsUniformValue for math::V3<f32> {
    fn as_uniform_value(&self) -> glium::uniforms::UniformValue {
        glium::uniforms::UniformValue::Vec3([self.x, self.y, self.z])
    }
}

impl glium::uniforms::AsUniformValue for math::V4<f32> {
    fn as_uniform_value(&self) -> glium::uniforms::UniformValue {
        glium::uniforms::UniformValue::Vec4([self.x, self.y, self.z, self.w])
    }
}

impl glium::uniforms::AsUniformValue for math::M3<f32> {
    fn as_uniform_value(&self) -> glium::uniforms::UniformValue {
        glium::uniforms::UniformValue::Mat3([
            [self.c0.x, self.c0.y, self.c0.z],
            [self.c1.x, self.c1.y, self.c1.z],
            [self.c2.x, self.c2.y, self.c2.z],
        ])
    }
}

impl glium::uniforms::AsUniformValue for math::M4<f32> {
    fn as_uniform_value(&self) -> glium::uniforms::UniformValue {
        glium::uniforms::UniformValue::Mat4([
            [self.c0.x, self.c0.y, self.c0.z, self.c0.w],
            [self.c1.x, self.c1.y, self.c1.z, self.c1.w],
            [self.c2.x, self.c2.y, self.c2.z, self.c2.w],
            [self.c3.x, self.c3.y, self.c3.z, self.c3.w],
        ])
    }
}

pub struct InputState {
    event_proxy: EventLoopProxy<UserEvent>,
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
    pub fn new(event_proxy: EventLoopProxy<UserEvent>) -> InputState {
        InputState {
            event_proxy,
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
        let event_err = self.event_proxy.send_event(event);
        match event_err {
            Err(error) => log::error!("error sending user event: {:?}", error),
            _ => (),
        }
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
    fn draw<S: glium::Surface>(&mut self, graphics_state: &GraphicsState, frame: &mut S);
}
