// src/message_passing.rs

use crate::configuration_manager::ApplicationSettings;
use crate::war_thunder_connector::WarThunderIndicators;
// В buttplug v9 ButtplugClientDevice это уже Arc<ButtplugDeviceImpl>
use buttplug::client::ButtplugClientDevice; // Это уже Arc<ButtplugDeviceImpl>

// Сообщения от GUI к асинхронным задачам
#[derive(Debug, Clone)]
pub enum CommandToAsyncTasks {
    StartProcessing,
    StopProcessing,
    UpdateApplicationSettings(ApplicationSettings),
    VibrateDevice {
        device_index: usize,
        speed: f64,
    },
    StopDevice(usize),
    ScanForButtplugDevices,
    DisconnectButtplug,
}

// Сообщения от асинхронных задач к GUI
#[derive(Debug, Clone)]
pub enum UpdateFromAsyncTasks {
    LogMessage(String),
    WarThunderIndicatorsUpdate(WarThunderIndicators),
    WarThunderConnectionStatus(bool),
    ButtplugConnected,
    ButtplugDisconnected,
    // ButtplugClientDevice (который Arc<ButtplugDeviceImpl>) можно клонировать
    ButtplugDeviceFound(ButtplugClientDevice),
    ButtplugDeviceLost(ButtplugClientDevice),
    ButtplugError(String),
    ApplicationSettingsLoaded(ApplicationSettings),
}