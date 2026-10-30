use aios_core::lang::Language;

fn main() {
    let lang = std::env::var("AIOS_LANG")
        .ok()
        .and_then(|v| Language::from_code(&v))
        .unwrap_or(Language::English);
    println!("AIOS core starting (language: {})", lang.native_name());
}
