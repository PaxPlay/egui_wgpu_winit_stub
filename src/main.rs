use winit::{
    application::ApplicationHandler,
    dpi::PhysicalSize,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop},
    window::Window
};
use std::sync::Arc;

mod ui;

struct App {
    app_resources: Option<AppResources>
}

impl App {
    fn new() -> Self {
        Self {
            app_resources: None
        }
    }

    fn get_app_resources(&mut self) -> &mut AppResources {
        self.app_resources.as_mut().unwrap()
    }
    
    fn get_window(&self) -> &Window {
        &self.app_resources.as_ref().unwrap().window
    }
}

struct AppResources {
    window: Arc<Window>,
    gpu_resources: GpuResources,
    ui_painter: egui_wgpu::Renderer,
    ui_state: egui_winit::State,
    ui_gallery: ui::WidgetGallery,
}

impl AppResources {
    fn new(event_loop: &ActiveEventLoop) -> Self {
        let attributes = Window::default_attributes().with_title("Cool Window");

        let window = Arc::new(event_loop.create_window(attributes).unwrap());

        let gpu_resources = pollster::block_on(GpuResources::new(&window));

        let ui_painter = egui_wgpu::Renderer::new(&gpu_resources.device, gpu_resources.surface_format, None, 1, false);
        let ui_context = egui::Context::default();
        let viewport_id = ui_context.viewport_id();
        let ui_state = egui_winit::State::new(ui_context, viewport_id, &window, None, None, None);

        Self {
            window,
            gpu_resources,
            ui_painter,
            ui_state,
            ui_gallery: ui::WidgetGallery::default(),
        }
    }

    fn draw_ui(&mut self, ce: &mut wgpu::CommandEncoder, render_pass: &mut wgpu::RenderPass<'static>) {
        let raw_input = self.ui_state.take_egui_input(&self.window);
        let ui_ctx = self.ui_state.egui_ctx();
        let ui_out = ui_ctx.run(raw_input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.label("Hello World");
                if ui.button("Click Me!").clicked() {
                    println!("Button Clicked!");
                }
            });

            self.ui_gallery.show(ctx);
        });


        let clipped_primitives = ui_ctx.tessellate(ui_out.shapes, ui_out.pixels_per_point);

        let r = &self.gpu_resources;
        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [r.surface_config.width, r.surface_config.height],
            pixels_per_point: ui_out.pixels_per_point,
        };

        for (id, delta) in ui_out.textures_delta.set {
            self.ui_painter.update_texture(&r.device, &r.queue, id, &delta);
        }

        self.ui_painter.update_buffers(&r.device, &r.queue, ce, &clipped_primitives, &screen_descriptor);
        self.ui_painter.render(render_pass, &clipped_primitives, &screen_descriptor);
    }

    fn do_render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let output = self.gpu_resources.surface.get_current_texture()?;
        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut ce = self.gpu_resources.device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        {
            let render_pass = ce.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store }
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            let mut rp_static = render_pass.forget_lifetime();
            self.draw_ui(&mut ce, &mut rp_static);
        }

        self.gpu_resources.queue.submit(std::iter::once(ce.finish()));
        output.present();

        Ok(())
    }

    fn on_window_event(&mut self, event: &winit::event::WindowEvent, window_id: winit::window::WindowId) -> bool {
        if self.window.id() == window_id {
            let response = self.ui_state.on_window_event(&self.window, event);
            if response.repaint {
                self.window.request_redraw();
            }

            response.consumed
        } else {
            false
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.app_resources = Some(AppResources::new(event_loop));
    }

    fn window_event(
            &mut self,
            event_loop: &ActiveEventLoop,
            window_id: winit::window::WindowId,
            event: winit::event::WindowEvent,
        ) {
        if self.get_app_resources().on_window_event(&event, window_id) {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            },
            WindowEvent::RedrawRequested => {
                self.get_app_resources().do_render().unwrap();
                self.get_window().request_redraw();
            },
            WindowEvent::Resized(physical_size) => {
                self.get_app_resources().gpu_resources.resize(physical_size);
            },
            _ => (),
        }
    }
}

#[allow(dead_code)]
struct GpuResources {
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    surface_format: wgpu::TextureFormat,
    surface_config: wgpu::SurfaceConfiguration,
}

impl GpuResources {
    async fn new(window: &Arc<Window>) -> GpuResources {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone()).unwrap();
        let adapter = instance.enumerate_adapters(wgpu::Backends::all()).into_iter()
            .filter(| adapter | adapter.is_surface_supported(&surface)).next().unwrap();

        let (device, queue) = adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("Egui Test Device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            ..Default::default() }, None).await.unwrap();

        let capabilities = surface.get_capabilities(&adapter);
        let surface_format = capabilities.formats
            .iter().copied().filter(|f| f.is_srgb()).next().unwrap_or(capabilities.formats[0]);

        let size = window.inner_size();
        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: capabilities.present_modes[0],
            alpha_mode: capabilities.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        Self {
            instance,
            surface,
            adapter,
            device,
            queue,
            surface_format,
            surface_config,
        }
    }

    fn resize(&mut self, size: PhysicalSize<u32>) {
        self.surface_config.width = size.width;
        self.surface_config.height = size.height;
        self.surface.configure(&self.device, &self.surface_config);
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App::new();
    event_loop.run_app(&mut app).unwrap();
}
