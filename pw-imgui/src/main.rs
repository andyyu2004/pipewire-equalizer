use std::{num::NonZeroU32, sync::Arc, time::Instant};

mod state;
mod pipewire;
mod autoeq;
mod filter;

use dear_imgui_glow::GlowRenderer;
use dear_imgui_rs::*;
use dear_imgui_winit::{HiDpiMode, WinitPlatform};
use dear_implot::{PlotContext, ImPlotExt};
use glow::HasContext;
use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext},
    display::{GetGlDisplay, GlDisplay},
    surface::{GlSurface, Surface, SurfaceAttributesBuilder, WindowSurface},
};
use pw_util::NodeInfo;
use raw_window_handle::HasWindowHandle;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

use state::ImguiState;
use pipewire::PipewireState;

struct AppWindow {
    imgui: ImguiState,
    context: PossiblyCurrentContext,
    surface: Surface<WindowSurface>,
    window: Arc<Window>,
    pipewire: PipewireState,
}

struct App {
    window: Option<AppWindow>,
    default_audio_sink: Option<NodeInfo>,
}

impl AppWindow {
    fn new(event_loop: &ActiveEventLoop, default_audio_sink: Option<NodeInfo>) -> Result<Self, Box<dyn std::error::Error>> {
        // Create window with OpenGL context
        let window_attributes = winit::window::Window::default_attributes()
            .with_title("pw-imgui")
            .with_inner_size(LogicalSize::new(900.0, 900.0));

        let (window, cfg) = glutin_winit::DisplayBuilder::new()
            .with_window_attributes(Some(window_attributes))
            .build(event_loop, ConfigTemplateBuilder::new(), |mut configs| {
                configs.next().unwrap()
            })?;

        assert!(window.is_some());
        let window = Arc::new(window.unwrap());

        // Create OpenGL context
        let context_attribs =
            ContextAttributesBuilder::new().build(Some(window.window_handle()?.as_raw()));
        let context = unsafe { cfg.display().create_context(&cfg, &context_attribs)? };

        // Create surface (request sRGB-capable framebuffer for consistent visuals)
        let size = window.inner_size();
        let surface_attribs = SurfaceAttributesBuilder::<WindowSurface>::new()
            .with_srgb(Some(true))
            .build(
                window.window_handle()?.as_raw(),
                NonZeroU32::new(size.width.max(1)).unwrap(),
                NonZeroU32::new(size.height.max(1)).unwrap(),
            );
        let surface = unsafe {
            cfg.display()
                .create_window_surface(&cfg, &surface_attribs)?
        };

        let context = context.make_current(&surface)?;

        // Setup Dear ImGui
        let mut imgui_context = Context::create();
        imgui_context.set_ini_filename(None::<String>).unwrap();

        let scale_factor = window.scale_factor();
        imgui_context.io_mut().set_config_dpi_scale_fonts(true);

        const FONT_DATA: &[u8] = include_bytes!("../ProggyVector.ttf");
        imgui_context.fonts().add_font(&[FontSource::TtfData {
            data: FONT_DATA,
            size_pixels: Some(13.0),
            config: None,
        }]);

        let mut platform = WinitPlatform::new(&mut imgui_context);
        let dpi_mode = if scale_factor != 1.0 {
            HiDpiMode::Locked(scale_factor)
        } else {
            HiDpiMode::Default
        };
        platform.attach_window(
            &window,
            dpi_mode,
            &mut imgui_context,
        );

        // Create Glow context and renderer
        let gl = unsafe {
            glow::Context::from_loader_function_cstr(|s| {
                context.display().get_proc_address(s).cast()
            })
        };

        let mut renderer = GlowRenderer::new(gl, &mut imgui_context)?;
        // Use sRGB framebuffer: enable FRAMEBUFFER_SRGB during ImGui rendering

        renderer.set_framebuffer_srgb_enabled(true);
        renderer.new_frame()?;

        let pipewire = PipewireState::new(default_audio_sink);

        let imgui = ImguiState {
            plot_context: PlotContext::create(&imgui_context),
            context: imgui_context,
            platform,
            renderer,
            clear_color: [0.1, 0.2, 0.3, 1.0],
            last_frame: Instant::now(),
            auto_eq: autoeq::AutoEqWindowState::new(pipewire.notifs_tx.clone()),
            filter: filter::FilterWindowState::new(pipewire.sample_rate),
        };

        Ok(Self {
            window,
            surface,
            context,
            imgui,
            pipewire,
        })
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.surface.resize(
                &self.context,
                NonZeroU32::new(new_size.width).unwrap(),
                NonZeroU32::new(new_size.height).unwrap(),
            );
        }
    }

    fn render(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let now = Instant::now();
        let delta_time = now - self.imgui.last_frame;
        self.imgui
            .context
            .io_mut()
            .set_delta_time(delta_time.as_secs_f32());
        self.imgui.last_frame = now;

        self.imgui
            .platform
            .prepare_frame(&self.window, &mut self.imgui.context);

        let ui = self.imgui.context.frame();

        let mut opened = true;
        ui.show_demo_window(&mut opened);

        // AutoEq window
        self.imgui.auto_eq.draw_window(ui, self.pipewire.sample_rate);
        if let Some((name, eq)) = self.imgui.auto_eq.get_eq_to_set(){
            self.imgui.filter.set_eq(name, eq);
        }

        // Filter window
        let plot_ui = ui.implot(&self.imgui.plot_context);
        self.imgui.filter.draw_window(ui, &plot_ui, self.pipewire.sample_rate);

        // Render
        let gl = self.imgui.renderer.gl_context().unwrap();
        unsafe {
            // Enable sRGB write for clear on sRGB-capable surface
            gl.enable(glow::FRAMEBUFFER_SRGB);
            gl.clear_color(
                self.imgui.clear_color[0],
                self.imgui.clear_color[1],
                self.imgui.clear_color[2],
                self.imgui.clear_color[3],
            );
            gl.clear(glow::COLOR_BUFFER_BIT);
            gl.disable(glow::FRAMEBUFFER_SRGB);
        }

        self.imgui
            .platform
            .prepare_render_with_ui(&ui, &self.window);
        let draw_data = self.imgui.context.render();

        self.imgui.renderer.new_frame()?;
        self.imgui.renderer.render(&draw_data)?;

        self.surface.swap_buffers(&self.context)?;

        self.pipewire.update(&mut self.imgui.auto_eq);

        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            match AppWindow::new(event_loop, self.default_audio_sink.take()) {
                Ok(window) => {
                    // Request initial redraw to start the render loop
                    window.window.request_redraw();
                    self.window = Some(window);
                }
                Err(e) => {
                    eprintln!("Failed to create window: {e}");
                    event_loop.exit();
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let window = match self.window.as_mut() {
            Some(window) => window,
            None => return,
        };

        // Handle the event with ImGui first (window-local path)
        window.imgui.platform.handle_window_event(
            &mut window.imgui.context,
            &window.window,
            &event,
        );

        match event {
            WindowEvent::Resized(physical_size) => {
                window.resize(physical_size);
                window.window.request_redraw();
            }
            WindowEvent::CloseRequested => {
                println!("Close requested");
                window.pipewire.close();
                event_loop.exit();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.logical_key == Key::Named(NamedKey::Escape) {
                    event_loop.exit();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = window.render() {
                    eprintln!("Render error: {e}");
                }
                window.window.request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.window.request_redraw();
        }
    }
}

#[tokio::main]
async fn main() {
    env_logger::init();

    let default_audio_sink = match pw_util::get_default_audio_sink().await {
        Ok(node) => { tracing::info!(?node, "detected default audio sink"); Some(node) }
        Err(err) => { tracing::error!(error = &*err, "failed to get default audio sink"); None }
    };

    if default_audio_sink.is_none() {
        println!("Unable to get default audio sink");
    }
    else {
        println!("Got default audio sink");
    }

    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App {
        window: None,
        default_audio_sink: default_audio_sink
    };

    event_loop.run_app(&mut app).unwrap();
}
