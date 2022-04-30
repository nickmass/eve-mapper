use glium::program::ProgramCreationInput;
use notify::Watcher;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;

use std::path::{Path, PathBuf};

macro_rules! shader_program(
    ($name:ident, $vert:literal, $frag:literal) => {
        #[derive(Debug)]
        pub struct $name;

        impl ShaderProgram for $name {
            const VERTEX_RELATIVE_PATH: &'static str = $vert;
            const FRAGMENT_RELATIVE_PATH: &'static str = $frag;
            const VERTEX_SOURCE: &'static str = include_str!($vert);
            const FRAGMENT_SOURCE: &'static str = include_str!($frag);
        }

    }
);

shader_program!(
    SystemsShader,
    "../../../shaders/systems_vert.glsl",
    "../../../shaders/systems_frag.glsl"
);

shader_program!(
    JumpsShader,
    "../../../shaders/jumps_vert.glsl",
    "../../../shaders/jumps_frag.glsl"
);

shader_program!(
    TextShader,
    "../../../shaders/text_vert.glsl",
    "../../../shaders/text_frag.glsl"
);

shader_program!(
    QuadShader,
    "../../../shaders/quad_vert.glsl",
    "../../../shaders/quad_frag.glsl"
);

pub trait ShaderProgram {
    const VERTEX_RELATIVE_PATH: &'static str;
    const FRAGMENT_RELATIVE_PATH: &'static str;
    const VERTEX_SOURCE: &'static str;
    const FRAGMENT_SOURCE: &'static str;

    fn vertex_source() -> &'static str {
        Self::VERTEX_SOURCE
    }

    fn fragment_source() -> &'static str {
        Self::FRAGMENT_SOURCE
    }

    fn vertex_path<P: AsRef<Path>>(shader_dir: P) -> PathBuf {
        let path = PathBuf::from(Self::VERTEX_RELATIVE_PATH);
        shader_dir.as_ref().join(path.file_name().unwrap())
    }

    fn fragment_path<P: AsRef<Path>>(shader_dir: P) -> PathBuf {
        let path = PathBuf::from(Self::FRAGMENT_RELATIVE_PATH);
        shader_dir.as_ref().join(path.file_name().unwrap())
    }
}

pub struct ShaderCollection {
    pub version: Arc<AtomicUsize>,
    pub closed: Arc<AtomicBool>,
    watcher: notify::RecommendedWatcher,
    update_thread: Option<std::thread::JoinHandle<()>>,
    shader_dir: PathBuf,
}

impl ShaderCollection {
    pub fn new<P: AsRef<Path>>(shader_dir: P) -> ShaderCollection {
        let (tx, rx) = channel();
        let watcher = notify::watcher(tx, std::time::Duration::from_millis(100)).unwrap();

        let closed = Arc::new(AtomicBool::new(false));
        let version = Arc::new(AtomicUsize::new(0));

        let update_thread = Some(std::thread::spawn({
            let closed = closed.clone();
            let version = version.clone();
            move || {
                while !closed.load(Ordering::Relaxed) {
                    use notify::DebouncedEvent;
                    match rx.try_recv() {
                        Ok(event) => match event {
                            DebouncedEvent::Write(path) | DebouncedEvent::Create(path) => {
                                log::info!("updated shader source of: {}", path.display(),);
                                version.fetch_add(1, Ordering::Relaxed);
                            }
                            _ => (),
                        },
                        Err(std::sync::mpsc::TryRecvError::Empty) => {
                            std::thread::sleep(std::time::Duration::from_millis(50))
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            log::error!("shader update thread disconnected");
                            return;
                        }
                    }
                }
            }
        }));

        ShaderCollection {
            closed,
            version,
            watcher,
            update_thread,
            shader_dir: shader_dir.as_ref().into(),
        }
    }

    pub fn load_if_newer<S: ShaderProgram>(
        &mut self,
        display: &glium::Display,
        shader: &mut Option<Shader<S>>,
    ) {
        let vertex_path = S::vertex_path(&self.shader_dir);
        let fragment_path = S::fragment_path(&self.shader_dir);

        let current_version = self.version();

        if let Some(shader) = shader {
            if current_version > shader.version {
                log::info!(
                    "updating shader: {} {}",
                    vertex_path.display(),
                    fragment_path.display()
                );
                let vertex_source = std::fs::read_to_string(&vertex_path).unwrap();
                let fragment_source = std::fs::read_to_string(&fragment_path).unwrap();

                let shader_result = program_from_source(display, &vertex_source, &fragment_source);
                match shader_result {
                    Ok(program) => {
                        *shader = Shader {
                            version: current_version,
                            program,
                            shader_type: Default::default(),
                        }
                    }
                    Err(error) => {
                        log::error!(
                            "unable to load shader: {} {} {}",
                            error,
                            vertex_path.display(),
                            fragment_path.display()
                        );
                        shader.version = current_version;
                    }
                }
            }
        } else {
            let _ = self
                .watcher
                .watch(vertex_path, notify::RecursiveMode::NonRecursive);
            let _ = self
                .watcher
                .watch(fragment_path, notify::RecursiveMode::NonRecursive);
            let shader_result =
                program_from_source(display, S::vertex_source(), S::fragment_source());
            match shader_result {
                Ok(program) => {
                    *shader = Some(Shader {
                        version: current_version,
                        program,
                        shader_type: Default::default(),
                    })
                }
                Err(error) => log::error!(
                    "unable to load shader: {} {} {}",
                    error,
                    S::VERTEX_RELATIVE_PATH,
                    S::FRAGMENT_RELATIVE_PATH
                ),
            }
        }
    }

    pub fn version(&self) -> usize {
        self.version.load(Ordering::Relaxed)
    }
}

fn program_from_source(
    display: &glium::Display,
    vertex_shader: &str,
    fragment_shader: &str,
) -> Result<glium::Program, glium::ProgramCreationError> {
    let input = ProgramCreationInput::SourceCode {
        vertex_shader,
        fragment_shader,
        tessellation_control_shader: None,
        tessellation_evaluation_shader: None,
        geometry_shader: None,
        transform_feedback_varyings: None,
        outputs_srgb: true,
        uses_point_size: false,
    };

    glium::Program::new(display, input)
}

pub struct Shader<S: ShaderProgram> {
    version: usize,
    program: glium::Program,
    shader_type: std::marker::PhantomData<S>,
}

impl<S: ShaderProgram> std::ops::Deref for Shader<S> {
    type Target = glium::Program;
    fn deref(&self) -> &Self::Target {
        &self.program
    }
}

impl std::ops::Drop for ShaderCollection {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::SeqCst);
        if let Some(thread) = self.update_thread.take() {
            thread.join().unwrap();
        }
    }
}
