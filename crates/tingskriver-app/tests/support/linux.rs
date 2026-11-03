//! Real GPUI scene rasterization. No desktop capture or production services.
use crate::{fixtures, pixel_compare};

use anyhow::{bail, Context, Result};
use gpui::{prelude::*, px, size};
use gpui_component::{Root, ThemeMode};
use std::{path::PathBuf, sync::Arc};
use tingskriver_app::{assets, theme, ui::Shell};

type ContextType = gpui::HeadlessAppContext;

fn context() -> ContextType {
    gpui::HeadlessAppContext::with_platform(
        gpui_platform::current_platform(true).text_system(),
        Arc::new(assets::Assets),
        gpui_platform::current_headless_renderer,
    )
}

pub fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let update = args == ["--update"];
    let capture = args == ["--capture"];
    if !args.is_empty() && !update && !capture {
        bail!("Usage: screenshots [--update|--capture]");
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let golden = root.join("tests/golden").join(std::env::consts::OS);
    let output = std::env::var_os("TINGSKRIVER_SCREENSHOT_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            root.join("../../target/screenshots")
                .join(std::env::consts::OS)
        });
    std::fs::create_dir_all(&output)?;
    if update {
        std::fs::create_dir_all(&golden)?;
    }
    let runtime = tokio::runtime::Runtime::new()?;
    let mut failures = Vec::new();
    let mut light_images = std::collections::HashMap::new();
    // Page::ALL is also the navigation source, so new pages cannot silently
    // escape the golden inventory.
    for (appearance, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
        for (scenario, page, db) in fixtures() {
            let mut cx = context();
            cx.update(|cx| {
                gpui_component::init(cx);
                assets::register_fonts(cx);
                theme::apply(mode, cx);
            });
            let build = |window: &mut gpui::Window, cx: &mut gpui::App| {
                let shell = Shell::fixture(page, db, runtime.handle().clone(), window, cx);
                cx.new(|cx| Root::new(shell, window, cx))
            };
            let window = cx.open_window(size(px(1100.0), px(760.0)), build)?;
            cx.run_until_parked();
            // Root initializes its window theme on construction; pin the
            // fixture appearance afterwards, independently of the desktop.
            cx.update(|cx| theme::apply(mode, cx));
            // Draw twice: first layout resolves lazy component state/assets.
            for _ in 0..2 {
                cx.update(|cx| crate::native::draw_frame(window.into(), cx))?;
                cx.run_until_parked();
            }
            let (actual, expected) =
                cx.update(|cx| crate::native::capture_frame(window.into(), cx))?;
            let name = format!("{scenario}-{appearance}.png");
            actual.save(output.join(&name))?;
            if !pixel_compare::valid_viewport(actual.dimensions(), expected) {
                bail!(
                    "{name}: expected full viewport {expected:?}, got {:?}",
                    actual.dimensions()
                );
            }
            if appearance == "light" {
                light_images.insert(scenario.clone(), actual.clone());
            } else {
                let light = &light_images[&scenario];
                if light.dimensions() != actual.dimensions()
                    || pixel_compare::changed_pixels(light.as_raw(), actual.as_raw(), 2)
                        < actual.width() as usize * actual.height() as usize / 2
                {
                    bail!("{scenario}: light and dark must render visibly different themes");
                }
            }
            if update {
                actual.save(golden.join(&name))?;
            } else if !capture {
                let result = (|| -> Result<()> {
                    let expected = image::open(golden.join(&name))
                        .with_context(|| format!("Missing golden {name}; update explicitly"))?
                        .to_rgba8();
                    if expected.dimensions() != actual.dimensions() {
                        expected.save(output.join(format!("expected-{name}")))?;
                        bail!(
                            "Image dimensions differ: {:?} versus {:?}",
                            expected.dimensions(),
                            actual.dimensions()
                        );
                    }
                    let changed =
                        pixel_compare::changed_pixels(expected.as_raw(), actual.as_raw(), 2);
                    // Up to 0.01% of pixels may differ beyond two channel levels.
                    if changed > actual.width() as usize * actual.height() as usize / 10_000 {
                        expected.save(output.join(format!("expected-{name}")))?;
                        let diff =
                            image::RgbaImage::from_fn(actual.width(), actual.height(), |x, y| {
                                if expected
                                    .get_pixel(x, y)
                                    .0
                                    .iter()
                                    .zip(actual.get_pixel(x, y).0.iter())
                                    .any(|(a, b)| a.abs_diff(*b) > 2)
                                {
                                    image::Rgba([255, 0, 100, 255])
                                } else {
                                    image::Rgba([0, 0, 0, 255])
                                }
                            });
                        diff.save(output.join(format!("diff-{name}")))?;
                        bail!("{changed} changed pixels");
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    failures.push(format!("{name}: {error:#}"));
                }
            }
            println!("Rendered {name}");
            cx.update_window(window.into(), |_, window, _| window.remove_window())?;
        }
    }
    std::fs::write(
        output.join("result.txt"),
        if failures.is_empty() {
            if capture {
                "Capture smoke test passed (no golden comparison)".into()
            } else if update {
                "Reference images updated (no golden comparison)".into()
            } else {
                "All golden images matched".into()
            }
        } else {
            failures.join("\n")
        },
    )?;
    if !failures.is_empty() {
        bail!("{}", failures.join("\n"));
    }
    Ok(())
}
