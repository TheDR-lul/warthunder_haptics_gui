// src/buttplug_connector.rs

use tracing::info;

use crate::message_passing::{CommandToAsyncTasks, UpdateFromAsyncTasks, ClonableButtplugClientDevice};
use buttplug::client::{
    ButtplugClient, ButtplugClientDevice, ButtplugClientEvent,
};
use buttplug::core::connector::ButtplugInProcessClientConnector;
use buttplug::core::message::{ActuatorType, ScalarCmdV3, ScalarSubcommandV3};
use futures::{StreamExt, FutureExt}; // Добавлен FutureExt для now_or_never
use tokio::sync::mpsc;
use std::sync::Arc;

pub async fn run_buttplug_service_loop(
    to_gui_sender: mpsc::Sender<UpdateFromAsyncTasks>,
    mut from_gui_receiver: mpsc::Receiver<CommandToAsyncTasks>,
) {
    let mut optional_client: Option<ButtplugClient> = None;
    let mut connected_devices: Vec<Arc<ButtplugClientDevice>> = Vec::new();

    loop {
        tokio::select! {
            biased;

            Some(command_from_gui) = from_gui_receiver.recv() => {
                match command_from_gui {
                    CommandToAsyncTasks::ScanForButtplugDevices => {
                        if optional_client.is_none() {
                            tracing::info!("Клиент Buttplug не инициализирован. Попытка создания и подключения (InProcess)...");
                            let new_client = ButtplugClient::new("WarThunder Haptics GUI");
                            match new_client.connect(ButtplugInProcessClientConnector::default()).await {
                                Ok(_) => {
                                    optional_client = Some(new_client);
                                    let _ = to_gui_sender.send(UpdateFromAsyncTasks::ButtplugConnected).await;
                                    let _ = to_gui_sender.send(UpdateFromAsyncTasks::LogMessage("Успешно подключено к Buttplug (InProcess).".to_string())).await;
                                }
                                Err(connection_error) => {
                                    tracing::error!("Не удалось подключиться к InProcess: {:?}", connection_error);
                                    let _ = to_gui_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка подключения InProcess: {}", connection_error))).await;
                                    let _ = to_gui_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                                    optional_client = None;
                                    continue;
                                }
                            }
                        }

                        if let Some(client_ref) = optional_client.as_ref() {
                            if client_ref.connected() {
                                tracing::info!("Начинаем сканирование устройств Buttplug...");
                                if let Err(scan_error) = client_ref.start_scanning().await {
                                    tracing::error!("Ошибка при старте сканирования: {:?}", scan_error);
                                    let _ = to_gui_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка сканирования: {}", scan_error))).await;
                                } else {
                                    let _ = to_gui_sender.send(UpdateFromAsyncTasks::LogMessage("Сканирование устройств Buttplug запущено.".to_string())).await;
                                }
                            } else {
                                tracing::warn!("Клиент Buttplug не подключен. Сканирование невозможно.");
                                optional_client = None;
                                let _ = to_gui_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                            }
                        }
                    }

                    CommandToAsyncTasks::VibrateDevice { device_index, speed } => {
                        if let Some(ref client_ref) = optional_client {
                            if client_ref.connected() {
                                if let Some(device) = connected_devices.get(device_index) {
                                    let device_to_command = device.clone();
                                    tracing::info!(
                                        "Вибрация устройства '{}' (индекс GUI: {}, индекс BP: {}) со скоростью {}",
                                        device_to_command.name(),
                                        device_index,
                                        device_to_command.index(),
                                        speed
                                    );

                                    if let Some(scalar_features) = device_to_command.message_attributes().scalar_cmd() {
                                        let mut scalar_subcommands = Vec::new();
                                        for feature_actuator in scalar_features {
                                            if *feature_actuator.actuator_type() == ActuatorType::Vibrate {
                                                scalar_subcommands.push(
                                                    ScalarSubcommandV3::new(
                                                        *feature_actuator.index(), // Разыменовываем ссылку
                                                        speed,
                                                        ActuatorType::Vibrate
                                                    )
                                                );
                                            }
                                        }

                                        if !scalar_subcommands.is_empty() {
                                            let assembled_vibration_command = ScalarCmdV3::new(
                                                device_to_command.index(),
                                                scalar_subcommands
                                            );
                                            let target_device_for_vibration = device_to_command.clone();
                                            tokio::spawn(async move {
                                                if let Err(vibration_error) = target_device_for_vibration.scalar(&assembled_vibration_command).await {
                                                    tracing::error!(
                                                        "Ошибка ScalarCmd для {}: {:?}",
                                                        target_device_for_vibration.name(),
                                                        vibration_error
                                                    );
                                                }
                                            });
                                        } else {
                                            tracing::warn!("Устройство {} не имеет подходящих вибраторов.", device_to_command.name());
                                        }
                                    } else {
                                        tracing::warn!("Устройство {} не поддерживает ScalarCmd.", device_to_command.name());
                                    }
                                } else {
                                    tracing::warn!("Устройство с GUI индексом {} не найдено.", device_index);
                                }
                            } else {
                                tracing::warn!("Клиент Buttplug не подключен для VibrateDevice.");
                            }
                        }
                    }

                    CommandToAsyncTasks::StopDevice(device_index) => {
                        if let Some(ref client_ref) = optional_client {
                            if client_ref.connected() {
                                if let Some(device) = connected_devices.get(device_index) {
                                    let device_to_stop = device.clone();
                                    tracing::info!(
                                        "Остановка устройства '{}' (индекс GUI: {}, индекс BP: {})",
                                        device_to_stop.name(),
                                        device_index,
                                        device_to_stop.index()
                                    );
                                    tokio::spawn(async move {
                                        if let Err(stop_error) = device_to_stop.stop().await {
                                            tracing::error!("Ошибка при остановке {}: {:?}", device_to_stop.name(), stop_error);
                                        }
                                    });
                                } else {
                                    tracing::warn!("Устройство с GUI индексом {} не найдено для StopDevice.", device_index);
                                }
                            } else {
                                tracing::warn!("Клиент Buttplug не подключен для StopDevice.");
                            }
                        }
                    }

                    CommandToAsyncTasks::DisconnectButtplug => {
                        if let Some(client_instance) = optional_client.take() {
                            if client_instance.connected() {
                                tracing::info!("Отключение от Buttplug сервера...");
                                if let Err(disconnect_error) = client_instance.disconnect().await {
                                    tracing::error!("Ошибка при отключении от Buttplug: {:?}", disconnect_error);
                                }
                            }
                        }
                        connected_devices.clear();
                        let _ = to_gui_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                        let _ = to_gui_sender.send(UpdateFromAsyncTasks::LogMessage("Отключено от Buttplug сервера по команде.".to_string())).await;
                    }

                    _ => {}
                }
            }

            optional_event_from_stream = async {
                optional_client.as_ref()
                    .and_then(|client| client.connected().then(|| client.event_stream()))
                    .and_then(|mut stream| stream.next().now_or_never())
                    .and_then(|res| res.transpose()) // Решаем проблему Result -> Option
                    .flatten()
            } => {
                match optional_event_from_stream {
                    Some(event) => {
                        match event {
                            ButtplugClientEvent::DeviceAdded(device_arc) => {
                                tracing::info!("Найдено устр-во: {} (Индекс BP: {})", device_arc.name(), device_arc.index());
                                if !connected_devices.iter().any(|d| d.index() == device_arc.index()) {
                                    connected_devices.push(device_arc.clone());
                                    if to_gui_sender.send(UpdateFromAsyncTasks::ButtplugDeviceFound(ClonableButtplugClientDevice(device_arc))).await.is_err() {
                                        tracing::warn!("GUI канал (DeviceFound) закрыт");
                                    }
                                }
                            }
                            ButtplugClientEvent::DeviceRemoved(removed_device_arc) => {
                                tracing::info!("Устр-во удалено: {} (Индекс BP: {})", removed_device_arc.name(), removed_device_arc.index());
                                let mut device_to_send_as_lost: Option<Arc<ButtplugClientDevice>> = None;
                                connected_devices.retain(|device_in_list| {
                                    if device_in_list.index() == removed_device_arc.index() {
                                        device_to_send_as_lost = Some(device_in_list.clone());
                                        false
                                    } else {
                                        true
                                    }
                                });
                                if let Some(lost_device_arc) = device_to_send_as_lost {
                                    if to_gui_sender.send(UpdateFromAsyncTasks::ButtplugDeviceLost(ClonableButtplugClientDevice(lost_device_arc))).await.is_err() {
                                        tracing::warn!("GUI канал (DeviceLost) закрыт");
                                    }
                                }
                            }
                            ButtplugClientEvent::ServerDisconnect => {
                                tracing::info!("Buttplug сервер отключился.");
                                optional_client.take();
                                connected_devices.clear();
                                let _ = to_gui_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                                let _ = to_gui_sender.send(UpdateFromAsyncTasks::LogMessage("Buttplug сервер отключился.".to_string())).await;
                            }
                            ButtplugClientEvent::PingTimeout => {
                                tracing::warn!("Buttplug PING таймаут. Соединение потеряно.");
                                optional_client.take();
                                connected_devices.clear();
                                let _ = to_gui_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                                let _ = to_gui_sender.send(UpdateFromAsyncTasks::LogMessage("Buttplug PING таймаут. Соединение потеряно.".to_string())).await;
                            }
                            _ => {}
                        }
                    }
                    None => {
                        if optional_client.is_some() && !optional_client.as_ref().unwrap().connected() {
                            tracing::info!("Клиент Buttplug отсоединен (поток событий завершен или соединение разорвано).");
                            optional_client = None;
                            connected_devices.clear();
                            if to_gui_sender.try_send(UpdateFromAsyncTasks::ButtplugDisconnected).is_err() {
                                tracing::warn!("GUI канал (ButtplugDisconnected) закрыт при обработке конца стрима.");
                            }
                        }
                        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
                    }
                }
            }

            else => {
                tracing::info!("Цикл Buttplug сервиса завершается (канал команд закрыт или другая причина).");
                if let Some(client_instance) = optional_client.take() {
                    if client_instance.connected() {
                        let _ = client_instance.disconnect().await;
                    }
                }
                break;
            }
        }
    }
}