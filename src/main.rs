use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use gstreamer as gst;
use gstreamer::prelude::*;
use gtk::prelude::*;
use gtk::{Application, ApplicationWindow, Box as GtkBox, Button, Label, Orientation};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "aac", "flac", "ogg", "opus", "wav", "m4a"];

#[derive(Parser, Debug)]
#[command(author, version, about = "Radio FM starter CLI + GUI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Scan {
        folder: PathBuf,
        #[arg(long)]
        json: bool,
    },
    Play {
        file: PathBuf,
    },
    Gui,
}

#[derive(Serialize)]
struct ScanResult {
    folder: String,
    files: Vec<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Scan { folder, json } => run_scan(&folder, json),
        Commands::Play { file } => run_play(&file),
        Commands::Gui => {
            run_gui();
            Ok(())
        }
    }
}

fn run_scan(folder: &Path, json: bool) -> Result<()> {
    if !folder.exists() {
        bail!("Folder does not exist: {}", folder.display());
    }
    if !folder.is_dir() {
        bail!("Path is not a directory: {}", folder.display());
    }

    let mut files = Vec::new();
    collect_media_files(folder, &mut files)?;
    files.sort();

    if json {
        let result = ScanResult {
            folder: folder
                .canonicalize()
                .context("Failed to canonicalize folder path")?
                .display()
                .to_string(),
            files: files
                .into_iter()
                .map(|path| path.display().to_string())
                .collect(),
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&result).context("Failed to serialize JSON output")?
        );
    } else {
        for path in files {
            println!("{}", path.display());
        }
    }

    Ok(())
}

fn collect_media_files(dir: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    let entries =
        fs::read_dir(dir).with_context(|| format!("Failed reading directory {}", dir.display()))?;

    for entry in entries {
        let entry =
            entry.with_context(|| format!("Failed reading an entry in {}", dir.display()))?;
        let path = entry.path();

        if path.is_dir() {
            collect_media_files(&path, output)?;
            continue;
        }

        if is_supported_media_file(&path) {
            output.push(path);
        }
    }

    Ok(())
}

fn is_supported_media_file(path: &Path) -> bool {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };

    let extension_lower = extension.to_ascii_lowercase();
    SUPPORTED_EXTENSIONS.contains(&extension_lower.as_str())
}

fn run_play(file: &Path) -> Result<()> {
    if !file.exists() {
        bail!("File does not exist: {}", file.display());
    }
    if !file.is_file() {
        bail!("Path is not a file: {}", file.display());
    }

    gst::init().context("Failed to initialize GStreamer")?;

    let playbin = gst::ElementFactory::make("playbin")
        .build()
        .context("Could not create GStreamer playbin element")?;

    let absolute = file
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize file {}", file.display()))?;
    let uri = gst::glib::filename_to_uri(absolute, None)
        .context("Failed to convert file path into URI")?;
    playbin.set_property("uri", &uri);

    playbin
        .set_state(gst::State::Playing)
        .context("Failed to set playback state to Playing")?;
    println!("Playing {}", file.display());

    let bus = playbin.bus().context("Pipeline has no message bus")?;
    for message in bus.iter_timed(gst::ClockTime::NONE) {
        use gst::MessageView;
        match message.view() {
            MessageView::Eos(..) => break,
            MessageView::Error(err) => {
                let src = err
                    .src()
                    .map(|s| s.path_string().to_string())
                    .unwrap_or_else(|| "unknown".to_string());
                bail!("Playback error from {src}: {}", err.error());
            }
            _ => {}
        }
    }

    playbin
        .set_state(gst::State::Null)
        .context("Failed to stop GStreamer pipeline")?;
    Ok(())
}

fn run_gui() {
    let app = Application::builder()
        .application_id("dev.radiofm.scheduler")
        .build();

    app.connect_activate(|app| {
        let label = Label::new(Some("Radio FM Scheduler starter is running"));
        let button = Button::with_label("Close");
        let app_weak = app.downgrade();
        button.connect_clicked(move |_| {
            if let Some(app) = app_weak.upgrade() {
                app.quit();
            }
        });

        let container = GtkBox::builder()
            .orientation(Orientation::Vertical)
            .spacing(12)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(24)
            .margin_end(24)
            .build();
        container.append(&label);
        container.append(&button);

        let window = ApplicationWindow::builder()
            .application(app)
            .title("Radio FM Scheduler")
            .default_width(520)
            .default_height(220)
            .child(&container)
            .build();

        window.present();
    });

    app.run();
}
