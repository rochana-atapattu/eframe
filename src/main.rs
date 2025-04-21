#[macro_use]
extern crate tracing;
use eframe::{egui, glow::DEPTH_FUNC};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{runtime, runtime::Handle, sync::watch, time::interval};
use tracing_subscriber::field::debug;

struct Screen {
    id: usize,
    label: String,
    visible: Arc<AtomicBool>,
    rx_frame: watch::Receiver<egui::ColorImage>,
    texture: Option<egui::TextureHandle>,
}

impl Screen {
    fn new(id: usize, label: String, rx: watch::Receiver<egui::ColorImage>) -> Self {
        Self {
            id,
            label,
            visible: Arc::new(AtomicBool::new(true)),
            rx_frame: rx,
            texture: None,
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

        // placeholder so watch always has something
        let placeholder = egui::ColorImage::example();
        let (tx, rx) = watch::channel(placeholder);
        let screen = Screen::new(id, label, rx);

        // spawn your frame‐producer task
        let ctx_clone = ctx.clone();
        self.rt_handle.spawn(async move {
            let mut tick = 0u64;
            let mut intv = interval(Duration::from_millis(500));
            let (w, h) = (200, 200);
            loop {
                intv.tick().await;
                let color = [
                    ((tick * 50) % 256) as u8,
                    ((tick * 80) % 256) as u8,
                    ((tick * 110) % 256) as u8,
                    255,
                ];
                let pixels = std::iter::repeat(color)
                    .take(w * h)
                    .flat_map(|c| c)
                    .collect::<Vec<_>>();
                let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &pixels);
                let _ = tx.send(img);
                debug!("sent");
                ctx_clone.request_repaint_of(egui::ViewportId::from_hash_of(screen.id as u64));
                tick += 1;
            }
        });

        self.screens.push(screen);
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        debug!("screens");
        // — add new screens —
        egui::TopBottomPanel::top("add").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Label:");
                ui.text_edit_singleline(&mut self.new_label);
                if ui.button("Add").clicked() && !self.new_label.is_empty() {
                    self.add_screen(ctx);
                }
            });
        });

        // — for each screen, pull & paint —
        for scr in &mut self.screens {
            if scr.visible.load(Ordering::Relaxed) {
                // 1) pull any new frame
                if scr.rx_frame.has_changed().unwrap_or(false) {
                    let img = scr.rx_frame.borrow_and_update().clone();
                    match &mut scr.texture {
                        Some(tex) => tex.set(img, egui::TextureOptions::default()),
                        None => {
                            let handle = ctx.load_texture(
                                format!("tex_{}", scr.id),
                                img.clone(),
                                egui::TextureOptions::default(),
                            );
                            scr.texture = Some(handle);
                        }
                    }
                }

                // 2) prepare the values the closure needs
                if let Some(tex) = &scr.texture {
                    let vis = scr.visible.clone();
                    let title = scr.label.clone();
                    let view_id = egui::ViewportId::from_hash_of(scr.id as u64);
                    let tex_id = tex.id();

                    ctx.show_viewport_deferred(
                        view_id,
                        egui::ViewportBuilder::default()
                            .with_title(title)
                            .with_inner_size([300.0, 300.0]),
                        move |ctx, _| {
                            egui::CentralPanel::default().show(ctx, |ui| {
                                ui.image((tex_id, ui.available_size()));
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
}

fn main() -> eframe::Result {
    setup_logging();
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

fn setup_logging() {
    use tracing::metadata::LevelFilter;
    use tracing_subscriber::EnvFilter;
    use tracing_subscriber::prelude::*;

    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::WARN.into())
        .with_env_var("APP_LOG")
        .from_env_lossy();

    let fmt_layer = tracing_subscriber::fmt::layer()
        .compact()
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_target(false);
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .try_init()
        .unwrap();
}
