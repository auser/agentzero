/// Ensure the `ui/dist` directory exists so that `#[derive(RustEmbed)]`
/// does not fail at compile time when the frontend has not been built.
fn main() {
    let dist = std::path::Path::new("ui/dist");
    if !dist.exists() {
        std::fs::create_dir_all(dist).expect("failed to create ui/dist placeholder directory");
    }
}
