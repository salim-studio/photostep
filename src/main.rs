//! PhotoStep — fast layer-based image editor in Rust.
//! Desktop GUI (default): `photostep` — native window via eframe.
//! Web: compiled to WebAssembly and served from `index.html` (see Trunk.toml).
//! Headless: `photostep --input in.png --out out.png --op "brightness:20" --op "blur:4"`

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console on Windows release

mod app;
mod core;
mod io;
mod ops;

#[cfg(not(target_arch = "wasm32"))]
use clap::{Parser, Subcommand};

#[cfg(not(target_arch = "wasm32"))]
#[derive(Parser, Debug)]
#[command(name = "photostep", version, about = "PhotoStep — fast layer-based image editor in Rust")]
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

#[cfg(not(target_arch = "wasm32"))]
#[derive(Subcommand, Debug)]
enum Cmd {
    /// Launch the desktop GUI
    Gui,
}

#[cfg(not(target_arch = "wasm32"))]
fn apply_op(doc: &mut core::Document, spec: &str) -> anyhow::Result<()> {
    let (name, arg) = match spec.split_once(':') {
        Some((a, b)) => (a.trim().to_lowercase(), b.trim().to_string()),
        None => (spec.trim().to_lowercase(), String::new()),
    };
    let num = |d: &str| d.parse::<f32>().unwrap_or(0.0);
    let csv = |d: &str| -> Vec<f32> {
        d.split(',').map(|x| x.trim().parse::<f32>().unwrap_or(0.0)).collect()
    };
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
        "levels" => {
            let v = csv(&arg);
            let lo = (*v.first().unwrap_or(&0.0)).clamp(0.0, 255.0) as u8;
            let hi = (*v.get(1).unwrap_or(&255.0)).clamp(0.0, 255.0) as u8;
            ops::levels(doc, lo, hi, *v.get(2).unwrap_or(&1.0));
        }
        "curves" => ops::curves(doc, &[(0, 0), (64, 56), (192, 200), (255, 255)]),
        "colorbal" => {
            let v = csv(&arg);
            ops::color_balance(
                doc,
                *v.first().unwrap_or(&0.0) as i16,
                *v.get(1).unwrap_or(&0.0) as i16,
                *v.get(2).unwrap_or(&0.0) as i16,
            );
        }
        "blackwhite" | "bw" => ops::black_white(doc, 0.299, 0.587, 0.114),
        "photofilter" => ops::photo_filter(doc, [255, 128, 0], if arg.is_empty() { 0.25 } else { num(&arg) }),
        "gradmap" => ops::gradient_map(doc, [0, 0, 0], [255, 255, 255]),
        "shadowhi" => {
            let v = csv(&arg);
            ops::shadows_highlights(doc, *v.first().unwrap_or(&0.0), *v.get(1).unwrap_or(&0.0));
        }
        "motion" => {
            let v = csv(&arg);
            ops::motion_blur(doc, *v.first().unwrap_or(&25.0), (*v.get(1).unwrap_or(&12.0)) as u32);
        }
        "radial" => ops::radial_blur(doc, if arg.is_empty() { 30.0 } else { num(&arg) }),
        "median" => ops::median(doc, num(&arg) as u32),
        "highpass" => ops::high_pass(doc, num(&arg) as u32),
        "rotate" => ops::rotate_arbitrary(doc, num(&arg)),
        "scale" => {
            let v = csv(&arg);
            ops::scale_content(doc, *v.first().unwrap_or(&100.0), *v.get(1).unwrap_or(&100.0));
        }
        "fliph" => ops::flip_horizontal(doc),
        "flipv" => ops::flip_vertical(doc),
        "rot90" => ops::rotate90_cw(doc),
        _ => anyhow::bail!("unknown op: {spec}"),
    }
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
fn run_headless(cli: &Cli) -> anyhow::Result<()> {
    let inp = cli.input.clone().expect("need --input");
    let out = cli.out.clone().expect("need --out");
    let lower = inp.to_lowercase();
    let mut doc = if lower.ends_with(".pstep") || lower.ends_with(".json") {
        io::load_project(&inp)?
    } else if lower.ends_with(".psd") {
        io::load_psd(&inp)?
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

// When compiling natively (desktop GUI + CLI):
#[cfg(not(target_arch = "wasm32"))]
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
        "PhotoStep",
        opts,
        Box::new(|cc| Ok(Box::new(app::PhotoStepApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("gui error: {e}"))?;
    Ok(())
}

// When compiling to web (WebAssembly via trunk):
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    // Redirect `log` messages to `console.log` and friends:
    eframe::WebLogger::init(log::LevelFilter::Debug).ok();

    let web_options = eframe::WebOptions::default();

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("No window")
            .document()
            .expect("No document");

        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("Failed to find the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id was not a HtmlCanvasElement");

        let start_result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(|cc| Ok(Box::new(app::PhotoStepApp::new(cc)))),
            )
            .await;

        // Remove the loading text and spinner:
        if let Some(loading_text) = document.get_element_by_id("loading_text") {
            match start_result {
                Ok(()) => {
                    loading_text.remove();
                }
                Err(err) => {
                    loading_text.set_inner_html(
                        "<p> The app has crashed. See the developer console for details. </p>",
                    );
                    panic!("Failed to start eframe: {err:?}");
                }
            }
        }
    });
}
