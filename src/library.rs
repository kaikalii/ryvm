use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub fn ryvm_dir() -> io::Result<PathBuf> {
    ensure_dir_exists(
        "Ryvm dir",
        dirs::home_dir()
            .expect("Unable to determine home folder directory")
            .join(".ryvm"),
    )
}

pub fn specs_dir() -> io::Result<PathBuf> {
    ensure_dir_exists("Specs dir", ryvm_dir()?.join("specs"))
}

pub fn samples_dir() -> io::Result<PathBuf> {
    ensure_dir_exists("Samples dir", ryvm_dir()?.join("samples"))
}

pub fn startup_path() -> io::Result<PathBuf> {
    let path = specs_dir()?.join("startup.ron");
    if !path.exists() {
        println!("Startup spec does not exists. Creating it...");
        fs::write(&path, b"{\n\t\n}\n")?;
        println!("type \"specs\" to open the specs directory and edit it");
    }
    Ok(path)
}

fn ensure_dir_exists(name: &str, path: PathBuf) -> io::Result<PathBuf> {
    if !path.exists() {
        println!("{} does not exist. Creating it...", name);
        fs::create_dir_all(&path)?;
    }
    Ok(path)
}

pub fn spec_path<P>(name: P) -> io::Result<PathBuf>
where
    P: AsRef<Path>,
{
    Ok(specs_dir()?
        .join(name)
        .with_extension("ron")
        .canonicalize()?)
}
