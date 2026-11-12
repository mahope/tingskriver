//! VisualTestAppContext is macOS-only in GPUI 0.3.10. Drive the native
//! Windows application and read D3D textures without presenting the windows.
use anyhow::{bail, Result};
use gpui::{point, prelude::*, px, size, Bounds, QuitMode, WindowBounds, WindowOptions};
use gpui_component::{Root, ThemeMode};
use std::{collections::HashMap, path::PathBuf};
use tingskriver_app::{assets, theme, ui::Shell};

pub fn draw_frame(handle: gpui::AnyWindowHandle, cx: &mut gpui::App) -> Result<()> {
    // Drawing borrows Root internally; a typed WindowHandle::update would
    // already hold that entity and panic on the nested borrow.
    cx.update_window(handle, |_, window, cx| window.draw(cx).clear(cx))
}

pub struct CapturedFrame {
    pub image: image::RgbaImage,
    pub expected: (u32, u32),
    pub minimum: (u32, u32),
}

pub fn capture_frame(handle: gpui::AnyWindowHandle, cx: &mut gpui::App) -> Result<CapturedFrame> {
    cx.update_window(handle, |_, window, _| {
        let viewport = window.viewport_size();
        let scale = window.scale_factor();
        let expected = (
            (f32::from(viewport.width) * scale).round() as u32,
            (f32::from(viewport.height) * scale).round() as u32,
        );
        window.render_to_image().map(|image| CapturedFrame {
            image,
            expected,
            minimum: crate::minimum_viewport(scale),
        })
    })?
}

pub fn run() -> Result<()> {
    if std::env::args().skip(1).collect::<Vec<_>>() != ["--capture"] {
        bail!("Windows uses native capture only. Run with --capture; Linux owns goldens.");
    }
    // A blocked GPU readback or native event loop must fail the process,
    // rather than leaving agents waiting for the outer CI job timeout.
    let _watchdog = watchdog(std::time::Duration::from_secs(180));
    println!("Launching native GPUI application");
    // The snapshot executable owns these workers and exits after validation.
    let runtime = tokio::runtime::Runtime::new()?;
    let runtime_handle = runtime.handle().clone();
    gpui_platform::application()
        .with_assets(assets::Assets)
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx| {
            // Native resize is queued on this executor. Capture must yield
            // to the event loop instead of reading initial 1×1 targets.
            cx.spawn(async move |cx| {
                let outcome = capture(cx, runtime_handle).await;
                match outcome {
                    Ok(()) => {
                        println!("All snapshot assertions passed; exiting capture process");
                        std::process::exit(0);
                    }
                    Err(error) => {
                        eprintln!("Native snapshot failed: {error:#}");
                        std::process::exit(1);
                    }
                }
            })
            .detach();
        });
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    bail!("Native event loop returned without completing snapshot validation")
}

fn watchdog(timeout: std::time::Duration) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        std::thread::sleep(timeout);
        eprintln!("Native capture timed out; inspect the last stage above");
        std::process::exit(1);
    })
}

pub fn watchdog_self_test() -> ! {
    let _watchdog = watchdog(std::time::Duration::from_millis(10));
    loop {
        std::thread::park();
    }
}

async fn capture(cx: &mut gpui::AsyncApp, runtime: tokio::runtime::Handle) -> Result<()> {
    let output = std::env::var_os("TINGSKRIVER_SCREENSHOT_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/screenshots/windows")
        });
    std::fs::create_dir_all(&output)?;
    cx.update(|cx| {
        gpui_component::init(cx);
        assets::register_fonts(cx);
    });
    let mut light_images: HashMap<String, image::RgbaImage> = HashMap::new();
    for (appearance, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
        for (scenario, page, db, variant) in crate::fixtures() {
            let name = format!("{scenario}-{appearance}.png");
            println!("Opening {name}");
            let handle = cx.update(|cx| {
                cx.open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds {
                            origin: point(px(-10000.0), px(-10000.0)),
                            size: size(px(1100.0), px(760.0)),
                        })),
                        show: true,
                        focus: false,
                        ..Default::default()
                    },
                    |window, cx| {
                        let shell = Shell::fixture(page, db, variant, runtime.clone(), window, cx);
                        cx.new(|cx| Root::new(shell, window, cx))
                    },
                )
            })?;
            cx.update(|cx| {
                cx.update_window(handle.into(), |_, window, _| {
                    window.resize(size(px(1100.0), px(760.0)));
                })
            })?;
            cx.background_executor()
                .timer(std::time::Duration::from_millis(100))
                .await;
            cx.update(|cx| theme::apply(mode, cx));
            println!("Drawing {name}");
            for _ in 0..2 {
                cx.update(|cx| draw_frame(handle.into(), cx))?;
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
            }
            println!("Reading pixels for {name}");
            let CapturedFrame {
                image: actual,
                expected,
                minimum,
            } = cx.update(|cx| capture_frame(handle.into(), cx))?;
            actual.save(output.join(&name))?;
            if !crate::pixel_compare::valid_viewport(actual.dimensions(), expected, minimum) {
                bail!(
                    "{name}: expected full viewport {expected:?}, got {:?}",
                    actual.dimensions()
                );
            }
            if appearance == "light" {
                light_images.insert(scenario.clone(), actual);
            } else {
                let light = &light_images[&scenario];
                if light.dimensions() != actual.dimensions()
                    || crate::pixel_compare::changed_pixels(light.as_raw(), actual.as_raw(), 2)
                        < actual.width() as usize * actual.height() as usize / 2
                {
                    bail!("{scenario}: light and dark must render visibly different themes");
                }
            }
            println!("Rendered {name}");
            cx.update(|cx| cx.update_window(handle.into(), |_, window, _| window.remove_window()))?;
            cx.background_executor()
                .timer(std::time::Duration::from_millis(16))
                .await;
        }
    }
    std::fs::write(
        output.join("result.txt"),
        "Capture smoke test passed (no golden comparison)",
    )?;
    println!("All native PNGs and result file saved");
    Ok(())
}
