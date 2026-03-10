pub(crate) fn is_korean() -> bool {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .filter_map(|key| std::env::var(key).ok())
        .map(|value| value.trim().to_ascii_lowercase())
        .any(|value| value == "ko" || value.starts_with("ko_") || value.starts_with("ko-"))
}

pub(crate) fn localize<'a>(en: &'a str, ko: &'a str) -> &'a str {
    if is_korean() { ko } else { en }
}

#[cfg(test)]
mod tests {
    use super::is_korean;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn detects_korean_lang_prefixes() {
        let _guard = env_lock().lock().expect("lock env");
        let original = std::env::var("LANG").ok();
        unsafe {
            std::env::set_var("LANG", "ko_KR.UTF-8");
        }
        assert!(is_korean());
        match original {
            Some(value) => unsafe {
                std::env::set_var("LANG", value);
            },
            None => unsafe {
                std::env::remove_var("LANG");
            },
        }
    }
}
