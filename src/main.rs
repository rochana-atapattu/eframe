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

    fn add_screen(&mut self) {
        let url = std::mem::take(&mut self.new_url);
        let (tx, rx) = mpsc::unbounded_channel();
        let id = self.next_id;
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
                    self.add_screen();
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            // Lay out each screen in its own group:
            for screen in &mut self.screens {
                ui.group(|ui| {
                    ui.label(format!("Screen #{}: {}", screen.id, screen.url));

                    // Poll for new frames:
                    while let Ok(frame) = screen.rx.try_recv() {
                        // Upload to egui texture:
                        let size = [frame.width, frame.height];
                        let image = egui::ColorImage::from_rgba_unmultiplied(size, &frame.pixels);
                        screen.texture = Some(ctx.load_texture(
                            format!("tex_{}", screen.id),
                            image,
                            TextureOptions::NEAREST,
                        ));
                    }

                    // Draw the last texture if any:
                    if let Some(tex) = &screen.texture {
                        // Keep aspect ratio:
                        ui.image(tex);
                    } else {
                        ui.label("Waiting for first frame…");
                    }
                });
                ui.separator();
            }
        });

        // Request repaint so we animate as frames come in:
        ctx.request_repaint();
    }
}

fn main() -> eframe::Result {
    // Build a multithreaded runtime on which we can spawn tasks:
    let rt = runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build runtime");

    let rt_handle = rt.handle().clone();
    // Prevent the runtime from dropping:
    let _guard = rt.enter();

    let app = MyApp::new(rt_handle);

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "",
        native_options,
        Box::new(|cc| {
            // cc (CreationContext) provides egui context if needed for setup
            Ok(Box::<MyApp>::new(app))
        }),
    )
}
