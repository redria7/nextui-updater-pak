use std::io::{Read, Write};

use bytes::Bytes;
use reqwest::IntoUrl;

use crate::github::{Release, Tag};

use crate::Result;

const USER_AGENT: &str = "NextUI Updater";

pub fn fetch_latest_release(repo: &str) -> Result<Release> {
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

pub fn fetch_tag(repo: &str, tag: &str) -> Result<Tag> {
    let client = reqwest::blocking::Client::new();
    let response = client
        .get(format!("https://api.github.com/repos/{repo}/tags"))
        .header("User-Agent", USER_AGENT)
        .send()?;

    if !response.status().is_success() {
        return Err(format!("GitHub API request failed: {}", response.status()).into());
    }

    let tags: Vec<Tag> = response.json()?;

    let tag = tags.iter().find(|t| t.name == tag).ok_or("Tag not found")?;

    Ok(tag.clone())
}

pub fn download<U: IntoUrl>(url: U, progress_cb: impl Fn(f32)) -> Result<Bytes> {
    let client = reqwest::blocking::Client::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .timeout(None)
        .build()?;
    let request_builder = client
        .get(url)
        .header("Accept", "application/octet-stream")
        .header("User-Agent", USER_AGENT);

    let mut response = request_builder.send()?;
    let total_size = response.content_length().unwrap_or(0);

    let mut bytes = Vec::new();
    let mut downloaded: u64 = 0;
    let mut buffer = [0; 16384];

    loop {
        let bytes_read = response.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        bytes.write_all(&buffer[..bytes_read])?;
        downloaded += bytes_read as u64;

        // Show progress
        if total_size > 0 {
            let percentage = downloaded as f64 / total_size as f64;
            progress_cb(percentage as f32);
        }
    }

    println!("\nDownload complete!");
    println!("Status: {}", response.status());
    println!("Headers: {:?}", response.headers());

    Ok(bytes.into())
}
