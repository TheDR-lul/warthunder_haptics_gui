// src/message_passing.rs

use crate::configuration_manager::ApplicationSettings; // Предполагаем, что AppSettings будет там
use crate::war_thunder_connector::WarThunderIndicators; // Пример структуры данных от WT
use buttplug::client::ButtplugClientDevice;
use std::sync::Arc;

// Сообщения от GUI к асинхронным задачам
#[derive(Debug, Clone)]
pub enum CommandToAsyncTasks {
    StartProcessing,
    StopProcessing,
    UpdateApplicationSettings(ApplicationSettings),
    VibrateDevice {
        device_index: usize, // Индекс устройства в списке известных
        speed: f64,
    },
    StopDevice(usize), // Индекс устройства
    ScanForButtplugDevices,
    DisconnectButtplug,
}

// Сообщения от асинхронных задач к GUI
#[derive(Debug, Clone)]
pub enum UpdateFromAsyncTasks {
    LogMessage(String), // Простое строковое сообщение для лога
    WarThunderIndicatorsUpdate(WarThunderIndicators),
    WarThunderConnectionStatus(bool), // true если подключено, false если нет
    ButtplugConnected,
    ButtplugDisconnected,
    ButtplugDeviceFound(Arc<ButtplugClientDevice>), // Передаем Arc для избежания проблем с владением
    ButtplugDeviceLost(Arc<ButtplugClientDevice>),
    ButtplugError(String),
    ApplicationSettingsLoaded(ApplicationSettings), // Когда настройки загружены
    // Добавь другие типы сообщений по мере необходимости
}