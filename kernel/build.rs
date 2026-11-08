// Build script: runs on your PC before the kernel is compiled.
// It tells the linker to use our linker script (kernel/linker.ld).
fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-arg=-T{dir}/linker.ld");
    println!("cargo:rerun-if-changed={dir}/linker.ld");
}
