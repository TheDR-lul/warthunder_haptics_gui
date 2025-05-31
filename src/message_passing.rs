// src/message_passing.rs

use crate::configuration_manager::ApplicationSettings;
use crate::war_thunder_connector::WarThunderIndicators;
use buttplug::client::ButtplugClientDevice; // Это Arc<ButtplugDeviceImpl>
use std::sync::Arc; // Добавим для явного использования Arc, если потребуется

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

// Обертка для ButtplugClientDevice, чтобы гарантировать Clone
// Это нужно только если derive(Clone) на UpdateFromAsyncTasks не работает напрямую с ButtplugClientDevice
#[derive(Debug)]
pub struct ClonableButtplugClientDevice(pub ButtplugClientDevice);

impl Clone for ClonableButtplugClientDevice {
    fn clone(&self) -> Self {
        ClonableButtplugClientDevice(self.0.clone()) // Клонируем внутренний Arc
    }
}


// Сообщения от асинхронных задач к GUI
#[derive(Debug, Clone)] // Clone должен работать для ApplicationSettings, String, bool, WarThunderIndicators
pub enum UpdateFromAsyncTasks {
    LogMessage(String),
    WarThunderIndicatorsUpdate(WarThunderIndicators),
    WarThunderConnectionStatus(bool),
    ButtplugConnected,
    ButtplugDisconnected,
    // Используем нашу обертку, если прямой Clone для ButtplugClientDevice не работает
    ButtplugDeviceFound(ClonableButtplugClientDevice),
    ButtplugDeviceLost(ClonableButtplugClientDevice),
    // Или, если ButtplugClientDevice сам по себе Clone (что должно быть правдой, т.к. это Arc):
    // ButtplugDeviceFound(ButtplugClientDevice),
    // ButtplugDeviceLost(ButtplugClientDevice),
    ButtplugError(String),
    ApplicationSettingsLoaded(ApplicationSettings),
}