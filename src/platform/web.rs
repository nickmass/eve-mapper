use byteorder::{LittleEndian, WriteBytesExt};
use wasm_bindgen::JsCast;
use web_sys::WebGlRenderingContext as GL;
use winit::event_loop::{ControlFlow, EventLoop};
use winit::platform::web::*;
use winit::window::WindowBuilder;

use crate::gfx::font::{FontCache, PositionedTextSpan};
use crate::gfx::images::{Image, Images};
use crate::gfx::{CircleVertex, LineVertex, QuadVertex, SystemData, TextVertex, UserEvent};
use crate::math;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub use wasm_bindgen_futures::spawn_local as spawn;
pub use wasm_timer as time;

mod gl;

const PROFILE: &[u8] = include_bytes!("../../eve-profile.json");
const STATIC: &[u8] = include_bytes!("../../eve-static.dat");
const DYNAMIC: &[u8] = include_bytes!("../../eve-dynamic.dat");
const BRIDGES: &[u8] = include_bytes!("../../bridges.tsv");

pub const ESI_IMAGE_SERVER: &'static str =
    "https://cors-anywhere.herokuapp.com/https://images.evetech.net/";
pub const USER_AGENT: Option<&'static str> = None;

pub fn file_exists<P: AsRef<std::path::Path>>(path: P) -> bool {
    match path.as_ref().file_name().and_then(|s| s.to_str()) {
        Some("eve-profile.json") => true,
        Some("eve-static.dat") => true,
        Some("eve-dynamic.dat") => true,
        Some("bridges.tsv") => true,
        _ => false,
    }
}

pub async fn read_file<P: AsRef<std::path::Path>>(path: P) -> std::io::Result<Vec<u8>> {
    match path.as_ref().file_name().and_then(|s| s.to_str()) {
        Some("eve-profile.json") => Ok(Vec::from(PROFILE)),
        Some("eve-static.dat") => Ok(Vec::from(STATIC)),
        Some("eve-dynamic.dat") => Ok(Vec::from(DYNAMIC)),
        Some("bridges.tsv") => Ok(Vec::from(BRIDGES)),
        Some(p) => {
            log::info!("loading file: {}", p);
            Ok(Vec::new())
        }
        _ => Ok(Vec::new()),
    }
}

pub async fn write_file<P: AsRef<std::path::Path>, C: AsRef<[u8]>>(
    path: P,
    contents: C,
) -> std::io::Result<()> {
    Ok(())
}

pub fn parse_http_date(s: &str) -> Option<time::SystemTime> {
    None
}

pub type EventSender = std::sync::mpsc::Sender<UserEvent>;
pub type EventReceiver = std::sync::mpsc::Receiver<UserEvent>;

pub fn create_event_proxy(_event_loop: &EventLoop<UserEvent>) -> (EventSender, EventReceiver) {
    let (tx, rx) = std::sync::mpsc::channel();
    (tx, rx)
}

pub const DEFAULT_CONTROL_FLOW: ControlFlow = ControlFlow::Poll;

const SYSTEMS_VERT: &'static str = include_str!("../../shaders/systems_vert_web.glsl");
const SYSTEMS_FRAG: &'static str = include_str!("../../shaders/systems_frag_web.glsl");

const JUMPS_VERT: &'static str = include_str!("../../shaders/jumps_vert_web.glsl");
const JUMPS_FRAG: &'static str = include_str!("../../shaders/jumps_frag_web.glsl");

const QUAD_VERT: &'static str = include_str!("../../shaders/quad_vert_web.glsl");
const QUAD_FRAG: &'static str = include_str!("../../shaders/quad_frag_web.glsl");

const TEXT_VERT: &'static str = include_str!("../../shaders/text_vert_web.glsl");
const TEXT_FRAG: &'static str = include_str!("../../shaders/text_frag_web.glsl");

pub struct GraphicsBackend {
    canvas: web_sys::HtmlCanvasElement,
    window: winit::window::Window,
    context: Rc<gl::GlContext>,
    window_size: Cell<math::V2<f32>>,
    system_program: RefCell<gl::GlProgram>,
    jumps_program: RefCell<gl::GlProgram>,
    quad_program: RefCell<gl::GlProgram>,
    text_program: RefCell<gl::GlProgram>,
}

impl GraphicsBackend {
    pub fn new(
        window_builder: WindowBuilder,
        event_loop: &EventLoop<UserEvent>,
        width: u32,
        height: u32,
    ) -> GraphicsBackend {
        let document = web_sys::window().unwrap().document().unwrap();
        let canvas: web_sys::HtmlCanvasElement = document
            .create_element("canvas")
            .unwrap()
            .dyn_into()
            .unwrap();
        document.body().unwrap().append_with_node_1(&canvas);

        let html_node = document.document_element().unwrap();
        let width = html_node.client_width() as u32;
        let height = html_node.client_height() as u32;

        let monitor = event_loop.primary_monitor();

        let window = window_builder
            .with_canvas(Some(canvas.clone()))
            .with_inner_size(winit::dpi::LogicalSize::new(width, height))
            .build(event_loop)
            .unwrap();

        let window_size = { math::v2(canvas.width(), canvas.height()).as_f32() };
        let context = Rc::new(gl::GlContext::new(canvas.clone()));

        let system_program = RefCell::new(gl::GlProgram::new(
            context.clone(),
            SYSTEMS_VERT,
            SYSTEMS_FRAG,
        ));
        let jumps_program =
            RefCell::new(gl::GlProgram::new(context.clone(), JUMPS_VERT, JUMPS_FRAG));
        let quad_program = RefCell::new(gl::GlProgram::new(context.clone(), QUAD_VERT, QUAD_FRAG));
        let text_program = RefCell::new(gl::GlProgram::new(context.clone(), TEXT_VERT, TEXT_FRAG));

        context.enable(GL::BLEND);
        context.blend_equation_separate(GL::FUNC_ADD, GL::FUNC_ADD);
        context.blend_func_separate(GL::SRC_ALPHA, GL::ONE_MINUS_SRC_ALPHA, GL::ZERO, GL::ONE);
        context.blend_color(1.0, 1.0, 1.0, 1.0);

        context.depth_func(GL::GEQUAL);
        context.depth_mask(true);

        GraphicsBackend {
            canvas,
            window,
            context,
            window_size: Cell::new(window_size),
            system_program,
            jumps_program,
            quad_program,
            text_program,
        }
    }

    fn depth_test(&self, enable: bool) {
        if enable {
            self.context.enable(GL::DEPTH_TEST);
        } else {
            self.context.disable(GL::DEPTH_TEST);
        }
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub fn create_texture<T: Texture>(&self, width: u32, height: u32) -> T {
        T::create(self.context.clone(), width, height)
    }

    pub fn fill_buffer<T: gl::AsGlVertex + Clone>(&self, buffer: &[T]) -> Buffer<T> {
        let model = gl::GlModel::new(self.context.clone(), Vec::from(buffer));
        Buffer {
            marker: Default::default(),
            data: Vec::from(buffer),
            model,
        }
    }

    pub fn update_texture<T: Texture>(&self, texture: &T, region: math::Rect<u32>, data: &[u8]) {
        texture.update(region, data);
    }

    pub fn update_window_size(&self, _window_size: math::V2<f32>) {
        let window_size = math::v2(self.canvas.width(), self.canvas.height());
        self.window_size.set(window_size.as_f32());
        log::info!("resized {} {}", window_size.x, window_size.y);
    }

    pub fn window_size(&self) -> math::V2<f32> {
        self.window_size.get()
    }

    pub fn begin(&self) -> Frame {
        Frame {
            context: self.context.clone(),
        }
    }

    pub fn end(&self, frame: Frame) {
        self.context.finish();
    }

    pub fn draw_system(
        &self,
        frame: &mut Frame,
        circle_buffer: &Buffer<CircleVertex>,
        system_data: &Buffer<SystemData>,
        zoom: f32,
        scale_matrix: math::M3<f32>,
        view_matrix: math::M3<f32>,
    ) {
        self.depth_test(false);
        let mut uniforms = gl::GlUniformCollection::new();
        uniforms
            .add("u_map_scale_matrix", &scale_matrix)
            .add("u_map_view_matrix", &view_matrix)
            .add("u_zoom", &zoom);

        self.system_program.borrow_mut().draw_instanced(
            &circle_buffer.model,
            system_data.data.clone(),
            &uniforms,
        );
    }

    pub fn draw_jump(
        &self,
        frame: &mut Frame,
        jump_buffer: &Buffer<LineVertex>,
        zoom: f32,
        scale_matrix: math::M3<f32>,
        view_matrix: math::M3<f32>,
    ) {
        self.depth_test(true);
        let mut uniforms = gl::GlUniformCollection::new();
        uniforms
            .add("u_map_scale_matrix", &scale_matrix)
            .add("u_map_view_matrix", &view_matrix)
            .add("u_zoom", &zoom);

        self.jumps_program
            .borrow_mut()
            .draw(&jump_buffer.model, &uniforms, None);
    }

    pub fn draw_text(
        &self,
        frame: &mut Frame,
        font_cache: &FontCache,
        text: &[PositionedTextSpan],
        ui_scale: f32,
    ) {
        self.depth_test(false);
        let mut uniforms = gl::GlUniformCollection::new();
        let window_size = self.window_size.get();
        uniforms
            .add("u_window_size", &window_size)
            .add("u_font_atlas", &font_cache.texture().texture);

        let mut text_buf = Vec::new();

        for text in text {
            font_cache.draw(self, text, &mut text_buf, self.window_size.get(), ui_scale);
        }

        let text_model = gl::GlModel::new(self.context.clone(), text_buf);

        self.text_program
            .borrow_mut()
            .draw(&text_model, &uniforms, None);
    }

    pub fn draw_image(
        &self,
        frame: &mut Frame,
        images: &Images,
        image: Image,
        position: math::Rect<f32>,
    ) {
        self.depth_test(false);
        let mut uniforms = gl::GlUniformCollection::new();
        let window_size = self.window_size.get();
        let color = math::V4::fill(1.0);
        uniforms
            .add("u_window_size", &window_size)
            .add("u_texture_atlas", &images.texture().texture)
            .add("u_textured", &true)
            .add("u_color", &color);

        let mut image_buf = Vec::new();
        images.draw(&mut image_buf, image, position);

        let image_model = gl::GlModel::new(self.context.clone(), image_buf);

        self.quad_program
            .borrow_mut()
            .draw(&image_model, &uniforms, None);
    }

    pub fn draw_quad(
        &self,
        frame: &mut Frame,
        images: &Images,
        color: math::V4<f32>,
        position: math::Rect<f32>,
    ) {
        self.depth_test(false);
        let mut uniforms = gl::GlUniformCollection::new();
        let window_size = self.window_size.get();
        uniforms
            .add("u_window_size", &window_size)
            .add("u_texture_atlas", &images.texture().texture)
            .add("u_textured", &false)
            .add("u_color", &color);

        let mut rect_buf = Vec::new();
        for v in position.triangle_list_iter() {
            rect_buf.push(QuadVertex {
                position: v,
                uv: math::v2(0.0, 0.0),
            })
        }

        let quad_model = gl::GlModel::new(self.context.clone(), rect_buf);

        self.quad_program
            .borrow_mut()
            .draw(&quad_model, &uniforms, None);
    }
}

pub struct Frame {
    context: Rc<gl::GlContext>,
}

impl Frame {
    pub fn clear_color(&mut self, color: math::V4<f32>) {
        self.context.clear_color(color.x, color.y, color.z, color.w);
        self.context.clear(GL::COLOR_BUFFER_BIT);
    }

    pub fn clear_depth(&mut self, value: f32) {
        self.context.clear_depth(value);
        self.context.clear(GL::DEPTH_BUFFER_BIT);
    }
}

pub trait Texture {
    fn create(context: Rc<gl::GlContext>, width: u32, height: u32) -> Self;
    fn update(&self, region: math::Rect<u32>, data: &[u8]);
}

pub struct RgbTexture<T: TextureFormat> {
    marker: std::marker::PhantomData<T>,
    texture: gl::GlTexture,
}

impl<T: TextureFormat> Texture for RgbTexture<T> {
    fn create(context: Rc<gl::GlContext>, width: u32, height: u32) -> Self {
        let format = match T::PIXEL_FORMAT {
            PixelFormat::Alpha => gl::PixelFormat::Alpha,
            PixelFormat::Rgb => gl::PixelFormat::RGB,
            PixelFormat::Rgba => gl::PixelFormat::RGBA,
        };
        let texture = gl::GlTexture::new(context, width, height, format);
        RgbTexture {
            texture,
            marker: Default::default(),
        }
    }

    fn update(&self, region: math::Rect<u32>, data: &[u8]) {
        let format = match T::PIXEL_FORMAT {
            PixelFormat::Alpha => gl::PixelFormat::Alpha,
            PixelFormat::Rgb => gl::PixelFormat::RGB,
            PixelFormat::Rgba => gl::PixelFormat::RGBA,
        };
        self.texture.sub_image(
            region.min.x,
            region.min.y,
            region.width(),
            region.height(),
            format,
            data,
        )
    }
}

pub struct SrgbTexture<T: TextureFormat> {
    marker: std::marker::PhantomData<T>,
    texture: gl::GlTexture,
}

impl<T: TextureFormat> Texture for SrgbTexture<T> {
    fn create(context: Rc<gl::GlContext>, width: u32, height: u32) -> Self {
        let format = match T::PIXEL_FORMAT {
            PixelFormat::Alpha => gl::PixelFormat::Alpha,
            PixelFormat::Rgb => gl::PixelFormat::RGB,
            PixelFormat::Rgba => gl::PixelFormat::RGBA,
        };
        let texture = gl::GlTexture::new(context, width, height, format);
        SrgbTexture {
            texture,
            marker: Default::default(),
        }
    }

    fn update(&self, region: math::Rect<u32>, data: &[u8]) {
        let format = match T::PIXEL_FORMAT {
            PixelFormat::Alpha => gl::PixelFormat::Alpha,
            PixelFormat::Rgb => gl::PixelFormat::SRGB,
            PixelFormat::Rgba => gl::PixelFormat::SRGBA,
        };
        self.texture.sub_image(
            region.min.x,
            region.min.y,
            region.width(),
            region.height(),
            format,
            data,
        )
    }
}

pub struct U8;

impl TextureFormat for U8 {
    const PIXEL_FORMAT: PixelFormat = PixelFormat::Alpha;
}

pub struct U8U8U8U8;

impl TextureFormat for U8U8U8U8 {
    const PIXEL_FORMAT: PixelFormat = PixelFormat::Rgba;
}

enum PixelFormat {
    Alpha,
    Rgb,
    Rgba,
}

pub trait TextureFormat {
    const PIXEL_FORMAT: PixelFormat;
}

pub struct Buffer<T: gl::AsGlVertex> {
    marker: std::marker::PhantomData<T>,
    data: Vec<T>,
    model: gl::GlModel<T>,
}

impl gl::AsGlVertex for CircleVertex {
    const ATTRIBUTES: &'static [(&'static str, gl::GlValueType)] =
        &[("a_position", gl::GlValueType::Vec2)];
    const POLY_TYPE: u32 = GL::TRIANGLE_FAN;
    const SIZE: usize = 8;

    fn write(&self, mut buf: impl std::io::Write) {
        let _ = buf.write_f32::<LittleEndian>(self.position.x);
        let _ = buf.write_f32::<LittleEndian>(self.position.y);
    }
}

impl gl::AsGlVertex for SystemData {
    const ATTRIBUTES: &'static [(&'static str, gl::GlValueType)] = &[
        ("a_color", gl::GlValueType::Vec4),
        ("a_highlight", gl::GlValueType::Vec4),
        ("a_center", gl::GlValueType::Vec2),
        ("a_scale", gl::GlValueType::Float),
        ("a_radius", gl::GlValueType::Float),
    ];
    const POLY_TYPE: u32 = GL::TRIANGLE_FAN;
    const SIZE: usize = 48;

    fn write(&self, mut buf: impl std::io::Write) {
        let _ = buf.write_f32::<LittleEndian>(self.color.x);
        let _ = buf.write_f32::<LittleEndian>(self.color.y);
        let _ = buf.write_f32::<LittleEndian>(self.color.z);
        let _ = buf.write_f32::<LittleEndian>(self.color.w);

        let _ = buf.write_f32::<LittleEndian>(self.highlight.x);
        let _ = buf.write_f32::<LittleEndian>(self.highlight.y);
        let _ = buf.write_f32::<LittleEndian>(self.highlight.z);
        let _ = buf.write_f32::<LittleEndian>(self.highlight.w);

        let _ = buf.write_f32::<LittleEndian>(self.center.x);
        let _ = buf.write_f32::<LittleEndian>(self.center.y);

        let _ = buf.write_f32::<LittleEndian>(self.scale);
        let _ = buf.write_f32::<LittleEndian>(self.radius);
    }
}

impl gl::AsGlVertex for LineVertex {
    const ATTRIBUTES: &'static [(&'static str, gl::GlValueType)] = &[
        ("a_position", gl::GlValueType::Vec3),
        ("a_normal", gl::GlValueType::Vec2),
        ("a_color", gl::GlValueType::Vec3),
    ];
    const POLY_TYPE: u32 = GL::TRIANGLES;
    const SIZE: usize = 32;

    fn write(&self, mut buf: impl std::io::Write) {
        let _ = buf.write_f32::<LittleEndian>(self.position.x);
        let _ = buf.write_f32::<LittleEndian>(self.position.y);
        let _ = buf.write_f32::<LittleEndian>(self.position.z);

        let _ = buf.write_f32::<LittleEndian>(self.normal.x);
        let _ = buf.write_f32::<LittleEndian>(self.normal.y);

        let _ = buf.write_f32::<LittleEndian>(self.color.x);
        let _ = buf.write_f32::<LittleEndian>(self.color.y);
        let _ = buf.write_f32::<LittleEndian>(self.color.z);
    }
}

impl gl::AsGlVertex for QuadVertex {
    const ATTRIBUTES: &'static [(&'static str, gl::GlValueType)] = &[
        ("a_position", gl::GlValueType::Vec2),
        ("a_uv", gl::GlValueType::Vec2),
    ];
    const POLY_TYPE: u32 = GL::TRIANGLES;
    const SIZE: usize = 16;

    fn write(&self, mut buf: impl std::io::Write) {
        let _ = buf.write_f32::<LittleEndian>(self.position.x);
        let _ = buf.write_f32::<LittleEndian>(self.position.y);

        let _ = buf.write_f32::<LittleEndian>(self.uv.x);
        let _ = buf.write_f32::<LittleEndian>(self.uv.y);
    }
}

impl gl::AsGlVertex for TextVertex {
    const ATTRIBUTES: &'static [(&'static str, gl::GlValueType)] = &[
        ("a_position", gl::GlValueType::Vec2),
        ("a_uv", gl::GlValueType::Vec2),
        ("a_color", gl::GlValueType::Vec4),
    ];
    const POLY_TYPE: u32 = GL::TRIANGLES;
    const SIZE: usize = 32;

    fn write(&self, mut buf: impl std::io::Write) {
        let _ = buf.write_f32::<LittleEndian>(self.position.x);
        let _ = buf.write_f32::<LittleEndian>(self.position.y);

        let _ = buf.write_f32::<LittleEndian>(self.uv.x);
        let _ = buf.write_f32::<LittleEndian>(self.uv.y);

        let _ = buf.write_f32::<LittleEndian>(self.color.x);
        let _ = buf.write_f32::<LittleEndian>(self.color.y);
        let _ = buf.write_f32::<LittleEndian>(self.color.z);
        let _ = buf.write_f32::<LittleEndian>(self.color.w);
    }
}
