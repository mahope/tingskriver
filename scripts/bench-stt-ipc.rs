//! Standalone optimized benchmark of the production PCM pipe codec.
#[path = "../crates/tingskriver-core/src/stt/ipc.rs"]
mod ipc;
fn main() {
    use std::io::{BufReader, BufWriter};
    use std::process::{Command, Stdio};
    let mut child = Command::new(
        std::env::args_os()
            .nth(1)
            .expect("Pass the path to tingskriver-stt"),
    )
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .spawn()
    .unwrap();
    let mut input = BufWriter::new(child.stdin.take().unwrap());
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let samples = vec![0.125; 240000];
    let mut times = Vec::new();
    for id in 0..21 {
        let start = std::time::Instant::now();
        ipc::write_frame(&mut input, id, b"\"Echo\"", &samples).unwrap();
        let frame = ipc::read_frame(&mut output).unwrap().unwrap();
        if id > 0 {
            times.push(start.elapsed());
        }
        assert_eq!(frame.control, b"null");
        assert_eq!(frame.samples, samples);
        assert_eq!(frame.id, id);
    }
    drop(input);
    assert!(child.wait().unwrap().success());
    times.sort();
    println!(
        "STT worker + optimized client, 15 s PCM: p50={:?}, p95={:?}, max={:?}",
        times[10], times[18], times[19]
    );
    assert!(times[18].as_millis() < 50);
}
