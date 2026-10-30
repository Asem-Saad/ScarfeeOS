use scarfeeos_core::lang::Language;

fn main() {
    let lang = std::env::var("SCARFEEOS_LANG")
        .ok()
        .and_then(|v| Language::from_code(&v))
        .unwrap_or(Language::English);
    println!("ScarfeeOS core starting (language: {})", lang.native_name());
}
