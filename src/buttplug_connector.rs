// src/buttplug_connector.rs

use crate::message_passing::{CommandToAsyncTasks, UpdateFromAsyncTasks};
use buttplug::client::{ButtplugClient, ButtplugClientDevice, ButtplugClientEvent};
use buttplug::core::connector::ButtplugInProcessClientConnector;
use buttplug::core::message::{ActuatorType, ButtplugMessageSpecVersion, ScalarCmd};
use futures::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn run_buttplug_service_loop(
    gui_update_sender: mpsc::Sender<UpdateFromAsyncTasks>,
    mut command_receiver: mpsc::Receiver<CommandToAsyncTasks>,
    _buttplug_server_address: String,
) {
    let mut client: Option<ButtplugClient> = None;
    let mut connected_devices: Vec<ButtplugClientDevice> = Vec::new();

    loop {
        tokio::select! {
            Some(command) = command_receiver.recv() => {
                match command {
                    CommandToAsyncTasks::ScanForButtplugDevices => {
                        if let Some(ref existing_client) = client {
                            if existing_client.connected() {
                                tracing::info!("Начинаем сканирование устройств Buttplug...");
                                if let Err(err) = existing_client.start_scanning().await {
                                    tracing::error!("Ошибка при сканировании: {:?}", err);
                                    let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка сканирования: {}", err))).await;
                                } else {
                                    let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Сканирование устройств запущено.".to_string())).await;
                                }
                            } else {
                                let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Клиент Buttplug не подключён.".to_string())).await;
                            }
                        } else {
                            tracing::info!("Инициализация InProcess клиента Buttplug...");
                            let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Подключение к серверу Buttplug...".to_string())).await;

                            match ButtplugInProcessClientConnector::try_create(None) {
                                Ok(connector_builder) => {
                                    let connector = connector_builder.finish();
                                    let new_client = ButtplugClient::new_with_options(
                                        "WarThunder Haptics GUI",
                                        ButtplugMessageSpecVersion::V3,
                                        true,
                                        None,
                                    ).expect("Не удалось создать клиента Buttplug");

                                    match new_client.connect(connector).await {
                                        Ok(_) => {
                                            client = Some(new_client);
                                            let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugConnected).await;
                                            let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Подключено к Buttplug (InProcess).".to_string())).await;

                                            if let Some(ref cl) = client {
                                                if let Err(e) = cl.start_scanning().await {
                                                    tracing::error!("Ошибка при запуске сканирования: {:?}", e);
                                                }
                                            }
                                        }
                                        Err(err) => {
                                            tracing::error!("Ошибка подключения: {:?}", err);
                                            let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка подключения: {}", err))).await;
                                            let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                                        }
                                    }
                                }
                                Err(err) => {
                                    tracing::error!("Не удалось создать InProcess коннектор: {:?}", err);
                                    let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка коннектора: {}", err))).await;
                                    let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                                }
                            }
                        }
                    }

                    CommandToAsyncTasks::VibrateDevice { device_index, speed } => {
                        if let Some(ref cl) = client {
                            if cl.connected() {
                                if let Some(device) = connected_devices.get(device_index) {
                                    let device_clone = device.clone();
                                    tracing::info!("Вибрация устройства {} со скоростью {}", device_clone.name(), speed);
                                    tokio::spawn(async move {
                                        if let Some(scalar_attrs) = device_clone.message_attributes().scalar_cmd() {
                                            if let Some(attr) = scalar_attrs.iter().find(|a| a.actuator_type() == &ActuatorType::Vibrate) {
                                                let cmd = ScalarCmd::new(device_clone.index(), vec![(attr.index(), speed, ActuatorType::Vibrate)]);
                                                if let Err(e) = device_clone.scalar(&cmd).await {
                                                    tracing::error!("Ошибка при вибрации {}: {:?}", device_clone.name(), e);
                                                }
                                            } else {
                                                tracing::warn!("{} не поддерживает вибрацию.", device_clone.name());
                                            }
                                        } else {
                                            tracing::warn!("{} не поддерживает ScalarCmd.", device_clone.name());
                                        }
                                    });
                                } else {
                                    tracing::warn!("Устройство по индексу {} не найдено.", device_index);
                                }
                            } else {
                                let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Клиент Buttplug не подключен для вибрации.".to_string())).await;
                            }
                        }
                    }

                    CommandToAsyncTasks::StopDevice(device_index) => {
                        if let Some(ref cl) = client {
                            if cl.connected() {
                                if let Some(device) = connected_devices.get(device_index) {
                                    let device_clone = device.clone();
                                    tracing::info!("Остановка устройства {}", device_clone.name());
                                    tokio::spawn(async move {
                                        if let Err(e) = device_clone.stop().await {
                                            tracing::error!("Ошибка при остановке {}: {:?}", device_clone.name(), e);
                                        }
                                    });
                                }
                            }
                        }
                    }

                    CommandToAsyncTasks::DisconnectButtplug => {
                        if let Some(ref cl) = client {
                            if cl.connected() {
                                tracing::info!("Отключение от сервера Buttplug...");
                                let _ = cl.disconnect().await;
                                let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Отключено от Buttplug.".to_string())).await;
                                let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                                connected_devices.clear();
                            }
                        }
                        client = None;
                    }

                    CommandToAsyncTasks::UpdateApplicationSettings(settings) => {
                        let new_addr = settings.buttplug_server_address;
                        if _buttplug_server_address != new_addr {
                            let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage(
                                format!("Обновлён адрес Buttplug ({}). Переподключитесь вручную.", new_addr)
                            )).await;
                        }
                    }

                    _ => {}
                }
            }

            Some(event) = async {
                if let Some(cl) = &client {
                    let mut stream = cl.event_stream();
                    stream.next().await
                } else {
                    None
                }
            } => {
                match event {
                    Ok(ButtplugClientEvent::DeviceAdded(device)) => {
                        tracing::info!("Добавлено устройство: {}", device.name());
                        connected_devices.push(device.clone());
                        let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage(format!("Устройство подключено: {}", device.name()))).await;
                    }

                    Ok(ButtplugClientEvent::DeviceRemoved(device)) => {
                        tracing::info!("Удалено устройство: {}", device.name());
                        connected_devices.retain(|d| d.index() != device.index());
                        let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage(format!("Устройство отключено: {}", device.name()))).await;
                    }

                    Ok(ButtplugClientEvent::ServerDisconnect) => {
                        tracing::warn!("Отключение от сервера Buttplug");
                        client = None;
                        connected_devices.clear();
                        let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                    }

                    Ok(_) => {}
                    Err(e) => {
                        tracing::error!("Ошибка событий Buttplug: {:?}", e);
                        let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка: {}", e))).await;
                    }
                }
            }
        }
    }
}
