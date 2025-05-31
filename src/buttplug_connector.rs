// src/buttplug_connector.rs

use crate::message_passing::{CommandToAsyncTasks, UpdateFromAsyncTasks};
use buttplug::client::{
    ButtplugClient, ButtplugClientDevice, ButtplugClientEvent, ButtplugClientError,
    ButtplugClientDeviceIdentifier, // Импорт для идентификатора устройства
};
use buttplug::core::message::{self, ActuatorType, ScalarCmd, ScalarSubcommand}; // Используем message:: для конкретных типов
use futures::StreamExt;
use tokio::sync::mpsc;

pub async fn run_buttplug_service_loop(
    gui_update_sender: mpsc::Sender<UpdateFromAsyncTasks>,
    mut command_receiver: mpsc::Receiver<CommandToAsyncTasks>,
    _buttplug_server_address: String, // Для InProcess не используется
) {
    let mut client_opt: Option<ButtplugClient> = None;
    let mut connected_devices: Vec<ButtplugClientDevice> = Vec::new();

    loop {
        tokio::select! {
            biased; // Приоритет командам от GUI

            Some(command) = command_receiver.recv() => {
                match command {
                    CommandToAsyncTasks::ScanForButtplugDevices => {
                        let current_client = match client_opt.as_ref() {
                            Some(c) => c,
                            None => {
                                tracing::info!("Клиент Buttplug не инициализирован. Попытка создания и подключения (InProcess)...");
                                let new_client = ButtplugClient::new("WarThunder Haptics GUI");
                                if attempt_client_connect(&new_client, &gui_update_sender).await.is_ok() {
                                    client_opt = Some(new_client);
                                    client_opt.as_ref().unwrap() // Возвращаем ссылку на только что созданного клиента
                                } else {
                                    client_opt = None; // Если подключение не удалось
                                    continue; // Пропускаем остальную часть итерации
                                }
                            }
                        };

                        if current_client.connected() {
                            tracing::info!("Начинаем сканирование устройств Buttplug...");
                            if let Err(err) = current_client.start_scanning().await {
                                tracing::error!("Ошибка при старте сканирования: {:?}", err);
                                let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка сканирования: {}", err))).await;
                            } else {
                                let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Сканирование устройств Buttplug запущено.".to_string())).await;
                            }
                        } else {
                             tracing::info!("Клиент Buttplug существует, но не подключен. Попытка подключения...");
                             if attempt_client_connect(current_client, &gui_update_sender).await.is_ok() {
                                 if let Err(err) = current_client.start_scanning().await {
                                     tracing::error!("Ошибка сканирования после переподключения: {:?}", err);
                                 }
                             }
                        }
                    }
                    CommandToAsyncTasks::VibrateDevice { device_index, speed } => {
                        if let Some(ref cl) = client_opt {
                            if cl.connected() {
                                if let Some(device) = connected_devices.get(device_index) {
                                    let device_clone = device.clone();
                                    tracing::info!("Вибрация устройства '{}' (индекс в списке: {}, индекс устр.: {}) со скоростью {}", 
                                        device_clone.name(), device_index, device_clone.index(), speed);
                                    tokio::spawn(async move {
                                        if let Some(scalar_features) = device_clone.message_attributes().scalar_cmd() {
                                            if let Some(vibrator_feature) = scalar_features.iter().find(|feat| *feat.actuator_type() == message::ActuatorType::Vibrate) {
                                                let actuator_feature_index = vibrator_feature.index();
                                                let subcommands = vec![message::ScalarSubcommand::new(actuator_feature_index, speed, message::ActuatorType::Vibrate)];
                                                let cmd_to_send = message::ScalarCmd::new(device_clone.index(), subcommands);
                                                if let Err(e) = device_clone.scalar(&cmd_to_send).await {
                                                    tracing::error!("Ошибка вибрации устройства {}: {:?}", device_clone.name(), e);
                                                }
                                            } else {
                                                tracing::warn!("Устройство {} не имеет вибраторов (ActuatorType::Vibrate).", device_clone.name());
                                            }
                                        } else {
                                            tracing::warn!("Устройство {} не поддерживает ScalarCmd.", device_clone.name());
                                        }
                                    });
                                }
                            }
                        }
                    }
                    CommandToAsyncTasks::StopDevice(device_index) => {
                         if let Some(ref cl) = client_opt {
                            if cl.connected() {
                                if let Some(device) = connected_devices.get(device_index) {
                                    let device_clone = device.clone();
                                    tokio::spawn(async move { if let Err(e) = device_clone.stop().await { tracing::error!("Stop Err {}: {:?}",device_clone.name(), e);}});
                                }
                            }
                        }
                    }
                    CommandToAsyncTasks::DisconnectButtplug => {
                        if let Some(cl) = client_opt.take() { // Забираем клиента из Option
                            if cl.connected() {
                                tracing::info!("Отключение от Buttplug сервера...");
                                if let Err(e) = cl.disconnect().await {
                                    tracing::error!("Ошибка при отключении от Buttplug: {:?}", e);
                                }
                            }
                        }
                        connected_devices.clear();
                        // Уведомляем GUI об отключении
                        if gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await.is_err() { tracing::warn!("GUI канал (ButtplugDisconnected) закрыт"); }
                        if gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Отключено от Buttplug сервера по команде.".to_string())).await.is_err() { tracing::warn!("GUI канал (LogMessage) закрыт");}
                    }
                    CommandToAsyncTasks::UpdateApplicationSettings(_settings) => {
                        // Здесь можно обновить _buttplug_server_address, если он используется для WebSocket
                    }
                }
            },
            // Обработка событий от клиента Buttplug
            event_stream_item = async {
                if let Some(client) = client_opt.as_ref() {
                    if client.connected() {
                        return client.event_stream().next().await;
                    }
                }
                None
            } => {
                if let Some(event_result) = event_stream_item { // event_result это Result<ButtplugClientEvent, ButtplugClientError>
                    match event_result {
                        Ok(actual_event) => {
                            match actual_event {
                                ButtplugClientEvent::DeviceAdded(device_arc) => {
                                    tracing::info!("Найдено устр-во: {} (Индекс: {})", device_arc.name(), device_arc.index());
                                    if !connected_devices.iter().any(|d| d.index() == device_arc.index()) {
                                        connected_devices.push(device_arc.clone());
                                        if gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDeviceFound(device_arc)).await.is_err() { tracing::warn!("GUI канал (DeviceFound) закрыт"); }
                                    }
                                }
                                ButtplugClientEvent::DeviceRemoved(device_identifier_arc) => { // Arc<ButtplugClientDeviceIdentifier>
                                    tracing::info!("Устр-во удалено (Индекс устр-ва в сессии: {})", device_identifier_arc.device_index());
                                    let mut device_to_send_lost: Option<ButtplugClientDevice> = None;
                                    connected_devices.retain(|dev_arc_in_list| {
                                        if dev_arc_in_list.index() == device_identifier_arc.device_index() {
                                            device_to_send_lost = Some(dev_arc_in_list.clone());
                                            false
                                        } else { true }
                                    });
                                    if let Some(lost_arc) = device_to_send_lost {
                                         if gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDeviceLost(lost_arc)).await.is_err() { tracing::warn!("GUI канал (DeviceLost) закрыт"); }
                                    }
                                }
                                ButtplugClientEvent::ServerDisconnect => {
                                    tracing::info!("Buttplug сервер отключился.");
                                    client_opt = None;
                                    connected_devices.clear();
                                    if gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await.is_err() { tracing::warn!("GUI канал (Disconnected) закрыт"); }
                                    if gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Buttplug сервер отключился.".to_string())).await.is_err() { tracing::warn!("GUI канал (LogMessage) закрыт"); }
                                }
                                _ => { /* Другие события */ }
                            }
                        }
                        Err(err) => {
                            tracing::error!("Ошибка в потоке событий Buttplug: {:?}", err);
                            if gui_update_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка потока событий: {}", err))).await.is_err() { tracing::warn!("GUI канал (Error) закрыт"); }
                            if matches!(err, ButtplugClientError::ButtplugConnectorError(_)) {
                                client_opt = None; connected_devices.clear();
                                if gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await.is_err() { tracing::warn!("GUI канал (Disconnected) закрыт"); }
                            }
                        }
                    }
                } else { // Стрим None (завершился или клиент не был готов/подключен)
                    if let Some(c) = client_opt.as_ref() {
                        if !c.connected() {
                            tracing::info!("Клиент Buttplug отсоединен (поток событий завершен или клиент не подключен).");
                            client_opt = None; // Сбрасываем клиента
                            connected_devices.clear();
                            if gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await.is_err() { tracing::warn!("GUI канал (Disconnected) закрыт"); }
                        }
                    }
                    // Если стрим None, добавляем небольшую задержку, чтобы не перегружать CPU в select!
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            },
            else => {
                tracing::info!("Цикл Buttplug сервиса завершается (канал команд закрыт).");
                break;
            }
        }
    }
}

// Вспомогательная функция для подключения клиента
async fn attempt_client_connect(
    client_to_connect: &ButtplugClient,
    gui_update_sender: &mpsc::Sender<UpdateFromAsyncTasks>
) -> Result<(), ButtplugClientError> {
    if client_to_connect.connected() {
        tracing::info!("Клиент уже подключен.");
        return Ok(());
    }
    tracing::info!("Попытка подключения клиента Buttplug (InProcess)...");
    // `connect_in_process` вызывается на &self и возвращает Result<(), ButtplugConnectorError>
    match client_to_connect.connect_in_process(None).await {
        Ok(_) => {
            let _ = gui_update_sender.try_send(UpdateFromAsyncTasks::ButtplugConnected);
            let _ = gui_update_sender.try_send(UpdateFromAsyncTasks::LogMessage("Успешно подключено к Buttplug (InProcess).".to_string()));
            Ok(())
        }
        Err(err) => { // err здесь это ButtplugConnectorError
            tracing::error!("Не удалось подключиться к InProcess: {:?}", err);
            // Преобразуем ButtplugConnectorError в ButtplugClientError для единообразия, если это нужно
            let client_error = ButtplugClientError::from(err);
            let _ = gui_update_sender.try_send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка подключения InProcess: {}", client_error)));
            let _ = gui_update_sender.try_send(UpdateFromAsyncTasks::ButtplugDisconnected);
            Err(client_error)
        }
    }
}