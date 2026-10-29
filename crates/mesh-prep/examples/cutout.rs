//! Save the raw BiRefNet cut-out of an image, for comparing against other tools.
//!   cargo run --release -p mesh-prep --example cutout -- in.png out.png
fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let im = mesh_prep::open(std::path::Path::new(&a[1]))?.to_rgba8();
    mesh_prep::cutout::cutout(&im)?.save(&a[2])?;
    Ok(())
}
