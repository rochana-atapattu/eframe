#[macro_use]
extern crate tracing;
use crossbeam::channel::{Receiver, Sender, unbounded};
use eframe::{egui, glow::DEPTH_FUNC};
use std::cell::RefCell;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{runtime, runtime::Handle, time::interval};
use tracing_subscriber::field::debug;

struct Screen {
    rt_handle: Handle,
    id: usize,
    label: String,
    visible: Arc<AtomicBool>,
    texture: Option<egui::TextureHandle>,
    // tx lives on the UI‐thread struct, but can be cloned and sent into the background task
    tx: Sender<egui::ColorImage>,
    // rx is only ever touched on the UI thread in the callback,
    // so we can wrap it in RefCell for interior mutability
    rx: Receiver<egui::ColorImage>,
}

impl Screen {
    fn new(id: usize, label: String, rt_handle: Handle) -> Self {
        let (tx, rx) = unbounded::<egui::ColorImage>();
        // Create a placeholder 1x1 transparent image
        Self {
            rt_handle,
            id,
            label,
            visible: Arc::new(AtomicBool::new(true)),
            texture: None,
            tx,
            rx,
        }
    }

    /// Now borrows &mut self instead of consuming.
    fn start(&mut self, ctx: egui::Context) {
        // clone only what we need into the task
        let tx = self.tx.clone();
        let visible = self.visible.clone();
        let id = self.id;
        let mut ctx = ctx.clone();

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
                    .collect::<Vec<u8>>();
                let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &pixels);

                // ask egui for a repaint

                // actually send the image
                if let Err(e) = tx.send(img) {
                    debug!("{}: failed to send image: {:?}", id, e);
                    // if the receiver is gone, stop the loop
                    visible.store(false, Ordering::Relaxed);
                    break;
                }
                ctx.request_repaint();
                debug!("{}: sent image", id);
                tick += 1;
            }
        });
    }

    fn viewport_callback(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        // pull in any new frames
        while let Ok(img) = self.rx.try_recv() {
            if let Some(tex_handle) = &mut self.texture {
                tex_handle.set(img.clone(), egui::TextureOptions::default());
            } else {
                let handle = ctx.load_texture(
                    format!("tex_{}", self.id),
                    img.clone(),
                    egui::TextureOptions::default(),
                );
                self.texture = Some(handle);
            }
        }

        // now draw whatever texture we have
        if let Some(tex_handle) = &self.texture {
            let tex_id = tex_handle.id();
            ui.image((tex_id, ui.available_size()));
        }

        // close-requested?
        // if ctx.input(|i| i.close_requested()) {
        //     self.visible.store(false, Ordering::Relaxed);
        // }
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

        // spawn your frame‐producer task
        let mut screen = Screen::new(id, label.clone(), self.rt_handle.clone());
        screen.start(ctx.clone());

        // fix error: use of moved value: screen
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
                // 2) prepare the values the closure needs
                egui::Window::new(&scr.label)
                    .default_size([300.0, 300.0])
                    .show(ctx, |ui| {
                        scr.viewport_callback(ctx, ui);
                    });
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
        "image",
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
