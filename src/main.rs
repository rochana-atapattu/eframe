use eframe::egui;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use tokio::runtime;
use tokio::runtime::Handle;
use tokio::time::{Duration, interval};

struct Screen {
    id: usize,
    label: String,
    visible: Arc<AtomicBool>,
    texture: Arc<Mutex<egui::TextureHandle>>,
}

impl Screen {
    fn new(id: usize, label: String, ctx: &egui::Context) -> Self {
        // Create a placeholder 1x1 transparent image
        let empty = egui::ColorImage::example();
        let handle = ctx.load_texture(
            format!("tex_{}", id),
            empty,
            egui::TextureOptions::default(),
        );
        Self {
            id,
            label,
            visible: Arc::new(AtomicBool::new(true)),
            texture: Arc::new(Mutex::new(handle)),
        }
    }
}

struct MyApp {
    rt_handle: Handle,
    next_id: usize,
    new_label: String,
    screens: Vec<Screen>,
}

impl MyApp {
    fn new(rt_handle: Handle) -> Self {
        Self {
            rt_handle,
            next_id: 0,
            new_label: String::new(),
            screens: Vec::new(),
        }
    }

    fn add_screen(&mut self, ctx: &egui::Context) {
        let label = std::mem::take(&mut self.new_label);
        let id = self.next_id;
        self.next_id += 1;

        // Initialize screen with placeholder texture
        let screen = Screen::new(id, label.clone(), ctx);
        let texture = screen.texture.clone();
        let ctx_clone = ctx.clone();

        // Spawn dummy stream task that updates the texture directly
        self.rt_handle.spawn(async move {
            let mut tick = 0u64;
            let mut intv = interval(Duration::from_millis(500));
            let (w, h) = (200, 200);
            loop {
                intv.tick().await;
                // Generate solid-color frame
                let color = [
                    ((tick * 50) % 256) as u8,
                    ((tick * 80) % 256) as u8,
                    ((tick * 110) % 256) as u8,
                    255,
                ];
                let pixels = std::iter::repeat(color)
                    .take(w * h)
                    .flat_map(|c| c)
                    .collect::<Vec<u8>>();
                let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &pixels);

                // Update texture inside mutex
                if let Ok(mut guard) = texture.lock() {
                    guard.set(img, egui::TextureOptions::default());
                }

                // Request repaint
                ctx_clone.request_repaint_of(egui::ViewportId::from_hash_of(screen.id as u64));
                tick += 1;
            }
        });

        self.screens.push(screen);
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Top bar: add new screens
        egui::TopBottomPanel::top("add_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Label:");
                ui.text_edit_singleline(&mut self.new_label);
                if ui.button("Add").clicked() && !self.new_label.is_empty() {
                    self.add_screen(ctx);
                }
            });
        });

        // Deferred viewports
        for scr in &self.screens {
            if scr.visible.load(Ordering::Relaxed) {
                let vis = scr.visible.clone();
                let texture = scr.texture.clone();
                let title = scr.label.clone();
                let id = egui::ViewportId::from_hash_of(scr.id as u64);
                ctx.show_viewport_deferred(
                    id,
                    egui::ViewportBuilder::default()
                        .with_title(title)
                        .with_inner_size([300.0, 300.0]),
                    move |ctx, _class| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            let size = ui.available_size();
                            if let Ok(lock) = texture.lock() {
                                let tex = &*lock;
                                ui.image((tex.id(), size));
                            }
                        });
                        if ctx.input(|i| i.viewport().close_requested()) {
                            vis.store(false, Ordering::Relaxed);
                        }
                    },
                );
            }
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
