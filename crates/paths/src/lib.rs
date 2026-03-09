use directories::BaseDirs;
use opensession_runtime_config::CONFIG_FILE_NAME;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("could not determine home directory")]
    HomeUnavailable,
}

fn base_dirs() -> Result<BaseDirs, PathError> {
    BaseDirs::new().ok_or(PathError::HomeUnavailable)
}

fn join_segments(base: &Path, segments: &[&str]) -> PathBuf {
    segments
        .iter()
        .fold(base.to_path_buf(), |path, segment| path.join(segment))
}

pub fn home_dir() -> Result<PathBuf, PathError> {
    Ok(base_dirs()?.home_dir().to_path_buf())
}

pub fn config_dir() -> Result<PathBuf, PathError> {
    Ok(join_segments(&home_dir()?, &[".config", "opensession"]))
}

pub fn data_dir() -> Result<PathBuf, PathError> {
    Ok(join_segments(
        &home_dir()?,
        &[".local", "share", "opensession"],
    ))
}

pub fn runtime_config_path() -> Result<PathBuf, PathError> {
    Ok(config_dir()?.join(CONFIG_FILE_NAME))
}

pub fn local_db_path() -> Result<PathBuf, PathError> {
    if let Some(path) = std::env::var_os("OPENSESSION_LOCAL_DB_PATH")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        return Ok(path);
    }
    Ok(data_dir()?.join("local.db"))
}

pub fn local_store_root() -> Result<PathBuf, PathError> {
    Ok(data_dir()?.join("objects"))
}

#[cfg(test)]
mod tests {
    use super::{
        config_dir, data_dir, home_dir, local_db_path, local_store_root, runtime_config_path,
    };
    use opensession_runtime_config::CONFIG_FILE_NAME;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};

    fn env_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: tests serialize environment mutation with `env_test_lock`, so no
            // concurrent readers/writers observe partially updated process env state.
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }

        fn clear(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: tests serialize environment mutation with `env_test_lock`, so no
            // concurrent readers/writers observe partially updated process env state.
            unsafe { std::env::remove_var(key) };
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            if let Some(value) = &self.previous {
                // SAFETY: tests serialize environment mutation with `env_test_lock`.
                unsafe { std::env::set_var(self.key, value) };
            } else {
                // SAFETY: tests serialize environment mutation with `env_test_lock`.
                unsafe { std::env::remove_var(self.key) };
            }
        }
    }

    #[test]
    fn config_path_uses_opensession_suffix() {
        let path = config_dir().expect("config dir");
        assert_eq!(
            path,
            home_dir()
                .expect("home dir")
                .join(".config")
                .join("opensession")
        );
        assert_eq!(
            runtime_config_path()
                .expect("runtime config path")
                .file_name()
                .expect("runtime config filename")
                .to_string_lossy(),
            CONFIG_FILE_NAME
        );
    }

    #[test]
    fn data_paths_use_opensession_suffix() {
        let _lock = env_test_lock().lock().expect("env lock");
        let _guard = EnvVarGuard::clear("OPENSESSION_LOCAL_DB_PATH");
        let path = data_dir().expect("data dir");
        assert_eq!(
            path,
            home_dir()
                .expect("home dir")
                .join(".local")
                .join("share")
                .join("opensession")
        );
        assert_eq!(
            local_db_path()
                .expect("local db path")
                .parent()
                .expect("local db parent"),
            path.as_path()
        );
        assert_eq!(
            local_store_root()
                .expect("local store root")
                .parent()
                .expect("local store parent"),
            path.as_path()
        );
    }

    #[test]
    fn local_db_path_prefers_env_override() {
        let _lock = env_test_lock().lock().expect("env lock");
        let _guard = EnvVarGuard::set("OPENSESSION_LOCAL_DB_PATH", "/tmp/opensession-test.db");
        assert_eq!(
            local_db_path().expect("local db path"),
            PathBuf::from("/tmp/opensession-test.db")
        );
    }
}
