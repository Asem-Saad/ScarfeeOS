use std::fs;
use std::io;
use std::path::Path;

/// Tauri serves one directory, but the panel loads `../../locales/<lang>/scarfeeos.ftl`
/// from the page root. Gather the panel files and the catalogs in `dist/` so the
/// window sees the same paths as the dev bridge. Tests and other non-assets are left out.
fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let manifest = Path::new(&manifest);
    let dist = manifest.join("dist");
    let ui = manifest.join("../ui");
    let locales = manifest.join("../../locales");
    println!("cargo:rerun-if-changed={}", ui.display());
    println!("cargo:rerun-if-changed={}", locales.display());

    if dist.exists() {
        fs::remove_dir_all(&dist).expect("clear dist/");
    }
    fs::create_dir_all(&dist).expect("create dist/");
    copy_files(&ui, &dist, &|name| {
        !name.contains(".test.") && [".html", ".js", ".css"].iter().any(|ext| name.ends_with(ext))
    })
    .expect("copy shell/ui");
    for lang in fs::read_dir(&locales).expect("read locales/").flatten() {
        if lang.path().is_dir() {
            let target = dist.join("locales").join(lang.file_name());
            fs::create_dir_all(&target).expect("create dist/locales");
            copy_files(&lang.path(), &target, &|name| name.ends_with(".ftl")).expect("copy locales");
        }
    }

    tauri_build::build();
}

fn copy_files(from: &Path, to: &Path, keep: &dyn Fn(&str) -> bool) -> io::Result<()> {
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if entry.file_type()?.is_file() && keep(&name.to_string_lossy()) {
            fs::copy(entry.path(), to.join(name))?;
        }
    }
    Ok(())
}
