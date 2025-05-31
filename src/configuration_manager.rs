// src/configuration_manager.rs

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf; // Path убран, т.к. не использовался отдельно
use directories::ProjectDirs;

// ... (DeviceActionType, DeviceAction, EventActionSetting, ApplicationSettings, impl Default for ApplicationSettings - остаются как были) ...
// Код DeviceActionType, DeviceAction, EventActionSetting, ApplicationSettings, impl Default for ApplicationSettings, default_intensity, default_duration не меняется
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum DeviceActionType {
    Vibrate,
    Stop,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DeviceAction {
    pub action_type: DeviceActionType,
    #[serde(default = "default_intensity")]
    pub intensity: f64,
    #[serde(default = "default_duration")]
    pub duration_milliseconds: u64,
}

fn default_intensity() -> f64 { 0.5 }
fn default_duration() -> u64 { 500 }

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EventActionSetting {
    pub name: String,
    pub enabled: bool,
    pub device_action: DeviceAction,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ApplicationSettings {
    pub application_name: String,
    pub polling_interval_milliseconds: u64,
    pub buttplug_server_address: String,
    #[serde(default)]
    pub event_actions: Vec<EventActionSetting>,
}

impl Default for ApplicationSettings {
    fn default() -> Self {
        Self {
            application_name: "WarThunder Haptics GUI (Default)".to_string(),
            polling_interval_milliseconds: 250,
            buttplug_server_address: "ws://127.0.0.1:12345".to_string(), // Для WebSocket. Для InProcess это поле не так важно.
            event_actions: vec![
                EventActionSetting {
                    name: "Пример: Легкая вибрация при старте".to_string(),
                    enabled: true,
                    device_action: DeviceAction {
                        action_type: DeviceActionType::Vibrate,
                        intensity: 0.3,
                        duration_milliseconds: 1000,
                    }
                }
            ],
        }
    }
}


fn get_config_path() -> Result<PathBuf, String> {
    if let Some(proj_dirs) = ProjectDirs::from("com", "YourAppName", "WarThunderHapticsGUI") { // Можешь изменить "com", "YourAppName"
        let config_dir = proj_dirs.config_dir();
        if !config_dir.exists() {
            fs::create_dir_all(config_dir).map_err(|e| format!("Не удалось создать директорию конфигурации: {}", e))?;
        }
        Ok(config_dir.join("settings.toml"))
    } else {
        Err("Не удалось определить директорию конфигурации.".to_string())
    }
}

pub fn load_configuration() -> Result<ApplicationSettings, String> {
    let config_file_path = get_config_path()?;
    
    if !config_file_path.exists() {
        tracing::warn!("Файл конфигурации {:?} не найден. Будет создан файл с настройками по умолчанию.", config_file_path);
        let default_settings = ApplicationSettings::default();
        save_configuration(&default_settings)?;
        return Ok(default_settings);
    }

    let config_content = fs::read_to_string(&config_file_path)
        .map_err(|e| format!("Ошибка чтения файла конфигурации {:?}: {}", config_file_path, e))?;
    
    // Используем serde_toml для десериализации
    serde_toml::from_str(&config_content)
        .map_err(|e| format!("Ошибка парсинга TOML из файла конфигурации {:?}: {}", config_file_path, e))
}

pub fn save_configuration(settings: &ApplicationSettings) -> Result<(), String> {
    let config_file_path = get_config_path()?;
    // Используем serde_toml для сериализации
    let toml_content = serde_toml::to_string_pretty(settings) // to_string_pretty из serde_toml
        .map_err(|e| format!("Ошибка сериализации настроек в TOML: {}", e))?;
    
    fs::write(&config_file_path, toml_content)
        .map_err(|e| format!("Ошибка записи файла конфигурации {:?}: {}", config_file_path, e))
}