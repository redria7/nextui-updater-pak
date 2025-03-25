use egui::{Button, Color32, FullOutput, ProgressBar};
use egui_backend::egui;
use egui_backend::{sdl2::event::Event, DpiScaling, ShaderVersion};
use egui_sdl2_gl as egui_backend;
use egui_sdl2_gl::egui::RichText;
use serde::Deserialize;
use std::process::exit;
use std::{
    fs::File,
    io::{Cursor, Read, Write},
    sync::{Arc, Mutex},
    thread,
    time::Instant,
};

// Define GitHub API response structures
#[derive(Deserialize, Clone, Debug)]
struct Asset {
    name: String,
    url: String,
}

#[derive(Deserialize, Clone, Debug)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

// Application state shared between UI thread and update thread
struct AppState {
    latest_release: Option<Release>,
    current_operation: Option<String>,
    progress: Option<f32>,
    error: Option<String>,
}

// Constants
const USER_AGENT: &str = "NextUI Updater";
const OUTPUT_PATH: &str = "/mnt/SDCARD/";
const WINDOW_WIDTH: u32 = 1024;
const WINDOW_HEIGHT: u32 = 768;
const DPI_SCALE: f32 = 3.0;

// Error type for the application
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn fetch_latest_release(repo: &str) -> Result<Release> {
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(format!(
            "https://api.github.com/repos/{repo}/releases/latest"
        ))
        .header("User-Agent", USER_AGENT)
        .send()?;

    if !response.status().is_success() {
        return Err(format!("GitHub API request failed: {}", response.status()).into());
    }

    Ok(response.json()?)
}

fn self_update(app_state: Arc<Mutex<AppState>>) -> Result<()> {
    // Fetch latest release information
    {
        let mut state = app_state.lock().unwrap();
        state.current_operation = Some("Fetching latest updater release...".to_string());
    }

    let release = fetch_latest_release("LanderN/nextui-updater-pak")?;

    let available = semver::Version::parse(&release.tag_name)?;
    let installed = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;

    if available > installed {
        println!(
            "New version available: {} (current: {})",
            available, installed
        );

        let mut state = app_state.lock().unwrap();
        state.current_operation = Some("Downloading updater...".to_string());
        state.progress = Some(0.3);
    } else {
        println!("No updates available");

        return Ok(());
    }

    let client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .timeout(None)
        .build()?;

    let response = client
        .get(&release.assets[0].url)
        .header("Accept", "application/octet-stream")
        .header("User-Agent", USER_AGENT)
        .send()?;

    println!("Status: {}", response.status());
    println!("Headers: {:?}", response.headers());

    let bytes = response.bytes()?;
    {
        let mut state = app_state.lock().unwrap();
        state.current_operation =
            format!("Extracting NextUI Updater {}...", release.tag_name).into();
        state.progress = Some(0.6);
    }

    // Move the current binary to a backup location
    let current_binary = std::env::current_exe()?;
    std::fs::rename(&current_binary, current_binary.with_extension("bak"))?;

    // Extract the update package
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let result = archive.extract(OUTPUT_PATH);

    if result.is_err() {
        // Move the backup back
        std::fs::rename(current_binary.with_extension("bak"), current_binary)?;

        return Err("Failed to extract update package".into());
    }

    {
        let mut state = app_state.lock().unwrap();
        state.current_operation = Some("Self-update success! Restarting updater...".to_string());
        state.progress = Some(1.0);
    }

    // Give the user a moment to see the completion message
    thread::sleep(std::time::Duration::from_secs(1));

    // "5" is the exit code for "restart required"
    exit(5);
}

fn do_init(app_state: Arc<Mutex<AppState>>) {
    thread::spawn(move || {
        // Do self-update
        match self_update(app_state.clone()) {
            Ok(()) => {
                let mut state = app_state.lock().unwrap();
                state.current_operation = None;
                state.progress = None;
            }
            Err(err) => {
                let mut state = app_state.lock().unwrap();
                state.current_operation = None;
                state.error = Some(format!("Self-update failed: {}", err));
                state.progress = None;

                // Give the user a moment to see the error message
                thread::sleep(std::time::Duration::from_secs(1));
            }
        }

        // Fetch latest release information
        {
            let mut state = app_state.lock().unwrap();
            state.current_operation = Some("Fetching latest NextUI release...".to_string());
        }

        match fetch_latest_release("LoveRetro/NextUI") {
            Ok(release) => {
                let mut state = app_state.lock().unwrap();
                state.latest_release = Some(release.clone());
                state.current_operation = None;
            }
            Err(err) => {
                let mut state = app_state.lock().unwrap();
                state.current_operation = None;
                state.error = Some(format!("Fetch failed: {}", err));
            }
        }
    });
}

fn do_update(app_state: Arc<Mutex<AppState>>, full: bool) {
    thread::spawn(move || {
        if let Err(err) = update_process(app_state.clone(), full) {
            let mut state = app_state.lock().unwrap();
            state.current_operation = None;
            state.error = Some(format!("Update failed: {}", err));
        }
    });
}

fn update_process(app_state: Arc<Mutex<AppState>>, full: bool) -> Result<()> {
    let release = {
        let mut state = app_state.lock().unwrap();
        state.current_operation = Some("Downloading update...".to_string());
        state.progress = Some(0.3);

        state.latest_release.clone().ok_or("No release found")?
    };

    let client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .timeout(None)
        .build()?;

    for asset in release.assets.iter() {
        // Download the asset
        {
            let mut state = app_state.lock().unwrap();
            state.current_operation = format!("Downloading {}...", asset.name).into();
            state.progress = Some(0.3);
        }

        println!("Downloading from {}", asset.url);

        let response = client
            .get(&asset.url)
            .header("Accept", "application/octet-stream")
            .header("User-Agent", USER_AGENT)
            .send()?;

        println!("Status: {}", response.status());
        println!("Headers: {:?}", response.headers());

        let bytes = response.bytes()?;

        {
            let mut state = app_state.lock().unwrap();
            state.current_operation = format!("Extracting {}...", asset.name).into();
            state.progress = Some(0.6);
        }

        // Extract the update package
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;

        if !full {
            // "Quick" update, just extract MinUI.zip

            // Look for MinUI.zip in the archive
            let mut minui_data = Vec::new();
            match archive.by_name("MinUI.zip") {
                Ok(mut file) => {
                    file.read_to_end(&mut minui_data)?;
                }
                Err(_) => return Err("File MinUI.zip not found in archive".into()),
            }

            // Write the extracted file
            let mut file = File::create([OUTPUT_PATH, "MinUI.zip"].join("/"))?;
            file.write_all(&minui_data)?;

            break; // Done!
        } else {
            // Full update, extract all files
            archive.extract(OUTPUT_PATH)?;
        }
    }

    {
        let mut state = app_state.lock().unwrap();
        state.current_operation = Some("Update complete, preparing to reboot...".to_string());
        state.progress = Some(0.9);
    }

    // Give the user a moment to see the completion message
    thread::sleep(std::time::Duration::from_secs(2));

    {
        let mut state = app_state.lock().unwrap();
        state.current_operation = Some("Rebooting system...".to_string());
        state.progress = Some(1.0);
    }

    // Reboot the system
    match std::process::Command::new("reboot").output() {
        Ok(_) => Ok(()),
        Err(e) => Err(Box::new(e)),
    }
}

// Map controller buttons to keyboard keys
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

fn setup_ui_style() -> egui::Style {
    let mut style = egui::Style::default();
    style.visuals.panel_fill = Color32::from_rgb(0, 0, 0);
    style.visuals.override_text_color = Some(Color32::from_rgb(255, 255, 255));
    style.visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(30, 30, 30);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(40, 40, 40);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(70, 70, 70);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(90, 90, 90);
    style.visuals.widgets.noninteractive.fg_stroke.color = Color32::from_rgb(200, 200, 200);
    style
}

fn init_sdl() -> Result<(
    sdl2::Sdl,
    sdl2::video::Window,
    sdl2::EventPump,
    Option<sdl2::controller::GameController>,
)> {
    let sdl_context = sdl2::init()?;
    let video_subsystem = sdl_context.video()?;

    // Initialize game controller subsystem
    let game_controller_subsystem = sdl_context.game_controller()?;
    let available = game_controller_subsystem.num_joysticks()?;

    // Attempt to open the first available game controller
    let controller = (0..available).find_map(|id| {
        if !game_controller_subsystem.is_game_controller(id) {
            return None;
        }

        match game_controller_subsystem.open(id) {
            Ok(c) => Some(c),
            Err(e) => {
                eprintln!("Failed to open controller {}: {:?}", id, e);
                None
            }
        }
    });

    // Create a window
    let window = video_subsystem
        .window(
            &format!("NextUI Updater {}", env!("CARGO_PKG_VERSION")),
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
        )
        .position_centered()
        .opengl()
        .build()?;

    let event_pump = sdl_context.event_pump()?;

    Ok((sdl_context, window, event_pump, controller))
}

fn main() -> Result<()> {
    // Initialize SDL and create window
    let (_sdl_context, window, mut event_pump, _controller) = init_sdl()?;

    // Create OpenGL context and egui painter
    let _gl_context = window.gl_create_context()?;
    let shader_ver = ShaderVersion::Adaptive;
    let (mut painter, mut egui_state) =
        egui_backend::with_sdl2(&window, shader_ver, DpiScaling::Custom(DPI_SCALE));

    // Create egui context and set style
    let egui_ctx = egui::Context::default();
    egui_ctx.set_style(setup_ui_style());

    // Initialize application state
    let app_state = Arc::new(Mutex::new(AppState {
        latest_release: None,
        current_operation: None,
        progress: None,
        error: None,
    }));

    // Self-update + fetch latest release information
    do_init(app_state.clone());

    let start_time: Instant = Instant::now();
    let mut quit = false;

    'running: loop {
        egui_state.input.time = Some(start_time.elapsed().as_secs_f64());
        egui_ctx.begin_pass(egui_state.input.take());

        // UI rendering
        egui::CentralPanel::default().show(&egui_ctx, |ui| {
            ui.vertical_centered_justified(|ui| {
                ui.heading(format!("NextUI Updater {}", env!("CARGO_PKG_VERSION")));

                // Check application state
                let state_lock = app_state.lock().unwrap();
                let update_in_progress = state_lock.current_operation.is_some();
                drop(state_lock);

                // Quit button
                ui.add_space(8.0);
                if ui.button("Quit").clicked() {
                    quit = true;
                }
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                // Show release information if available
                if let Some(release) = &app_state.lock().unwrap().latest_release {
                    ui.label(format!("Latest version: {}", release.tag_name));
                    ui.add_space(8.0);
                }

                // Update buttons
                let quick_update_button =
                    ui.add_enabled(!update_in_progress, Button::new("Quick Update"));
                ui.label(
                    RichText::new("MinUI.zip only")
                        .font(egui::FontId::proportional(8.0))
                        .color(Color32::from_rgb(150, 150, 150)),
                );

                ui.add_space(8.0);

                let full_update_button =
                    ui.add_enabled(!update_in_progress, Button::new("Full Update"));
                ui.label(
                    RichText::new("Extract full zip files (base + extras)")
                        .font(egui::FontId::proportional(8.0))
                        .color(Color32::from_rgb(150, 150, 150)),
                );

                ui.add_space(8.0);

                // Display current operation
                if let Some(operation) = &app_state.lock().unwrap().current_operation {
                    ui.label(operation);
                }

                // Display error if any
                if let Some(error) = &app_state.lock().unwrap().error {
                    ui.add_space(8.0);
                    ui.colored_label(Color32::from_rgb(255, 100, 100), error);
                }

                // Show progress bar if available
                if let Some(progress) = app_state.lock().unwrap().progress {
                    ui.add_space(8.0);
                    ui.add(ProgressBar::new(progress).show_percentage().animate(true));
                }

                // Initiate update if button clicked
                if quick_update_button.clicked() {
                    // Clear any previous errors
                    app_state.lock().unwrap().error = None;
                    do_update(app_state.clone(), false);
                }

                if full_update_button.clicked() {
                    // Clear any previous errors
                    app_state.lock().unwrap().error = None;
                    do_update(app_state.clone(), true);
                }

                // Focus the update button for controller navigation
                ui.memory_mut(|r| {
                    if r.focused().is_none() {
                        r.request_focus(quick_update_button.id);
                    }
                });
            });
        });

        // End frame and render
        let FullOutput {
            platform_output,
            textures_delta,
            shapes,
            pixels_per_point,
            viewport_output: _,
        } = egui_ctx.end_pass();

        // Process output
        egui_state.process_output(&window, &platform_output);

        // Paint and swap buffers
        let paint_jobs = egui_ctx.tessellate(shapes, pixels_per_point);
        painter.paint_jobs(None, textures_delta, paint_jobs);
        window.gl_swap_window();

        // Process events
        if let Some(event) = event_pump.wait_event_timeout(5) {
            match event {
                Event::Quit { .. } => break 'running,
                Event::ControllerButtonDown {
                    timestamp, button, ..
                } => {
                    if let Some(keycode) = controller_to_key(button) {
                        let key_event = Event::KeyDown {
                            keycode: Some(keycode),
                            timestamp,
                            window_id: window.id(),
                            scancode: Some(sdl2::keyboard::Scancode::Down),
                            keymod: sdl2::keyboard::Mod::empty(),
                            repeat: false,
                        };
                        egui_state.process_input(&window, key_event, &mut painter);
                    }
                }
                Event::ControllerButtonUp {
                    timestamp, button, ..
                } => {
                    if let Some(keycode) = controller_to_key(button) {
                        let key_event = Event::KeyUp {
                            keycode: Some(keycode),
                            timestamp,
                            window_id: window.id(),
                            scancode: Some(sdl2::keyboard::Scancode::Down),
                            keymod: sdl2::keyboard::Mod::empty(),
                            repeat: false,
                        };
                        egui_state.process_input(&window, key_event, &mut painter);
                    }
                }
                _ => {
                    // Process other input events
                    egui_state.process_input(&window, event, &mut painter);
                }
            }
        }

        if quit {
            break;
        }
    }

    Ok(())
}
