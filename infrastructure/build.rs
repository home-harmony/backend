fn main() {
    // Re-embed SQL migrations when files in ../migrations are added or modified.
    println!("cargo:rerun-if-changed=../migrations");
}
