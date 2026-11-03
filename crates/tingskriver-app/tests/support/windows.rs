//! VisualTestAppContext is macOS-only in GPUI 0.3.10. Drive the native
//! Windows application and read D3D textures without presenting the windows.
use anyhow::{bail, Context, Result};
use gpui::{point, prelude::*, px, size, Bounds, QuitMode, WindowBounds, WindowOptions};
use gpui_component::{Root, ThemeMode};
use std::{cell::RefCell, collections::HashMap, path::PathBuf, rc::Rc};
use tingskriver_app::{assets, theme, ui::Shell};

pub fn run() -> Result<()> {
    if std::env::args().skip(1).collect::<Vec<_>>() != ["--capture"] {
        bail!("Windows uses native capture only. Run with --capture; Linux owns goldens.");
    }
    let result = Rc::new(RefCell::new(None));
    let captured = result.clone();
    gpui_platform::application()
        .with_assets(assets::Assets)
        .with_quit_mode(QuitMode::Explicit)
        .run(move |cx| {
            *captured.borrow_mut() = Some(capture(cx));
            cx.quit();
        });
    let result = result
        .borrow_mut()
        .take()
        .context("Windows application did not launch")?;
    result
}

fn capture(cx: &mut gpui::App) -> Result<()> {
    let output = std::env::var_os("TINGSKRIVER_SCREENSHOT_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/screenshots/windows")
        });
    std::fs::create_dir_all(&output)?;
    let runtime = tokio::runtime::Runtime::new()?;
    gpui_component::init(cx);
    assets::register_fonts(cx);
    let mut light_images: HashMap<String, image::RgbaImage> = HashMap::new();
    for (appearance, mode) in [("light", ThemeMode::Light), ("dark", ThemeMode::Dark)] {
        for (scenario, page, db) in crate::fixtures() {
            let handle = cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds {
                        origin: point(px(-10000.0), px(-10000.0)),
                        size: size(px(1100.0), px(760.0)),
                    })),
                    show: false,
                    focus: false,
                    ..Default::default()
                },
                |window, cx| {
                    let shell = Shell::fixture(page, db, runtime.handle().clone(), window, cx);
                    cx.new(|cx| Root::new(shell, window, cx))
                },
            )?;
            theme::apply(mode, cx);
            let actual = handle.update(cx, |_, window, cx| {
                for _ in 0..2 {
                    window.draw(cx).clear(cx);
                }
                window.render_to_image()
            })??;
            let name = format!("{scenario}-{appearance}.png");
            actual.save(output.join(&name))?;
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
            handle.update(cx, |_, window, _| window.remove_window())?;
        }
    }
    std::fs::write(
        output.join("result.txt"),
        "Capture smoke test passed (no golden comparison)",
    )?;
    Ok(())
}
