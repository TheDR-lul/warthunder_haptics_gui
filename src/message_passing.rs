// src/message_passing.rs

use crate::configuration_manager::ApplicationSettings;
use crate::war_thunder_connector::WarThunderIndicators;
use buttplug::client::ButtplugClientDevice; // Это Arc<ButtplugDeviceImpl>
use std::sync::Arc;

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
#[derive(Debug)]
pub struct ClonableButtplugClientDevice(pub Arc<ButtplugClientDevice>);

impl Clone for ClonableButtplugClientDevice {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

#[derive(Debug, Clone)] 
pub enum UpdateFromAsyncTasks {
    LogMessage(String),
    WarThunderIndicatorsUpdate(WarThunderIndicators),
    WarThunderConnectionStatus(bool),
    ButtplugConnected,
    ButtplugDisconnected,
    ButtplugDeviceFound(ClonableButtplugClientDevice), // Используем обертку
    ButtplugDeviceLost(ClonableButtplugClientDevice),  // Используем обертку
    ButtplugError(String),
    ApplicationSettingsLoaded(ApplicationSettings),
}