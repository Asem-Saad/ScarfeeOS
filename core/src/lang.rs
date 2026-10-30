/// Languages ScarfeeOS supports as main languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    English,
    Arabic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Ltr,
    Rtl,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::Arabic];

    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Arabic => "ar",
        }
    }

    pub fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Arabic => "العربية",
        }
    }

    pub fn direction(self) -> Direction {
        match self {
            Language::English => Direction::Ltr,
            Language::Arabic => Direction::Rtl,
        }
    }

    /// Parses a code such as "en", "ar" or a locale like "ar_SA.UTF-8".
    pub fn from_code(s: &str) -> Option<Language> {
        let base = s.split(|c| c == '_' || c == '-' || c == '.').next()?;
        match base.to_ascii_lowercase().as_str() {
            "en" => Some(Language::English),
            "ar" => Some(Language::Arabic),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_locales() {
        assert_eq!(Language::from_code("ar_SA.UTF-8"), Some(Language::Arabic));
        assert_eq!(Language::from_code("en-US"), Some(Language::English));
        assert_eq!(Language::from_code("fr"), None);
    }

    #[test]
    fn arabic_is_rtl() {
        assert_eq!(Language::Arabic.direction(), Direction::Rtl);
        assert_eq!(Language::English.direction(), Direction::Ltr);
    }
}
