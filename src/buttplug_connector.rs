// src/buttplug_connector.rs

use crate::message_passing::{CommandToAsyncTasks, UpdateFromAsyncTasks};
use buttplug::client::{ButtplugClient, ButtplugClientDevice, ButtplugClientEvent, ButtplugClientError}; // ButtplugClientDevice это Arc<ButtplugDeviceImpl>
use buttplug::core::connector::ButtplugInProcessClientConnector;
use buttplug::core::message::{ // Исправлен путь на singular 'message'
    ActuatorType, ButtplugMessageSpecVersion, ScalarCmd, ClientMessageResult, ServerMessage, Endpoint
};
use futures::StreamExt;
use std::sync::Arc; // Может не понадобиться явно, если ButtplugClientDevice уже Arc
use tokio::sync::mpsc;

pub async fn run_buttplug_service_loop(
    gui_update_sender: mpsc::Sender<UpdateFromAsyncTasks>,
    mut command_receiver: mpsc::Receiver<CommandToAsyncTasks>,
    _buttplug_server_address: String, // Менее релевантно для InProcess, но оставим
) {
    let mut client: Option<ButtplugClient> = None;
    let mut connected_devices: Vec<ButtplugClientDevice> = Vec::new(); // Vec<Arc<ButtplugDeviceImpl>>

    loop {
        tokio::select! {
            Some(command) = command_receiver.recv() => {
                match command {
                    CommandToAsyncTasks::ScanForButtplugDevices => {
                        if let Some(ref existing_client) = client {
                            if existing_client.connected() {
                                tracing::info!("Начинаем сканирование устройств Buttplug...");
                                if let Err(err) = existing_client.start_scanning().await {
                                    tracing::error!("Ошибка при старте сканирования устройств Buttplug: {:?}", err);
                                    let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка сканирования: {}", err))).await;
                                } else {
                                    let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Сканирование устройств Buttplug запущено.".to_string())).await;
                                }
                            } else {
                                let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Клиент Buttplug не подключен. Сначала подключитесь.".to_string())).await;
                            }
                        } else {
                            tracing::info!("Клиент Buttplug не инициализирован. Попытка подключения (InProcess)...");
                            let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Попытка подключения к Buttplug серверу (InProcess)...".to_string())).await;

                            match ButtplugInProcessClientConnector::try_create(None) { // v9.x API
                                Ok(connector_builder) => {
                                    let connector = connector_builder.finish();
                                    // В v9 клиент создается с именем и принимаемой версией спеки
                                    let new_client = ButtplugClient::new_with_options(
                                        "WarThunder Haptics GUI",
                                        ButtplugMessageSpecVersion::V3, // или другая версия, если нужно
                                        true, // Allow raw messages, если нужно (обычно нет)
                                        None // Device config map, если нужно
                                    ).expect("Failed to create client with options"); // или обработать Result

                                    if let Err(err) = new_client.connect(connector).await {
                                        tracing::error!("Не удалось подключиться к InProcess коннектору Buttplug: {:?}", err);
                                        let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка подключения: {}", err))).await;
                                        let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                                    } else {
                                        client = Some(new_client);
                                        let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugConnected).await;
                                        let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Успешно подключено к Buttplug (InProcess).".to_string())).await;
                                        if let Some(ref c) = client {
                                            if let Err(err) = c.start_scanning().await {
                                                 tracing::error!("Ошибка при старте сканирования после подключения: {:?}", err);
                                            }
                                        }
                                    }
                                }
                                Err(err) => {
                                    tracing::error!("Не удалось создать InProcess коннектор Buttplug: {:?}", err);
                                    let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка создания коннектора: {}", err))).await;
                                    let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                                }
                            }
                        }
                    }
                    CommandToAsyncTasks::VibrateDevice { device_index, speed } => {
                        if let Some(ref cl) = client { // client это &Option<ButtplugClient>
                            if cl.connected() {
                                if let Some(device) = connected_devices.get(device_index) { // device это &ButtplugClientDevice
                                    let device_clone = device.clone(); // Клонируем Arc<ButtplugDeviceImpl>
                                    tracing::info!("Вибрация устройства {:?} со скоростью {}", device_clone.name(), speed);
                                    tokio::spawn(async move {
                                        // Ищем первый вибратор (ActuatorType::Vibrate)
                                        if let Some(scalar_attrs) = device_clone.message_attributes().scalar_cmd() {
                                            if let Some(vibrator_attr) = scalar_attrs.iter().find(|attr| attr.actuator_type() == &ActuatorType::Vibrate) {
                                                let actuator_index = vibrator_attr.index();
                                                let scalar_cmd = ScalarCmd::new(device_clone.index(), vec![(actuator_index, speed, ActuatorType::Vibrate)]); // Используем индекс устройства из device_clone.index()
                                                if let Err(e) = device_clone.scalar(&scalar_cmd).await {
                                                    tracing::error!("Ошибка вибрации устройства {}: {:?}", device_clone.name(), e);
                                                }
                                            } else {
                                                tracing::warn!("Устройство {} не имеет вибраторов (ActuatorType::Vibrate).", device_clone.name());
                                            }
                                        } else {
                                            tracing::warn!("Устройство {} не поддерживает ScalarCmd.", device_clone.name());
                                        }
                                    });
                                } else {
                                    tracing::warn!("Устройство с индексом {} не найдено.", device_index);
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
                                    tracing::info!("Остановка устройства {:?}", device_clone.name());
                                    tokio::spawn(async move {
                                        if let Err(e) = device_clone.stop().await { // stop() должен работать
                                            tracing::error!("Ошибка остановки устройства {}: {:?}", device_clone.name(), e);
                                        }
                                    });
                                }
                            }
                        }
                    }
                    CommandToAsyncTasks::DisconnectButtplug => {
                        if let Some(ref cl) = client {
                            if cl.connected() {
                                tracing::info!("Отключение от Buttplug сервера...");
                                // `disconnect()` возвращает Result, но мы его не обрабатываем если не нужно
                                let _ = cl.disconnect().await; // Ошибку обработает ServerDisconnect или будет видно по connected()
                                // Явно посылаем сообщения, так как ServerDisconnect может прийти с задержкой или не прийти если ошибка была в disconnect
                                let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Отключено от Buttplug сервера.".to_string())).await;
                                let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                                connected_devices.clear();
                            }
                        }
                        client = None; // Сбрасываем клиента в любом случае
                    }
                    CommandToAsyncTasks::UpdateApplicationSettings(settings) => {
                        // Адрес сервера для InProcess не так важен, но если бы был WebSocket:
                        let new_address = settings.buttplug_server_address;
                        if _buttplug_server_address != new_address {
                            // _buttplug_server_address = new_address; // Не можем изменить _buttplug_server_address напрямую, если она не mut
                            // Для изменения адреса WebSocket сервера потребовалось бы переподключение.
                            let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage(format!("Адрес сервера Buttplug (в конфиге) изменен на: {}. Переподключитесь для WebSocket.", new_address))).await;
                        }
                    }
                     _ => {}
                }
            },
            // Обработка событий от клиента Buttplug
            Some(event_result) = async { client.as_ref().map(|c| c.event_stream()).map(|mut s| s.next().await).flatten() } => {
                // event_result это Result<ButtplugClientEvent, ButtplugClientError>
                match event_result {
                    Ok(event) => {
                        match event {
                            ButtplugClientEvent::DeviceAdded(new_device_arc) => { // new_device_arc это ButtplugClientDevice (Arc<ButtplugDeviceImpl>)
                                tracing::info!("Найдено устройство Buttplug: {} (Индекс устр-ва: {}, Адрес: {})", new_device_arc.name(), new_device_arc.index(), new_device_arc.address());
                                if !connected_devices.iter().any(|d_arc| d_arc.address() == new_device_arc.address()) {
                                    connected_devices.push(new_device_arc.clone());
                                    let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDeviceFound(new_device_arc)).await;
                                } else {
                                    tracing::info!("Устройство {} уже было в списке.", new_device_arc.name());
                                }
                            }
                            ButtplugClientEvent::DeviceRemoved(removed_device_info) => { // removed_device_info это DeviceInfo в v9
                                                                                        // Или это тоже Arc<ButtplugClientDevice>? Документация говорит DeviceInfo.
                                                                                        // Если это DeviceInfo, нужно искать по индексу или адресу.
                                                                                        // Давайте предположим, что это DeviceInfo, как часто бывает.
                                tracing::info!("Устройство Buttplug удалено/отключено: {} (Индекс: {})", removed_device_info.name(), removed_device_info.index());
                                let mut device_to_send_lost = None;
                                connected_devices.retain(|dev_arc| {
                                    if dev_arc.index() == removed_device_info.index() {
                                        device_to_send_lost = Some(dev_arc.clone());
                                        false
                                    } else {
                                        true
                                    }
                                });
                                if let Some(lost_arc) = device_to_send_lost {
                                     let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDeviceLost(lost_arc)).await;
                                }

                            }
                            ButtplugClientEvent::ServerDisconnect => {
                                tracing::info!("Buttplug сервер отключился.");
                                client = None; // Сбрасываем клиента
                                connected_devices.clear();
                                let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                                let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Buttplug сервер отключился.".to_string())).await;
                            }
                            other_event => {
                                tracing::trace!("Получено другое событие Buttplug: {:?}", other_event);
                            }
                        }
                    }
                    Err(err) => {
                        tracing::error!("Ошибка в потоке событий Buttplug: {:?}", err);
                        let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка потока событий: {}", err))).await;
                        if matches!(err, ButtplugClientError::ButtplugConnectorError(_)) {
                            tracing::info!("Потеряно соединение с сервером Buttplug из-за ошибки коннектора.");
                            client = None;
                            connected_devices.clear();
                            let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                        }
                    }
                }
            },
            else => {
                tracing::info!("Цикл Buttplug сервиса завершается.");
                break;
            }
        }
    }
}