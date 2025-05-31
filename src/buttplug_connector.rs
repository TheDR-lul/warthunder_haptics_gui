// src/buttplug_connector.rs

use crate::message_passing::{CommandToAsyncTasks, UpdateFromAsyncTasks, ClonableButtplugClientDevice};
use buttplug::client::{
    ButtplugClient, ButtplugClientDevice, ButtplugClientEvent, ButtplugClientError};
use buttplug::core::message::{ActuatorType, ScalarCmdV4, ScalarSubcommandV4};

use futures::StreamExt;
use tokio::sync::mpsc;

pub async fn run_buttplug_service_loop(
    gui_update_sender: mpsc::Sender<UpdateFromAsyncTasks>,
    mut command_receiver: mpsc::Receiver<CommandToAsyncTasks>,
    _buttplug_server_address: String,
) {
    let mut client_opt: Option<ButtplugClient> = None;
    let mut connected_devices: Vec<ButtplugClientDevice> = Vec::new();

    loop {
        tokio::select! {
            biased;

            Some(command) = command_receiver.recv() => {
                match command {
                    CommandToAsyncTasks::ScanForButtplugDevices => {
                        let mut client_needs_connection_attempt = true;
                        if let Some(ref client) = client_opt {
                            if client.connected() {
                                client_needs_connection_attempt = false;
                            }
                        } else {
                            client_opt = Some(ButtplugClient::new("WarThunder Haptics GUI"));
                        }

                        if client_needs_connection_attempt {
                            if let Some(ref client) = client_opt {
                                tracing::info!("Клиент не подключен или только что создан. Попытка подключения (InProcess)...");
                                // Этот метод должен быть доступен, если фича "in-process-connector" в Cargo.toml активна
                                match client.connect_in_process(None).await {
                                    Ok(_) => {
                                        let _ = gui_update_sender.try_send(UpdateFromAsyncTasks::ButtplugConnected);
                                        let _ = gui_update_sender.try_send(UpdateFromAsyncTasks::LogMessage("Успешно подключено к Buttplug (InProcess).".to_string()));
                                    }
                                    Err(err) => {
                                        tracing::error!("Не удалось подключиться к InProcess: {:?}", err);
                                        let _ = gui_update_sender.try_send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка подключения InProcess: {}", err)));
                                        let _ = gui_update_sender.try_send(UpdateFromAsyncTasks::ButtplugDisconnected);
                                        client_opt = None; 
                                        continue; 
                                    }
                                }
                            }
                        }
                        
                        if let Some(ref client) = client_opt {
                            if client.connected() {
                                tracing::info!("Начинаем сканирование устройств Buttplug...");
                                if let Err(err) = client.start_scanning().await {
                                    tracing::error!("Ошибка при старте сканирования: {:?}", err);
                                    let _ = gui_update_sender.try_send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка сканирования: {}", err)));
                                } else {
                                    let _ = gui_update_sender.try_send(UpdateFromAsyncTasks::LogMessage("Сканирование устройств Buttplug запущено.".to_string()));
                                }
                            }
                        }
                    }
                    CommandToAsyncTasks::VibrateDevice { device_index, speed } => {
                        if let Some(ref client) = client_opt {
                            if client.connected() {
                                if let Some(device) = connected_devices.get(device_index) {
                                    let device_clone = device.clone();
                                    tracing::info!("Вибрация устройства '{}' (индекс устр.: {}) со скоростью {}", device_clone.name(), device_clone.index(), speed);
                                    tokio::spawn(async move {
                                        if let Some(scalar_attrs) = device_clone.message_attributes().scalar_cmd() { // scalar_cmd() возвращает Option<&Vec<Arc<ScalarCmdV4Features>>>
                                            if let Some(vibrator_attr_feature) = scalar_attrs.iter().find(|feat_arc| feat_arc.actuator_type() == ActuatorType::Vibrate) {
                                                let actuator_feature_idx = vibrator_attr_feature.index();
                                                // Используем ScalarSubcommandV4 и ScalarCmdV4
                                                let subcommands = vec![ScalarSubcommandV4::new(actuator_feature_idx, speed, ActuatorType::Vibrate)];
                                                let cmd_to_send = ScalarCmdV4::new(device_clone.index(), subcommands);
                                                if let Err(e) = device_clone.scalar_v2(&cmd_to_send).await { // Устройство может ожидать scalar_v2 или аналогичный для V4 команд
                                                    tracing::error!("Ошибка вибрации устройства {}: {:?}", device_clone.name(), e);
                                                }
                                            } else { tracing::warn!("Устройство {} не имеет вибраторов.", device_clone.name()); }
                                        } else { tracing::warn!("Устройство {} не поддерживает ScalarCmd.", device_clone.name()); }
                                    });
                                }
                            }
                        }
                    }
                    CommandToAsyncTasks::StopDevice(device_index) => {
                         if let Some(ref client) = client_opt {
                            if client.connected() {
                                if let Some(device) = connected_devices.get(device_index) {
                                    let device_clone = device.clone();
                                    tokio::spawn(async move { if let Err(e) = device_clone.stop().await { tracing::error!("Stop Err {}: {:?}",device_clone.name(), e);}});
                                }
                            }
                        }
                    }
                    CommandToAsyncTasks::DisconnectButtplug => {
                        if let Some(client_instance) = client_opt.take() {
                            if client_instance.connected() { let _ = client_instance.disconnect().await; }
                        }
                        connected_devices.clear();
                        let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                        let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Отключено от Buttplug по команде.".to_string())).await;
                    }
                    CommandToAsyncTasks::UpdateApplicationSettings(_settings) => {}
                }
            },
            event_stream_item = async {
                if let Some(client) = client_opt.as_ref() {
                    if client.connected() {
                        return client.event_stream().next().await;
                    }
                }
                None
            } => {
                if let Some(event_result) = event_stream_item { 
                    match event_result {
                        Ok(actual_event) => { 
                            match actual_event {
                                ButtplugClientEvent::DeviceAdded(device_arc) => { 
                                    tracing::info!("Найдено устр-во: {} (Индекс: {})", device_arc.name(), device_arc.index());
                                    if !connected_devices.iter().any(|d| d.index() == device_arc.index()) {
                                        connected_devices.push(device_arc.clone()); 
                                        if gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDeviceFound(ClonableButtplugClientDevice(device_arc))).await.is_err() { return; }
                                    }
                                }

                                ButtplugClientEvent::DeviceRemoved(device_identifier) => { 
                                    let removed_idx = device_identifier.device_index(); // Этот метод должен быть у ButtplugClientDeviceIdentifier
                                    tracing::info!("Устр-во удалено (Индекс устройства в сессии: {})", removed_idx);
                                    
                                    let mut device_to_send_lost: Option<ButtplugClientDevice> = None;
                                    connected_devices.retain(|dev_arc_in_list| {
                                        if dev_arc_in_list.index() == removed_idx {
                                            device_to_send_lost = Some(dev_arc_in_list.clone());
                                            false
                                        } else { true }
                                    });
                                    if let Some(lost_arc) = device_to_send_lost { 
                                         if gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDeviceLost(ClonableButtplugClientDevice(lost_arc))).await.is_err() { return; }
                                    }
                                }
                                ButtplugClientEvent::ServerDisconnect => {
                                    tracing::info!("Buttplug сервер отключился.");
                                    client_opt = None;
                                    connected_devices.clear();
                                    if gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await.is_err() { return; }
                                    if gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Buttplug сервер отключился.".to_string())).await.is_err() { return; }
                                }
                                _ => { /* Другие события */ }
                            }
                        }
                        Err(err) => {
                            tracing::error!("Ошибка в потоке событий Buttplug: {:?}", err);
                            if gui_update_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка потока событий: {}", err))).await.is_err() { return; }
                            if matches!(err, ButtplugClientError::ButtplugConnectorError(_)) {
                                client_opt = None; connected_devices.clear();
                                if gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await.is_err() { return; }
                            }
                        }
                    }
                } else {
                    if let Some(c) = client_opt.as_ref() {
                        if !c.connected() {
                            client_opt = None; connected_devices.clear();
                            if gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await.is_err() { return; }
                        }
                    }
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            },
            else => {
                tracing::info!("Цикл Buttplug сервиса завершается.");
                break;
            }
        }
    }
}
// Вспомогательная функция attempt_client_connect была удалена, т.к. логика встроена в ScanForButtplugDevices