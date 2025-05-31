// src/configuration_manager.rs

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use directories::ProjectDirs; // Для поиска стандартных директорий конфигурации

// Действие, которое нужно выполнить на девайсе
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum DeviceActionType {
    Vibrate,
    // Rotate, // Добавь другие типы по мере необходимости
    // Linear,
    Stop,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DeviceAction {
    pub action_type: DeviceActionType,
    #[serde(default = "default_intensity")]
    pub intensity: f64, // 0.0 to 1.0
    #[serde(default = "default_duration")]
    pub duration_milliseconds: u64, // 0 для постоянного действия до команды Stop
}

fn default_intensity() -> f64 { 0.5 }
fn default_duration() -> u64 { 500 }


// Одно правило сопоставления события действию
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EventActionSetting {
    pub name: String,
    pub enabled: bool,
    // Здесь можно будет добавить условия, например:
    // pub war_thunder_event_field: String, // Какое поле из WarThunderIndicators проверять
    // pub comparison_type: String, // "Equals", "GreaterThan", "LessThan", "Changed"
    // pub target_value: String, // Значение для сравнения (может быть числом, строкой, bool)
    pub device_action: DeviceAction,
}

// Основная структура настроек приложения
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ApplicationSettings {
    pub application_name: String,
    pub polling_interval_milliseconds: u64,
    pub buttplug_server_address: String,
    #[serde(default)] // Если в конфиге нет event_actions, будет пустой Vec
    pub event_actions: Vec<EventActionSetting>,
}

impl Default for ApplicationSettings {
    fn default() -> Self {
        Self {
            application_name: "WarThunder Haptics GUI (Default)".to_string(),
            polling_interval_milliseconds: 250,
            buttplug_server_address: "ws://127.0.0.1:12345".to_string(),
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
    if let Some(proj_dirs) = ProjectDirs::from("com", "YourAppName", "WarThunderHapticsGUI") {
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
        save_configuration(&default_settings)?; // Сохраняем дефолтный конфиг, чтобы пользователь мог его видеть
        return Ok(default_settings);
    }

    let config_content = fs::read_to_string(&config_file_path)
        .map_err(|e| format!("Ошибка чтения файла конфигурации {:?}: {}", config_file_path, e))?;
    
    toml::from_str(&config_content)
        .map_err(|e| format!("Ошибка парсинга TOML из файла конфигурации {:?}: {}", config_file_path, e))
}

pub fn save_configuration(settings: &ApplicationSettings) -> Result<(), String> {
    let config_file_path = get_config_path()?;
    let toml_content = toml::to_string_pretty(settings)
        .map_err(|e| format!("Ошибка сериализации настроек в TOML: {}", e))?;
    
    fs::write(&config_file_path, toml_content)
        .map_err(|e| format!("Ошибка записи файла конфигурации {:?}: {}", config_file_path, e))
}