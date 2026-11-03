use std::io::{BufReader, BufWriter};
use std::process::{Command, Stdio};
use tingskriver_core::stt::{ipc, Request};

#[test]
fn actual_worker_exit_fails_call_and_restarts_on_next_request() {
    let _binary = env!("CARGO_BIN_EXE_tingskriver-stt");
    let echo = serde_json::to_vec(&Request::Echo).unwrap();
    let mut client = tingskriver_core::stt::Client::new(echo.clone()).unwrap();
    let error = client.call(b"invalid-json", &[0.125; 16000]).err().unwrap();
    assert_eq!(
        error,
        "Den lokale transskriptionsproces stoppede. Prøv igen."
    );
    assert_eq!(client.call(&echo, &[0.25]).unwrap().samples, vec![0.25]);
}

#[test]
fn actual_worker_keeps_control_frames_and_pcm_separate() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_tingskriver-stt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = BufWriter::new(child.stdin.take().unwrap());
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let samples = vec![0.125; 15 * 16000];
    ipc::write_frame(
        &mut input,
        71,
        &serde_json::to_vec(&Request::Echo).unwrap(),
        &samples,
    )
    .unwrap();
    let response = ipc::read_frame(&mut output).unwrap().unwrap();
    assert_eq!(response.id, 71);
    assert_eq!(response.samples, samples);
    let mut measurements = Vec::new();
    for id in 72..87 {
        let start = std::time::Instant::now();
        ipc::write_frame(
            &mut input,
            id,
            &serde_json::to_vec(&Request::Echo).unwrap(),
            &samples,
        )
        .unwrap();
        let response = ipc::read_frame(&mut output).unwrap().unwrap();
        measurements.push(start.elapsed());
        assert_eq!(response.id, id);
        assert_eq!(response.samples, samples);
    }
    measurements.sort();
    eprintln!(
        "Real STT worker, 15 s PCM roundtrip: p50={:?}, p95={:?}",
        measurements[7], measurements[14]
    );
    if !cfg!(debug_assertions) {
        assert!(measurements[14].as_millis() < 50);
    }
    drop(input);
    assert!(child.wait().unwrap().success());
}

#[test]
fn forced_ort_failure_falls_back_to_energy_in_an_isolated_app_process() {
    let status = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "ort_failure_child", "--ignored", "--nocapture"])
        .env("TINGSKRIVER_STT_FORCE_ORT_FAILURE", "1")
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
#[ignore = "Invoked by forced_ort_failure_falls_back_to_energy_in_an_isolated_app_process"]
fn ort_failure_child() {
    use tingskriver_core::vad_trim::{trim, Backend, TrimConfig};
    assert!(std::env::var_os("TINGSKRIVER_STT_FORCE_ORT_FAILURE").is_some());
    // Prove startup reached the actual worker, rather than passing because
    // the executable or its runtime dependencies were missing.
    let mut worker = Command::new(env!("CARGO_BIN_EXE_tingskriver-stt"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = BufWriter::new(worker.stdin.take().unwrap());
    let mut output = BufReader::new(worker.stdout.take().unwrap());
    let init = Request::Vad {
        rate: 16000,
        config: silero_rs::VadConfig::default().into(),
    };
    ipc::write_frame(&mut input, 1, &serde_json::to_vec(&init).unwrap(), &[]).unwrap();
    let response = ipc::read_frame(&mut output).unwrap().unwrap();
    let error: serde_json::Value = serde_json::from_slice(&response.control).unwrap();
    assert_eq!(error["error"], "Taledetektion kunne ikke starte.");
    drop(input);
    assert!(worker.wait().unwrap().success());
    let samples: Vec<f32> = (0..16000)
        .map(|i| if i % 2 == 0 { 0.1 } else { -0.1 })
        .collect();
    let result = trim(&samples, &TrimConfig::default()).unwrap();
    assert_eq!(result.backend, Backend::Energy);
    assert!(!result.samples.is_empty());
}
