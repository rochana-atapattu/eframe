// Cargo.toml
// ------------
// [package]
// name = "eframe_multiscreen_ws"
// version = "0.1.0"
// edition = "2021"
//
// [dependencies]
// eframe = "0.21"
// egui = "0.21"
// tokio = { version = "1", features = ["rt-multi-thread", "macros"] }
// tokio-tungstenite = "0.17"
// futures = "0.3"
// image = "0.24"
// bytes = "1.4"

use std::fmt::format;

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use tokio::runtime;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::time::{Duration, interval};

// A decoded RGBA frame ready for egui:
struct Frame {
    width: usize,
    height: usize,
    pixels: Vec<u8>, // RGBA8
}

struct Screen {
    id: usize,
    url: String,
    rx: mpsc::UnboundedReceiver<Frame>,
    texture: Option<TextureHandle>,
}

impl Screen {
    /// Poll incoming frames, update texture
    fn update_texture(&mut self, ctx: &egui::Context) {
        // Poll for new frames:
        if let Ok(frame) = self.rx.try_recv() {
            // Upload to egui texture:
            let size = [frame.width, frame.height];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, &frame.pixels);
            self.texture =
                Some(ctx.load_texture(format!("tex_{}", self.id), image, TextureOptions::NEAREST));
        }
    }
}

struct MyApp {
    rt_handle: Handle,
    next_id: usize,
    new_url: String,
    screens: Vec<Screen>,
}

impl MyApp {
    fn new(rt_handle: Handle) -> Self {
        Self {
            rt_handle,
            next_id: 0,
            new_url: "".into(),
            screens: Vec::new(),
        }
    }

    fn add_screen(&mut self, ctx: &egui::Context) {
        let url = std::mem::take(&mut self.new_url);
        let (tx, rx) = mpsc::unbounded_channel();
        let id = self.next_id;
        let ctx = ctx.clone();
        self.next_id += 1;

        // Spawn the WS + decode task:
        self.rt_handle.spawn(async move {
            let mut tick: u64 = 0;
            let mut interval = interval(Duration::from_millis(500));
            let width = 200;
            let height = 200;
            loop {
                interval.tick().await;
                let r = ((tick * 50) % 256) as u8;
                let g = ((tick * 80) % 256) as u8;
                let b = ((tick * 110) % 256) as u8;

                // Create a solid-color frame:
                let mut pixels = Vec::with_capacity(width * height * 4);
                for _ in 0..(width * height) {
                    pixels.extend_from_slice(&[r, g, b, 255]);
                }

                let _ = tx.send(Frame {
                    width,
                    height,
                    pixels,
                });
                tick += 1;
                ctx.request_repaint();
            }
        });

        self.screens.push(Screen {
            id,
            url,
            rx,
            texture: None,
        });
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("WebSocket URL:");
                ui.text_edit_singleline(&mut self.new_url);
                if ui.button("Add screen").clicked() && !self.new_url.is_empty() {
                    self.add_screen(ctx);
                }
            });
        });

        for (i, screen) in self.screens.iter_mut().enumerate() {
            screen.update_texture(ctx);

            ctx.show_viewport_immediate(
                egui::ViewportId::from_hash_of(format!("immediate_viewport_{}", i)),
                egui::ViewportBuilder::default()
                    .with_title("Immediate Viewport")
                    .with_inner_size([200.0, 100.0]),
                |ctx, class| {
                    assert!(
                        class == egui::ViewportClass::Immediate,
                        "This egui backend doesn't support multiple viewports"
                    );

                    egui::CentralPanel::default().show(ctx, |ui| {
                        if let Some(tex) = &screen.texture {
                            ui.image((tex.id(), ui.available_size()));
                        } else {
                            ui.label("Waiting…");
                        }
                    });
                },
            );

            // Throttle repaint to prevent OS 'not responding'
            // ctx.request_repaint_after(Duration::from_millis(100));
            // ctx.request_repaint();
        }
    }
}

fn main() -> eframe::Result {
    // Build a multithreaded runtime on which we can spawn tasks:
    let rt = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build runtime");

    let rt_handle = rt.handle().clone();

    let app = MyApp::new(rt_handle);

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "",
        native_options,
        Box::new(|cc| {
            // cc (CreationContext) provides egui context if needed for setup
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::<MyApp>::new(app))
        }),
    )
}
