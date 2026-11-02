// Generate the gate's inclusive ranges from the runner's Rust toolchain.
fn main() {
    let mut start = None;
    let mut separator = "";
    println!("[");
    for n in 0..=0x110000 {
        let keep = char::from_u32(n).is_some_and(char::is_alphanumeric);
        match (start, keep) {
            (None, true) => start = Some(n),
            (Some(first), false) => {
                print!("{separator}[{first},{}]", n - 1);
                separator = ",\n";
                start = None;
            }
            _ => {}
        }
    }
    println!("\n]");
}
