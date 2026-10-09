//! PhotoStep — Photoshop-like editor in Rust: fast + practical.
//! GUI (default): `photostep` or `photostep gui`
//! Headless: `photostep --input in.png --out out.png --op "brightness:20" --op "blur:4"`

mod app;
mod core;
mod io;
mod ops;

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(name = "photostep", version, about = "PhotoStep — fast Photoshop-like editor in Rust")]
struct Cli {
    /// input image (if omitted → launch GUI)
    #[arg(long)]
    input: Option<String>,
    /// output image
    #[arg(long)]
    out: Option<String>,
    /// ops like brightness:20 contrast:30 invert grayscale blur:5 sharpen:1.2 ...
    #[arg(long = "op")]
    ops_list: Vec<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Launch the desktop GUI
    Gui,
}

fn apply_op(doc: &mut core::Document, spec: &str) -> anyhow::Result<()> {
    let (name, arg) = match spec.split_once(':') {
        Some((a, b)) => (a.trim().to_lowercase(), b.trim().to_string()),
        None => (spec.trim().to_lowercase(), String::new()),
    };
    let num = |d: &str| d.parse::<f32>().unwrap_or(0.0);
    match name.as_str() {
        "brightness" => ops::brightness(doc, num(&arg) as i16),
        "contrast" => ops::contrast(doc, num(&arg)),
        "invert" => ops::invert(doc),
        "grayscale" | "gray" => ops::grayscale(doc),
        "threshold" => ops::threshold(doc, num(&arg) as u8),
        "posterize" => ops::posterize(doc, num(&arg) as u8),
        "exposure" => ops::exposure(doc, num(&arg)),
        "vibrance" => ops::vibrance(doc, num(&arg)),
        "saturate" => ops::hue_saturation(doc, 0.0, num(&arg)),
        "hue" => ops::hue_saturation(doc, num(&arg), 1.0),
        "blur" | "gaussian" => ops::gaussian_blur(doc, num(&arg) as u32),
        "sharpen" => ops::sharpen(doc, if arg.is_empty() { 1.2 } else { num(&arg) }),
        "edge" => ops::edge_detect(doc),
        "emboss" => ops::emboss(doc),
        "pixelate" => ops::pixelate(doc, num(&arg) as u32),
        "noise" => ops::add_noise(doc, num(&arg) as u8),
        "vignette" => ops::vignette(doc, if arg.is_empty() { 0.6 } else { num(&arg) }),
        "autocontrast" => ops::auto_contrast(doc),
        "fliph" => ops::flip_horizontal(doc),
        "flipv" => ops::flip_vertical(doc),
        "rot90" => ops::rotate90_cw(doc),
        _ => anyhow::bail!("unknown op: {spec}"),
    }
    Ok(())
}

fn run_headless(cli: &Cli) -> anyhow::Result<()> {
    let inp = cli.input.clone().expect("need --input");
    let out = cli.out.clone().expect("need --out");
    let mut doc = if inp.ends_with(".pstep") || inp.ends_with(".json") {
        io::load_project(&inp)?
    } else {
        io::load_image(&inp)?
    };
    for op in &cli.ops_list {
        apply_op(&mut doc, op)?;
    }
    if out.ends_with(".pstep") {
        io::save_project(&doc, &out)?;
    } else {
        io::save_image(&doc, &out)?;
    }
    println!("photostep: {} -> {} ({} ops)", inp, out, cli.ops_list.len());
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let headless = cli.input.is_some() || cli.out.is_some();
    if headless && !matches!(cli.cmd, Some(Cmd::Gui)) {
        return run_headless(&cli);
    }
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1280.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "PhotoStep — fast Photoshop in Rust",
        opts,
        Box::new(|cc| Ok(Box::new(app::PhotoStepApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("gui error: {e}"))?;
    Ok(())
}
