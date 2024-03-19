use anyhow::Result;

mod camera;
mod app;

fn main() -> Result<()> {
    #[cfg(not(target_os = "android"))]
    app::run()?;
    Ok(())
}
