use crate::{
    app_state::{AppStateManager, Progress},
    Result, SDCARD_ROOT,
};
use bytes::Bytes;
use fetching::{download, fetch_latest_release, fetch_tag};
use regex::Regex;

use std::{
    fs::File,
    io::{Cursor, Read, Write},
    path::PathBuf,
    process::exit,
    thread,
};

mod fetching;

fn extract_zip<T: Fn(&str) -> bool>(
    bytes: Bytes,
    filter: T,
    progress_cb: impl Fn(f32),
) -> Result<()> {
    pub fn file_write_all_bytes(path: &PathBuf, bytes: &[u8]) -> Result<usize> {
        let mut file = File::create(path)?;
        file.set_len(0)?;
        Ok(file.write(bytes)?)
    }

    // Extract the update package
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    let target_directory = PathBuf::from(SDCARD_ROOT);
    let archive_len = archive.len();

    for file_number in 0..archive_len {
        let mut next = archive.by_index(file_number)?;

        let sanitized_name = next.mangled_name();

        if !filter(sanitized_name.as_os_str().to_string_lossy().as_ref()) {
            println!("Skipping file: {sanitized_name:#?}");
            continue;
        }

        if next.is_dir() {
            let extracted_folder_path = target_directory.join(sanitized_name);
            std::fs::create_dir_all(&extracted_folder_path)?;
            println!("Created directory: {}", extracted_folder_path.display());
        } else if next.is_file() {
            let mut buffer: Vec<u8> = Vec::new();
            let _bytes_read = next.read_to_end(&mut buffer)?;
            let extracted_file_path = target_directory.join(sanitized_name);
            file_write_all_bytes(&extracted_file_path, buffer.as_ref())?;
            println!("Extracted file: {}", extracted_file_path.display());
        }

        progress_cb(file_number as f32 / (archive_len - 1) as f32);
    }

    Ok(())
}

pub fn self_update(app_state: &AppStateManager) -> Result<()> {
    // Fetch latest release information
    app_state.start_operation("Fetching latest updater release...");

    println!("Fetching latest updater release...");

    let release = fetch_latest_release("LanderN/nextui-updater-pak")?;

    println!("Latest updater release: {release:?}");

    let available = semver::Version::parse(&release.tag_name)?;
    let installed = semver::Version::parse(env!("CARGO_PKG_VERSION"))?;

    if available > installed {
        println!("New version available: {available} (current: {installed})");
        app_state.set_current_operation(Some("Downloading updater...".to_string()));
    } else {
        println!("No updates available");
        return Ok(());
    }

    let bytes = download(&release.assets[0].url, |pr| {
        app_state.update_progress(pr);
    })?;

    app_state
        .set_current_operation(format!("Extracting NextUI Updater {}...", release.tag_name).into());
    app_state.set_progress(Some(Progress::Indeterminate));

    // Move the current binary to a backup location
    let current_binary = std::env::current_exe()?;
    std::fs::rename(&current_binary, current_binary.with_extension("bak"))?;

    // Extract the update package
    let result = extract_zip(
        bytes,
        |_| true,
        |pr| {
            app_state.update_progress(pr);
        },
    );

    println!("Extraction complete!");
    app_state.set_progress(Some(Progress::Indeterminate));

    if result.is_err() {
        // Move the backup back
        std::fs::rename(current_binary.with_extension("bak"), current_binary)?;

        return Err("Failed to extract update package".into());
    }

    app_state.set_current_operation(Some(
        "Self-update success! Restarting updater...".to_string(),
    ));

    // Give the user a moment to see the completion message
    thread::sleep(std::time::Duration::from_secs(1));

    // "5" is the exit code for "restart required"
    exit(5);
}

pub fn do_nextui_release_check(app_state: &AppStateManager) {
    // Fetch latest release information
    app_state.start_operation("Fetching latest NextUI release...");

    let latest_release = fetch_latest_release("LoveRetro/NextUI");

    match &latest_release {
        Ok(release) => {
            app_state.set_nextui_release(Some(release.clone()));
        }
        Err(err) => {
            println!("Release fetch failed: {:?}", err.source());
            app_state.set_operation_failed(&format!("Release fetch failed: {err}"));
        }
    }

    if latest_release.is_err() {
        return;
    }
    let latest_release = latest_release.unwrap();

    // Fetch latest tag information
    app_state.start_operation("Fetching latest NextUI tag...");

    let latest_tag = fetch_tag("LoveRetro/NextUI", &latest_release.tag_name);
    match latest_tag {
        Ok(tag) => {
            app_state.set_nextui_tag(Some(tag.clone()));
        }
        Err(err) => {
            println!("Tag fetch failed: {:?}", err.source());
            app_state.set_operation_failed(&format!("Tag fetch failed: {err}"));
        }
    }

    app_state.finish_operation();
}

pub fn do_self_update(app_state: &AppStateManager) {
    // Do self-update
    let result = self_update(app_state);
    match result {
        Ok(()) => {
            app_state.finish_operation();
        }
        Err(err) => {
            println!("Self-update failed: {:?}", err.source());
            app_state.set_operation_failed(&format!("Self-update failed: {err}"));
        }
    }
}

pub fn do_update(app_state: &'static AppStateManager, full: bool) {
    thread::spawn(move || {
        if let Err(err) = update_nextui(app_state, full) {
            println!("Update failed: {:?}", err.source());

            app_state.set_operation_failed(&format!("Update failed: {err}"));

            // Try to fetch latest release information again
            do_nextui_release_check(app_state);
        }
    });
}

pub fn update_nextui(app_state: &AppStateManager, full: bool) -> Result<()> {
    let release = {
        app_state.start_operation("Downloading update...");

        app_state
            .nextui_release()
            .clone()
            .ok_or("No release found")?
    };

    let assets = release.assets;
    let asset = assets
        .iter()
        .find(|a| a.name.contains(if full { "all" } else { "base" }))
        .or(assets.first())
        .ok_or("No assets found")?;

    // Download the asset
    app_state.start_determinate_operation(&format!("Downloading {}...", asset.name));
    println!("Downloading from {}", asset.url);

    let bytes = download(&asset.url, |pr| app_state.update_progress(pr))?;

    app_state.set_current_operation(format!("Extracting {}...\nPlease wait...", asset.name).into());
    app_state.set_progress(Some(Progress::Indeterminate));

    // Extract the update package
    if full {
        let emu_tag_re = Regex::new(r"\((?<emu>\w+)\)").expect("Failed to compile regex");
        // Full update, extract all files, except for Roms folders which already exist
        extract_zip(
            bytes,
            |file| {
                if file.starts_with("Roms/") {
                    // Extract the emu tag from the folder name
                    if let Some(captures) = emu_tag_re.captures(file) {
                        if let Some(emu) = captures.name("emu").map(|c| c.as_str()) {
                            // Check if the emu tag already exists in the roms folder
                            if std::fs::read_dir(PathBuf::from(SDCARD_ROOT).join("Roms"))
                                .map(|d| {
                                    d.filter_map(std::result::Result::ok).any(|e| {
                                        e.file_name()
                                            .to_string_lossy()
                                            .contains(format!("({emu})").as_str())
                                    })
                                })
                                .unwrap_or(false)
                            {
                                println!("Roms folder for {emu} already exists, skipping");
                                return false;
                            }
                        }
                    }
                }

                true
            },
            |pr| app_state.update_progress(pr),
        )?;
    } else {
        // "Quick" update, just extract MinUI.zip and trimui folder
        extract_zip(
            bytes,
            |file| {
                ["MinUI.zip", "trimui"]
                    .iter()
                    .any(|prefix| file.starts_with(prefix))
            },
            |pr| app_state.update_progress(pr),
        )?;
    }

    println!("Extraction complete!");
    app_state.set_progress(Some(Progress::Indeterminate));

    app_state.set_current_operation(Some("Update complete, preparing to reboot...".to_string()));

    // Give the user a moment to see the completion message
    thread::sleep(std::time::Duration::from_secs(2));

    app_state.set_current_operation(Some("Rebooting system...".to_string()));

    // Reboot the system
    match std::process::Command::new("reboot").output() {
        Ok(_) => Ok(()),
        Err(e) => Err(Box::new(e)),
    }
}
