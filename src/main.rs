use eve_mapper::Window;

fn main() {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let window = Window::new(1024, 1024);
    window.run();
}
