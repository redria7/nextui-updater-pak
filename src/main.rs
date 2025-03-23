use egui::Button;
use egui::FullOutput;
use egui_backend::egui;
use egui_backend::{sdl2::event::Event, DpiScaling, ShaderVersion};
use egui_sdl2_gl as egui_backend;
use serde::Deserialize;
use std::fs::File;
use std::io::Cursor;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Deserialize, Clone)]
struct Asset {
    browser_download_url: String,
}

#[derive(Deserialize, Clone)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

struct AppState {
    latest_release: Option<Release>,
    current_operation: Option<String>,
    progress: Option<f32>,
}
fn do_update(app_state: Arc<Mutex<AppState>>) {
    // Use a new thread to not block the UI
    std::thread::spawn(move || {
        // Update NextUI from https://github.com/LoveRetro/NextUI

        {
            let mut app_state = app_state.lock().unwrap();
            app_state.current_operation = Some("Fetching latest release...".to_string());
        }

        let url = "https://api.github.com/repos/LoveRetro/NextUI/releases/latest";

        let client = reqwest::blocking::Client::new();

        let response = client
            .get(url)
            .header("User-Agent", "NextUI Updater")
            .send()
            .unwrap();

        let release: Release = response.json().unwrap();

        {
            let mut app_state = app_state.lock().unwrap();
            app_state.latest_release = Some(release.clone());
            app_state.current_operation = Some("Downloading...".to_string());
        }

        let assets = release.assets;
        let asset = assets.first().unwrap();
        let download_url = asset.browser_download_url.clone();

        let response = reqwest::blocking::get(&download_url).unwrap();

        {
            let mut app_state = app_state.lock().unwrap();
            app_state.current_operation = Some("Extracting...".to_string());
        }

        let mut archive = zip::ZipArchive::new(Cursor::new(response.bytes().unwrap())).unwrap();

        let mut archive_file = match archive.by_name("MinUI.zip") {
            Ok(file) => file,
            Err(..) => {
                println!("File MinUI.zip not found in archive");
                return;
            }
        };

        let mut file = File::create("/mnt/SDCARD/MinUI.zip").unwrap();
        std::io::copy(&mut archive_file, &mut file).unwrap();

        {
            let mut app_state = app_state.lock().unwrap();
            app_state.current_operation = Some("Rebooting...".to_string());
        }

        let res = std::process::Command::new("reboot").output().unwrap();
        println!("{:?}", res);
    });
}

fn main() {
    let sdl_context = sdl2::init().unwrap();
    let video_subsystem = sdl_context.video().unwrap();
    let game_controller_subsystem = sdl_context.game_controller().unwrap();
    let available = game_controller_subsystem
        .num_joysticks()
        .map_err(|e| format!("can't enumerate joysticks: {}", e))
        .unwrap();
    let _controller = (0..available).find_map(|id| {
        if !game_controller_subsystem.is_game_controller(id) {
            return None;
        }

        match game_controller_subsystem.open(id) {
            Ok(c) => Some(c),
            Err(e) => {
                println!("failed: {:?}", e);
                None
            }
        }
    });
    let window = video_subsystem
        .window("Updater", 1024, 768)
        .input_grabbed()
        .opengl()
        .build()
        .unwrap();

    let _ctx = window.gl_create_context().unwrap();
    let shader_ver = ShaderVersion::Adaptive;
    let (mut painter, mut egui_state) =
        egui_backend::with_sdl2(&window, shader_ver, DpiScaling::Custom(3.0));
    let egui_ctx = egui::Context::default();
    let mut event_pump = sdl_context.event_pump().unwrap();

    let mut quit = false;
    let app_state = Arc::new(Mutex::new(AppState {
        latest_release: None,
        current_operation: None,
        progress: None,
    }));

    let start_time = Instant::now();

    let mut style = egui::Style::default();

    style.visuals.panel_fill = egui::Color32::from_rgb(0, 0, 0);
    style.visuals.override_text_color = Some(egui::Color32::from_rgb(255, 255, 255));

    egui_ctx.set_style_of(egui::Theme::Dark, style);

    'running: loop {
        egui_state.input.time = Some(start_time.elapsed().as_secs_f64());
        egui_ctx.begin_pass(egui_state.input.take());

        egui::CentralPanel::default().show(&egui_ctx, |ui| {
            ui.vertical_centered_justified(|ui| {
                ui.heading("NextUI Updater");

                ui.add_space(16.0);

                let first_button = ui.add_enabled(
                    app_state.lock().unwrap().current_operation.is_none(),
                    Button::new("Update"),
                );
                if let Some(state) = &app_state.lock().unwrap().current_operation {
                    ui.label(state);
                }
                if first_button.clicked() {
                    do_update(app_state.clone());
                }

                if let Some(release) = &app_state.lock().unwrap().latest_release {
                    ui.label(format!("Latest release: {}", release.tag_name));
                }

                if let Some(progress) = &app_state.lock().unwrap().progress {
                    ui.add(egui::ProgressBar::new(*progress).show_percentage());
                }

                ui.separator();

                if ui.button("Quit").clicked() {
                    quit = true;
                }

                // Focus the first input element to start interaction with game controller
                ui.memory_mut(|r| {
                    if r.focused().is_none() {
                        r.request_focus(first_button.id);
                    }
                });
            });
        });

        let FullOutput {
            platform_output,
            textures_delta,
            shapes,
            pixels_per_point,
            viewport_output,
        } = egui_ctx.end_pass();

        // Process ouput
        egui_state.process_output(&window, &platform_output);

        let paint_jobs = egui_ctx.tessellate(shapes, pixels_per_point);
        painter.paint_jobs(None, textures_delta, paint_jobs);
        window.gl_swap_window();

        let repaint_after = viewport_output
            .get(&egui::ViewportId::ROOT)
            .expect("Missing ViewportId::ROOT")
            .repaint_delay;

        fn controller_to_key(button: sdl2::controller::Button) -> Option<sdl2::keyboard::Keycode> {
            match button {
                sdl2::controller::Button::DPadUp => Some(sdl2::keyboard::Keycode::Up),
                sdl2::controller::Button::DPadDown => Some(sdl2::keyboard::Keycode::Down),
                sdl2::controller::Button::DPadLeft => Some(sdl2::keyboard::Keycode::Left),
                sdl2::controller::Button::DPadRight => Some(sdl2::keyboard::Keycode::Right),
                sdl2::controller::Button::B => Some(sdl2::keyboard::Keycode::Return),
                sdl2::controller::Button::A => Some(sdl2::keyboard::Keycode::Escape),
                _ => None,
            }
        }

        let mut process_controller_event =
            |key: sdl2::controller::Button, timestamp: u32, down: bool| {
                if let Some(keycode) = controller_to_key(key) {
                    let event = if down {
                        sdl2::event::Event::KeyDown {
                            keycode: Some(keycode),
                            timestamp,
                            window_id: window.id(),
                            scancode: Some(sdl2::keyboard::Scancode::Down),
                            keymod: sdl2::keyboard::Mod::empty(),
                            repeat: true,
                        }
                    } else {
                        sdl2::event::Event::KeyUp {
                            keycode: Some(keycode),
                            timestamp,
                            window_id: window.id(),
                            scancode: Some(sdl2::keyboard::Scancode::Down),
                            keymod: sdl2::keyboard::Mod::empty(),
                            repeat: true,
                        }
                    };

                    egui_state.process_input(&window, event, &mut painter);
                }
            };

        if !repaint_after.is_zero() {
            if let Some(event) = event_pump.wait_event_timeout(5) {
                match event {
                    Event::Quit { .. } => break 'running,
                    Event::ControllerButtonDown {
                        timestamp, button, ..
                    } => process_controller_event(button, timestamp, true),
                    Event::ControllerButtonUp {
                        timestamp, button, ..
                    } => process_controller_event(button, timestamp, false),
                    _ => {
                        // Process input event
                        egui_state.process_input(&window, event, &mut painter);
                    }
                }
            }
        }

        if quit {
            break;
        }
    }
}
