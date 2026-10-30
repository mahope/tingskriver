//! See the pill without the rest of the app.
//!
//!   cargo run -p tingskriver-overlay --example demo            (Windows: the real window)
//!   cargo run -p tingskriver-overlay --example demo -- --png out  (any OS: PNG per state)
//!
//! On Windows the demo runs through every state for about 15 seconds. Click
//! into Notepad (or Word, or a browser field) first and keep typing: the
//! letters must keep landing there while the pill comes and goes.

use std::path::PathBuf;
use std::time::Instant;

use tingskriver_overlay::model::Pill;
use tingskriver_overlay::render::Renderer;
use tingskriver_overlay::PillState;

fn states() -> Vec<(&'static str, PillState)> {
    vec![
        (
            "lytter",
            PillState::Listening {
                level: 0.6,
                hands_free: false,
            },
        ),
        (
            "haandfri",
            PillState::Listening {
                level: 0.3,
                hands_free: true,
            },
        ),
        ("behandler", PillState::Processing),
        ("faerdig", PillState::Done),
        ("fejl", PillState::Error("Mikrofonen svarer ikke".into())),
        (
            "fejl-lang",
            PillState::Error("Modellen kunne ikke indlæses, fordi filen på disken er ufuldstændig eller beskadiget".into()),
        ),
        ("afbrudt", PillState::Cancelled),
    ]
}

fn write_pngs(dir: PathBuf) {
    std::fs::create_dir_all(&dir).expect("create output folder");
    let mut renderer = Renderer::new();
    for (name, state) in states() {
        let t0 = Instant::now();
        let mut pill = Pill::new(t0);
        pill.set_state(state, t0);
        // Let the bar history fill up.
        for i in 1..=10 {
            pill.tick(t0 + std::time::Duration::from_millis(50 * i));
        }
        let frame = pill
            .frame(t0 + std::time::Duration::from_millis(500))
            .expect("visible");
        for (suffix, scale) in [("100", 1.0), ("150", 1.5), ("200", 2.0)] {
            let (pixmap, _) = renderer.render(&frame, scale);
            let path = dir.join(format!("{name}-{suffix}.png"));
            pixmap.save_png(&path).expect("write png");
            println!("{}", path.display());
        }
    }
}

#[cfg(windows)]
fn run_window() {
    use std::time::Duration;
    use tingskriver_overlay::windows::Overlay;

    let overlay = Overlay::new().expect("overlay window");
    let sleep = |ms| std::thread::sleep(Duration::from_millis(ms));
    println!("Klik ind i Notesblok og skriv videre, mens pillen vises.");
    sleep(3000);

    println!("Lytter (lydniveau som en bølge)");
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(4) {
        let t = start.elapsed().as_secs_f32();
        let level = ((t * 5.0).sin() * 0.5 + 0.5) * ((t * 1.3).sin() * 0.3 + 0.7);
        overlay.set_state(PillState::Listening {
            level,
            hands_free: t > 2.0,
        });
        sleep(30);
    }
    println!("Behandler");
    overlay.set_state(PillState::Processing);
    sleep(2000);
    println!("Færdig");
    overlay.set_state(PillState::Done);
    sleep(1500);
    println!("Fejl");
    overlay.set_state(PillState::Error("Mikrofonen svarer ikke".into()));
    sleep(3500);
    println!("Afbrudt");
    overlay.set_state(PillState::Cancelled);
    sleep(1500);

    println!("Skjult i 2 sekunder: der må ikke tegnes nogen billeder");
    let frames = overlay.stats().frames;
    sleep(2000);
    let hidden_frames = overlay.stats().frames - frames;

    let stats = overlay.stats();
    println!();
    println!("Billeder tegnet i alt: {}", stats.frames);
    println!("Billeder tegnet mens pillen var skjult: {hidden_frames}");
    println!("Gange vist: {}", stats.shows);
    if let Some(max) = stats.max_show_latency {
        println!(
            "Længste tid fra tilstandsskift til synlig pille: {:.1} ms",
            max.as_secs_f64() * 1000.0
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--png") {
        let dir = args
            .get(i + 1)
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("pille-png"));
        write_pngs(dir);
        return;
    }
    #[cfg(windows)]
    run_window();
    #[cfg(not(windows))]
    eprintln!("Vinduet findes kun på Windows. Brug --png <mappe> for at se pillen som billeder.");
}
