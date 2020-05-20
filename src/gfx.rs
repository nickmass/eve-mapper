use glium::glutin;
use glium::Surface;
use glutin::event_loop::{ControlFlow, EventLoop};
use glutin::window::WindowBuilder;

use crate::font;
use crate::math;
use crate::shaders;

type UserEvent = ();

#[derive(Copy, Clone, Debug)]
enum EventResult {
    Redraw,
    Close,
    Nothing,
}

pub struct Window {
    event_loop: EventLoop<UserEvent>,
    user_state: UserState,
    graphics_state: GraphicsState,
}

struct UserState {
    mouse_down: Option<math::V2<f32>>,
    mouse_position: math::V2<f32>,
    map_offset: math::V2<f32>,
    window_size: math::V2<u32>,
    closed: bool,
    zoom: f32,
}

struct TextNode {
    font: font::FontId,
    scale: f32,
    position: math::V2<f32>,
    text: String,
    color: math::V3<f32>,
    alpha: f32,
}

struct GraphicsState {
    display: glium::Display,
    circle_model: glium::VertexBuffer<CircleVertex>,
    system_program: glium::Program,
    jump_program: glium::Program,
    text_program: glium::Program,
    shader_collection: shaders::ShaderCollection,
    shader_version: usize,
    ui_font: font::FontId,
    font_cache: font::FontCache,
    text_nodes: Vec<TextNode>,
}

impl Window {
    pub fn new(width: u32, height: u32) -> Self {
        let event_loop = EventLoop::new();
        let w_builder = WindowBuilder::new()
            .with_inner_size(glutin::dpi::LogicalSize::new(width, height))
            .with_transparent(false);
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

        let system_program =
            glium::Program::from_source(&display, &systems_vert, &systems_frag, None).unwrap();

        let jump_program =
            glium::Program::from_source(&display, &jumps_vert, &jumps_frag, None).unwrap();

        let text_program =
            glium::Program::from_source(&display, &text_vert, &text_frag, None).unwrap();

        let mut font_cache = font::FontCache::new(&display, 1024, 1024);
        let ui_font = font_cache.load("evesans", font::EVE_SANS_NEUE).unwrap();

        let graphics_state = GraphicsState {
            display,
            circle_model,
            system_program,
            jump_program,
            text_program,
            shader_collection,
            shader_version,
            font_cache,
            ui_font,
            text_nodes: Vec::new(),
        };

        let user_state = UserState {
            window_size: math::v2(width, height),
            mouse_down: None,
            mouse_position: math::v2(0.0, 0.0),
            map_offset: math::v2(0.0, 0.0),
            zoom: 1.0,
            closed: false,
        };

        Window {
            event_loop,
            graphics_state,
            user_state,
        }
    }

    pub fn run(self, systems: crate::SystemCollection, jumps: Vec<crate::DrawJump>) -> ! {
        let mut user_state = self.user_state;
        let mut graphics_state = self.graphics_state;
        self.event_loop.run(move |event, _window, control_flow| {
            *control_flow = ControlFlow::Wait;
            use glutin::event::*;
            match event {
                Event::RedrawRequested(_window) => {
                    Window::draw(&mut graphics_state, &user_state, &systems, &jumps);
                }
                event => {
                    let event_result = Window::event(event, &mut user_state);
                    match event_result {
                        EventResult::Redraw => {
                            graphics_state.display.gl_window().window().request_redraw()
                        }
                        EventResult::Close => *control_flow = ControlFlow::Exit,
                        EventResult::Nothing => (),
                    }
                }
            }
        });
    }

    fn event(event: glutin::event::Event<UserEvent>, user_state: &mut UserState) -> EventResult {
        use glutin::event::*;
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                user_state.closed = true;
                EventResult::Close
            }
            Event::WindowEvent {
                event: WindowEvent::MouseWheel { delta, .. },
                ..
            } => {
                let delta = match delta {
                    MouseScrollDelta::LineDelta(_x, y) => y * 5.0,
                    MouseScrollDelta::PixelDelta(pos) => pos.y as f32,
                };

                if delta != 0.0 {
                    user_state.zoom += (-delta * user_state.zoom) / 20.0;

                    if user_state.zoom <= 0.25 {
                        user_state.zoom = 0.25;
                    }
                    EventResult::Redraw
                } else {
                    EventResult::Nothing
                }
            }
            Event::WindowEvent {
                event:
                    WindowEvent::MouseInput {
                        state,
                        button: MouseButton::Left,
                        ..
                    },
                ..
            } => {
                match state {
                    ElementState::Pressed => {
                        user_state.mouse_down = Some(user_state.mouse_position)
                    }
                    ElementState::Released => {
                        if let Some(offset) = user_state.mouse_down {
                            user_state.map_offset = ((offset - user_state.mouse_position)
                                / user_state.zoom)
                                + user_state.map_offset;
                        }
                        user_state.mouse_down = None;
                    }
                }
                EventResult::Redraw
            }
            Event::WindowEvent {
                event: WindowEvent::CursorMoved { position, .. },
                ..
            } => {
                let position = math::v2(position.x, position.y).as_f32();
                let window_size = user_state.window_size.as_f32();

                let mouse_position = position / window_size * 2.0 - 1.0;

                user_state.mouse_position = mouse_position;
                EventResult::Redraw
            }
            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } => {
                user_state.window_size = math::v2(size.width, size.height);
                EventResult::Redraw
            }
            _ => EventResult::Nothing,
        }
    }

    fn draw(
        graphics_state: &mut GraphicsState,
        user_state: &UserState,
        systems: &crate::SystemCollection,
        jumps: &[crate::DrawJump],
    ) {
        Window::update_shaders(graphics_state);

        let mut frame = graphics_state.display.draw();
        frame.clear_color(1.0 / 255.0, 1.5 / 255.0, 2.0 / 255.0, 1.0);

        let mut max_mag = 0.0;
        let system_data: Vec<_> = systems
            .iter()
            .filter(|s| s.system_id < 30050000)
            .map(|s| {
                let mut pos: math::V3<f64> = (&s.position).into();
                let t = pos.y;
                pos.y = pos.z;
                pos.z = t;

                let mag = pos.magnitude() as f32;
                if mag > max_mag {
                    max_mag = mag;
                }
                let color = sec_status_color(s.security_status);

                SystemData {
                    center: pos.contract().as_f32(),
                    color,
                }
            })
            .collect();

        let mut jump_data = Vec::with_capacity(jumps.len() * 6);
        for j in jumps {
            push_line_segment(j, &mut jump_data);
        }

        let zoom = user_state.zoom;

        let map_offset = user_state.map_offset;
        let offset = if let Some(offset) = user_state.mouse_down {
            ((offset - user_state.mouse_position) / zoom) + map_offset
        } else {
            map_offset
        };

        let mut map_view_matrix = math::M3::<f32>::identity();
        map_view_matrix.c0.x = zoom / max_mag;
        map_view_matrix.c1.y = zoom / max_mag;
        map_view_matrix.c2.x = -offset.x * zoom;
        map_view_matrix.c2.y = offset.y * zoom;

        let window_size = user_state.window_size.as_f32();

        let window_scale = if window_size.x > window_size.y {
            math::v2(window_size.x / window_size.y, 1.0)
        } else if window_size.y > window_size.x {
            math::v2(1.0, window_size.y / window_size.x)
        } else {
            math::v2(1.0, 1.0)
        };

        let mut map_scale_matrix = math::M3::<f32>::identity();
        map_scale_matrix.c0.x = 1.0 / window_scale.x;
        map_scale_matrix.c1.y = 1.0 / window_scale.y;

        let font_atlas_sampler = graphics_state
            .font_cache
            .texture()
            .sampled()
            .magnify_filter(glium::uniforms::MagnifySamplerFilter::Nearest)
            .minify_filter(glium::uniforms::MinifySamplerFilter::Nearest);

        let uniforms = glium::uniform! {
            map_scale_matrix: map_scale_matrix,
            map_view_matrix: map_view_matrix,
            zoom: zoom,
            window_size: window_size,
            font_atlas: font_atlas_sampler
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

        let system_data_buf =
            glium::VertexBuffer::new(&graphics_state.display, &system_data).unwrap();

        let jump_data_buf = glium::VertexBuffer::new(&graphics_state.display, &jump_data).unwrap();

        frame
            .draw(
                &jump_data_buf,
                &glium::index::NoIndices(glium::index::PrimitiveType::TrianglesList),
                &graphics_state.jump_program,
                &uniforms,
                &draw_params,
            )
            .unwrap();

        frame
            .draw(
                (
                    &graphics_state.circle_model,
                    system_data_buf.per_instance().unwrap(),
                ),
                &glium::index::NoIndices(glium::index::PrimitiveType::TriangleFan),
                &graphics_state.system_program,
                &uniforms,
                &draw_params,
            )
            .unwrap();

        if zoom > 15.0 {
            let alpha = ((zoom - 15.0) / (25.0 - 15.0)).min(1.0);

            let mut text_view_matrix = math::M3::<f32>::identity();
            text_view_matrix.c0.x = zoom / max_mag;
            text_view_matrix.c1.y = zoom / max_mag;
            text_view_matrix.c2.x = -offset.x * zoom;
            text_view_matrix.c2.y = offset.y * zoom;

            let mut text_scale_matrix = math::M3::<f32>::identity();
            text_scale_matrix.c0.x = 1.0 / window_scale.x;
            text_scale_matrix.c1.y = 1.0 / window_scale.y;

            let mut text_screen_matrix = math::M3::<f32>::identity();
            text_screen_matrix.c0.x = window_size.x / 2.0;
            text_screen_matrix.c1.y = -window_size.y / 2.0;
            text_screen_matrix.c2.x = window_size.x / 2.0;
            text_screen_matrix.c2.y = window_size.y / 2.0;

            let text_transform = text_screen_matrix * text_scale_matrix * text_view_matrix;

            for system in systems.iter().filter(|s| s.system_id < 30050000) {
                let pos: math::V3<f64> = (&system.position).into();
                let pos = math::v2(pos.x, pos.z).as_f32();

                let pos = (text_transform * pos.expand(1.0)).collapse();

                let min_corner = pos - 50.0;
                let max_corner = pos + 50.0;

                if max_corner.x < 0.0
                    || max_corner.y < 0.0
                    || min_corner.x > window_size.x
                    || min_corner.y > window_size.y
                {
                    continue;
                }

                let color = if system.name == "Jita" {
                    sec_status_color(system.security_status)
                } else {
                    math::V3::fill(0.6)
                };

                let pos = pos + math::V2::fill(0.5 * zoom);

                let node = TextNode {
                    text: system.name.to_string(),
                    font: graphics_state.ui_font,
                    scale: 25.0,
                    position: pos,
                    alpha,
                    color,
                };

                graphics_state.text_nodes.push(node);
            }
        }

        let node = TextNode {
            text: "Hello World".to_string(),
            font: graphics_state.ui_font,
            scale: 130.0,
            position: math::v2(480.0, 20.0),
            alpha: 1.0,
            color: math::V3::fill(1.0),
        };

        graphics_state.text_nodes.push(node);

        if graphics_state.text_nodes.len() > 0 {
            let mut text_buf = Vec::new();
            for text in graphics_state.text_nodes.drain(..) {
                graphics_state
                    .font_cache
                    .prepare(
                        text.font,
                        text.text.as_str(),
                        &mut text_buf,
                        text.scale,
                        text.position,
                        text.color,
                        text.alpha,
                        window_size,
                    )
                    .unwrap();
            }

            let text_data_buf =
                glium::VertexBuffer::new(&graphics_state.display, &text_buf).unwrap();
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

        frame.finish().unwrap();
    }

    fn update_shaders(graphics_state: &mut GraphicsState) {
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

            let systems_program = glium::Program::from_source(
                &graphics_state.display,
                &systems_vert,
                &systems_frag,
                None,
            );

            let jumps_program = glium::Program::from_source(
                &graphics_state.display,
                &jumps_vert,
                &jumps_frag,
                None,
            );

            let text_program =
                glium::Program::from_source(&graphics_state.display, &text_vert, &text_frag, None);

            match systems_program {
                Ok(program) => graphics_state.system_program = program,
                Err(err) => eprintln!("{:?}", err),
            }

            match jumps_program {
                Ok(program) => graphics_state.jump_program = program,
                Err(err) => eprintln!("{:?}", err),
            }

            match text_program {
                Ok(program) => graphics_state.text_program = program,
                Err(err) => eprintln!("{:?}", err),
            }

            graphics_state.shader_version = new_version;

            println!("Shaders re-loaded");
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CircleVertex {
    position: math::V2<f32>,
}
glium::implement_vertex!(CircleVertex, position);

#[derive(Clone, Copy, Debug)]
struct LineVertex {
    position: math::V2<f32>,
    normal: math::V2<f32>,
    color: math::V3<f32>,
}
glium::implement_vertex!(LineVertex, position, normal, color);

#[derive(Clone, Copy, Debug)]
struct SystemData {
    color: math::V3<f32>,
    center: math::V2<f32>,
}
glium::implement_vertex!(SystemData, color, center);

fn sec_status_color(sec: f64) -> math::V3<f32> {
    let sec_status = sec.max(0.0).min(1.0) as f32;
    let blue = if sec_status >= 0.9 { 1.0 } else { 0.0 };
    math::v3(1.0 - sec_status, sec_status, blue)
}

fn jump_type_color(jump: &super::JumpType) -> math::V3<f32> {
    use super::JumpType;
    match jump {
        JumpType::System => math::v3(0.0, 0.0, 1.0),
        JumpType::Constellation => math::v3(0.4, 0.0, 0.65),
        JumpType::Region => math::v3(0.4, 0.0, 0.65),
    }
}

fn push_line_segment(jump: &super::DrawJump, buffer: &mut Vec<LineVertex>) {
    let (left_color, right_color) =
        match (jump.jump_type, jump.on_route, jump.left_sec, jump.right_sec) {
            (_, true, left, right) => (sec_status_color(left), sec_status_color(right)),
            (jump, false, _, _) => (jump_type_color(&jump), jump_type_color(&jump)),
        };

    let jump_left = math::v2(jump.left.x, jump.left.z).as_f32();
    let jump_right = math::v2(jump.right.x, jump.right.z).as_f32();

    let left_norm = math::v2(-(jump_left.y - jump_right.y), jump_left.x - jump_right.x).normalize();
    let right_norm =
        math::v2(jump_left.y - jump_right.y, -(jump_left.x - jump_right.x)).normalize();

    buffer.push(LineVertex {
        position: jump_left,
        color: left_color,
        normal: left_norm,
    });

    buffer.push(LineVertex {
        position: jump_right,
        color: right_color,
        normal: right_norm,
    });

    buffer.push(LineVertex {
        position: jump_left,
        color: left_color,
        normal: right_norm,
    });

    buffer.push(LineVertex {
        position: jump_right,
        color: right_color,
        normal: right_norm,
    });

    buffer.push(LineVertex {
        position: jump_right,
        color: right_color,
        normal: left_norm,
    });

    buffer.push(LineVertex {
        position: jump_left,
        color: left_color,
        normal: left_norm,
    });
}

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

unsafe impl glium::vertex::Attribute for math::M3<f32> {
    fn get_type() -> glium::vertex::AttributeType {
        glium::vertex::AttributeType::F32x3x3
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

impl glium::uniforms::AsUniformValue for math::M3<f32> {
    fn as_uniform_value(&self) -> glium::uniforms::UniformValue {
        glium::uniforms::UniformValue::Mat3([
            [self.c0.x, self.c0.y, self.c0.z],
            [self.c1.x, self.c1.y, self.c1.z],
            [self.c2.x, self.c2.y, self.c2.z],
        ])
    }
}
