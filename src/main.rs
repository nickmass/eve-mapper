mod cache;
pub(crate) mod error;
mod esi;
mod font;
mod gfx;
pub(crate) mod math;
mod oauth;
pub(crate) mod shaders;
mod world;

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let window = gfx::Window::new(1024, 1024);
    window.run();
}
