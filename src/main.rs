use egui::{Button, Color32, FullOutput, ProgressBar};
use egui_backend::egui;
use egui_backend::{sdl2::event::Event, DpiScaling, ShaderVersion};
use egui_sdl2_gl as egui_backend;
use serde::Deserialize;
use std::{
    fs::File,
    io::{Cursor, Read, Write},
    path::Path,
    sync::{Arc, Mutex},
    thread,
    time::Instant,
};

// Define GitHub API response structures
#[derive(Deserialize, Clone, Debug)]
struct Asset {
    browser_download_url: String,
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
const GITHUB_API_URL: &str = "https://api.github.com/repos/LoveRetro/NextUI/releases/latest";
const USER_AGENT: &str = "NextUI Updater";
const OUTPUT_PATH: &str = "/mnt/SDCARD/MinUI.zip";
const WINDOW_WIDTH: u32 = 1024;
const WINDOW_HEIGHT: u32 = 768;
const DPI_SCALE: f32 = 3.0;

// Error type for the application
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn do_update(app_state: Arc<Mutex<AppState>>) {
    thread::spawn(move || {
        if let Err(err) = update_process(app_state.clone()) {
            let mut state = app_state.lock().unwrap();
            state.current_operation = None;
            state.error = Some(format!("Update failed: {}", err));
        }
    });
}

fn update_process(app_state: Arc<Mutex<AppState>>) -> Result<()> {
    // Fetch latest release information
    {
        let mut state = app_state.lock().unwrap();
        state.current_operation = Some("Fetching latest release...".to_string());
        state.progress = Some(0.1);
    }

    let client = reqwest::blocking::Client::new();
    let response = client
        .get(GITHUB_API_URL)
        .header("User-Agent", USER_AGENT)
        .send()?;

    if !response.status().is_success() {
        return Err(format!("GitHub API request failed: {}", response.status()).into());
    }

    let release: Release = response.json()?;

    {
        let mut state = app_state.lock().unwrap();
        state.latest_release = Some(release.clone());
        state.current_operation = Some("Downloading update...".to_string());
        state.progress = Some(0.3);
    }

    // Download the release
    let asset = release.assets.first().ok_or("No assets found in release")?;

    let response = reqwest::blocking::get(&asset.browser_download_url)?;
    let bytes = response.bytes()?;

    {
        let mut state = app_state.lock().unwrap();
        state.current_operation = Some("Extracting files...".to_string());
        state.progress = Some(0.6);
    }

    // Extract the update package
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;

    // Look for MinUI.zip in the archive
    let mut minui_data = Vec::new();
    match archive.by_name("MinUI.zip") {
        Ok(mut file) => {
            file.read_to_end(&mut minui_data)?;
        }
        Err(_) => return Err("File MinUI.zip not found in archive".into()),
    }

    // Create the output directory if it doesn't exist
    if let Some(parent) = Path::new(OUTPUT_PATH).parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }

    // Write the extracted file
    let mut file = File::create(OUTPUT_PATH)?;
    file.write_all(&minui_data)?;

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
        .window("NextUI Updater", WINDOW_WIDTH, WINDOW_HEIGHT)
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

    let start_time = Instant::now();
    let mut quit = false;

    'running: loop {
        egui_state.input.time = Some(start_time.elapsed().as_secs_f64());
        egui_ctx.begin_pass(egui_state.input.take());

        // UI rendering
        egui::CentralPanel::default().show(&egui_ctx, |ui| {
            ui.vertical_centered_justified(|ui| {
                ui.heading("NextUI Updater");
                ui.add_space(32.0);

                // Check application state
                let state_lock = app_state.lock().unwrap();
                let update_in_progress = state_lock.current_operation.is_some();
                drop(state_lock);

                // Quit button
                if ui.button("Quit").clicked() {
                    quit = true;
                }
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                // Update button
                let update_button =
                    ui.add_enabled(!update_in_progress, Button::new("Update NextUI"));

                ui.add_space(16.0);

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

                // Show release information if available
                if let Some(release) = &app_state.lock().unwrap().latest_release {
                    ui.add_space(16.0);
                    ui.label(format!("Latest version: {}", release.tag_name));
                }

                // Initiate update if button clicked
                if update_button.clicked() {
                    // Clear any previous errors
                    app_state.lock().unwrap().error = None;
                    do_update(app_state.clone());
                }

                // Focus the update button for controller navigation
                ui.memory_mut(|r| {
                    if r.focused().is_none() {
                        r.request_focus(update_button.id);
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
