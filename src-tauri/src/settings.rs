use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub version: u32,
    #[serde(alias = "PreferredDeviceId")]
    pub preferred_device_id: String,
    #[serde(alias = "Left")]
    pub left: Option<i32>,
    #[serde(alias = "Top")]
    pub top: Option<i32>,
    pub reduced_motion: bool,
    pub startup_enabled: Option<bool>,
}
impl Settings {
    pub fn load(directory: &Path) -> Self {
        let path = directory.join("settings.json");
        let Ok(bytes) = fs::read(&path) else {
            return Self::default();
        };
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
        match serde_json::from_slice::<Self>(bytes) {
            Ok(mut settings) => {
                if settings.preferred_device_id.len() > 1024 {
                    settings.preferred_device_id.clear();
                }
                if settings.version < 3 {
                    let backup = directory.join("settings.before-tauri-v3.json");
                    if !backup.exists() {
                        let _ = fs::copy(&path, backup);
                    }
                }
                settings
            }
            Err(error) => {
                log(directory, &format!("设置无法读取，使用默认值: {error}"));
                Self::default()
            }
        }
    }
    pub fn save(&mut self, directory: &Path) -> Result<(), String> {
        fs::create_dir_all(directory).map_err(|e| e.to_string())?;
        self.version = 3;
        let path = directory.join("settings.json");
        let temporary = directory.join("settings.json.tmp");
        let data = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        {
            use std::io::Write;
            let mut file = fs::File::create(&temporary).map_err(|e| e.to_string())?;
            file.write_all(&data).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
        }
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::Storage::FileSystem::{
                MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
            };
            if MoveFileExW(
                crate::native::wide(temporary.as_os_str()).as_ptr(),
                crate::native::wide(path.as_os_str()).as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            ) == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        #[cfg(not(windows))]
        fs::rename(temporary, path).map_err(|e| e.to_string())?;
        Ok(())
    }
}
pub fn directory(demo: bool) -> PathBuf {
    if let Some(value) = std::env::args().skip_while(|a| a != "--data-dir").nth(1) {
        return PathBuf::from(value);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("ProPadDashboard");
    if demo {
        base.join("Demo")
    } else {
        base
    }
}
pub fn log(directory: &Path, text: &str) {
    use std::io::Write;
    let folder = directory.join("logs");
    let _ = fs::create_dir_all(&folder);
    let path = folder.join("app.log");
    if fs::metadata(&path)
        .map(|m| m.len() > 512 * 1024)
        .unwrap_or(false)
    {
        let _ = fs::remove_file(folder.join("app.previous.log"));
        let _ = fs::rename(&path, folder.join("app.previous.log"));
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {text}", propad_hardware::now_ms());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("propad-settings-{}-{name}", std::process::id()))
    }
    #[test]
    fn old_settings_migrate_and_target_address_is_ignored() {
        let parsed:Settings=serde_json::from_str(r#"{"PreferredDeviceId":"switchpro:ABC","Left":120,"Top":80,"TargetAddress":"112233445566"}"#).unwrap();
        assert_eq!(parsed.preferred_device_id, "switchpro:ABC");
        assert_eq!(parsed.left, Some(120));
        assert_eq!(parsed.top, Some(80));
        assert!(!serde_json::to_string(&parsed)
            .unwrap()
            .contains("TargetAddress"));
    }
    #[test]
    fn atomic_save_can_replace_existing_file() {
        let dir = temp("atomic");
        let mut s = Settings::default();
        s.save(&dir).unwrap();
        s.left = Some(-1920);
        s.reduced_motion = true;
        s.save(&dir).unwrap();
        let actual = Settings::load(&dir);
        assert_eq!(actual.left, Some(-1920));
        assert!(actual.reduced_motion);
        assert_eq!(actual.version, 3);
        fs::remove_file(dir.join("settings.json")).unwrap();
        fs::remove_dir(&dir).unwrap();
    }
    #[test]
    fn old_file_is_backed_up_once() {
        let dir = temp("backup");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, b"{\"Left\":7}").unwrap();
        assert_eq!(Settings::load(&dir).left, Some(7));
        fs::write(&path, b"{\"Left\":8}").unwrap();
        Settings::load(&dir);
        assert_eq!(
            fs::read_to_string(dir.join("settings.before-tauri-v3.json")).unwrap(),
            "{\"Left\":7}"
        );
        fs::remove_file(path).unwrap();
        fs::remove_file(dir.join("settings.before-tauri-v3.json")).unwrap();
        fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn optional_fields_have_safe_defaults() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.version, 0);
        assert!(s.preferred_device_id.is_empty());
        assert!(s.startup_enabled.is_none());
    }
}
