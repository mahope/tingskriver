fn main() {
    if tingskriver_core::stt::run_worker().is_err() {
        std::process::exit(1);
    }
}
