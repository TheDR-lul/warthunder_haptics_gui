// src/buttplug_connector.rs

use crate::message_passing::{CommandToAsyncTasks, UpdateFromAsyncTasks};
use buttplug::client::{ButtplugClient, ButtplugClientDevice, ButtplugClientEvent, VibrateCommand};
use buttplug::core::connector::{
    ButtplugConnector, ButtplugInProcessClientConnector, ButtplugWebsocketClientTransport,
};
use buttplug::core::messages::test::DeviceAdded; // Пример, если бы имя было такое
use futures::StreamExt; // для event_stream.next()
use std::sync::Arc;
use tokio::sync::mpsc;

pub async fn run_buttplug_service_loop(
    gui_update_sender: mpsc::Sender<UpdateFromAsyncTasks>,
    mut command_receiver: mpsc::Receiver<CommandToAsyncTasks>,
    mut buttplug_server_address: String, // Адрес сервера Buttplug
) {
    let mut client: Option<ButtplugClient> = None;
    let mut connected_devices: Vec<Arc<ButtplugClientDevice>> = Vec::new();

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
                            // Попытка подключиться, если еще не подключены
                            tracing::info!("Клиент Buttplug не инициализирован. Попытка подключения к {}...", buttplug_server_address);
                             let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage(format!("Попытка подключения к Buttplug серверу: {}", buttplug_server_address))).await;

                            // TODO: Рассмотреть возможность выбора типа коннектора через GUI (InProcess vs WebSocket)
                            // Пока используем WebSocket по умолчанию
                            let connector_result = ButtplugInProcessClientConnector::new_embedded(); // Простой вариант для начала
                            // let transport = ButtplugWebsocketClientTransport::new_insecure_connector(&buttplug_server_address);
                            // let connector_result = ButtplugClient::connect("WarThunder Haptics", transport).await;


                            match connector_result {
                                Ok(new_connector) => {
                                    let new_client = ButtplugClient::new("WarThunder Haptics GUI");
                                    if let Err(err) = new_client.connect(new_connector).await {
                                        tracing::error!("Не удалось подключиться к встроенному коннектору Buttplug: {:?}", err);
                                        let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка подключения: {}", err))).await;
                                        let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                                    } else {
                                        client = Some(new_client);
                                        let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugConnected).await;
                                        let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Успешно подключено к Buttplug.".to_string())).await;
                                        // Сразу запускаем сканирование после подключения
                                        if let Some(ref c) = client {
                                            if let Err(err) = c.start_scanning().await {
                                                 tracing::error!("Ошибка при старте сканирования после подключения: {:?}", err);
                                            }
                                        }
                                    }
                                }
                                Err(err) => { // Это для случая если бы connect возвращал Result напрямую
                                    tracing::error!("Не удалось создать или подключить клиент Buttplug: {:?}", err);
                                    let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка создания клиента: {}", err))).await;
                                    let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;

                                }
                            }
                        }
                    }
                    CommandToAsyncTasks::VibrateDevice { device_index, speed } => {
                        if let Some(ref cl) = client {
                            if cl.connected() {
                                if let Some(device_arc) = connected_devices.get(device_index) {
                                    let device = device_arc.clone(); // Клонируем Arc для асинхронной задачи
                                    tracing::info!("Вибрация устройства {:?} со скоростью {}", device.name(), speed);
                                    tokio::spawn(async move {
                                        if let Err(e) = device.vibrate(&VibrateCommand::Speed(speed)).await {
                                            tracing::error!("Ошибка вибрации устройства {}: {:?}", device.name(), e);
                                            // Тут можно отправить сообщение об ошибке в GUI, если нужно
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
                                if let Some(device_arc) = connected_devices.get(device_index) {
                                    let device = device_arc.clone();
                                    tracing::info!("Остановка устройства {:?}", device.name());
                                    tokio::spawn(async move {
                                        if let Err(e) = device.stop().await {
                                            tracing::error!("Ошибка остановки устройства {}: {:?}", device.name(), e);
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
                                if let Err(e) = cl.disconnect().await {
                                    tracing::error!("Ошибка при отключении от Buttplug: {:?}", e);
                                    let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка отключения: {}", e))).await;
                                } else {
                                    let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Отключено от Buttplug сервера.".to_string())).await;
                                    let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                                    connected_devices.clear(); // Очищаем список устройств
                                }
                            }
                        }
                        client = None; // В любом случае сбрасываем клиента
                    }
                    CommandToAsyncTasks::UpdateApplicationSettings(settings) => {
                        // Обновляем адрес сервера, если он изменился
                        if buttplug_server_address != settings.buttplug_server_address {
                            buttplug_server_address = settings.buttplug_server_address;
                            let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage(format!("Адрес сервера Buttplug изменен на: {}", buttplug_server_address))).await;
                            // Если клиент был подключен, его нужно будет переподключить к новому адресу
                            // Это более сложная логика, пока просто меняем адрес для следующего подключения.
                            if client.is_some() && client.as_ref().unwrap().connected() {
                                let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Для применения нового адреса сервера Buttplug, пожалуйста, переподключитесь.".to_string())).await;
                            }
                        }
                    }
                    _ => {} // Другие команды пока игнорируем
                }
            },
            // Обработка событий от клиента Buttplug (если он существует и подключен)
            Some(event) = async { if let Some(c) = &client { c.event_stream().next().await } else { None } } => {
                if let Some(event_result) = event { // event_stream().next() возвращает Result<ButtplugClientEvent, ButtplugError>
                     match event_result {
                        Ok(ButtplugClientEvent::DeviceAdded(device_message)) => {
                            tracing::info!("Найдено устройство Buttplug: {} (Индекс: {})", device_message.name(), device_message.index());
                            // Важно: ButtplugClientDevice создается из ButtplugClient и DeviceAdded (или Device)
                            // Мы не можем просто взять device_message, нам нужен объект ButtplugClientDevice.
                            // Проверим, есть ли такое устройство уже у клиента.
                            if let Some(ref cl) = client {
                                if let Some(device_obj) = cl.device(&device_message.address()) {
                                     let device_arc = Arc::new(device_obj); // Создаем Arc здесь
                                     connected_devices.push(device_arc.clone());
                                     let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDeviceFound(device_arc)).await;
                                } else {
                                    tracing::warn!("Устройство {} было объявлено, но не найдено в клиенте.", device_message.name());
                                }
                            }
                        }
                        Ok(ButtplugClientEvent::DeviceRemoved(device_info)) => {
                            tracing::info!("Устройство Buttplug удалено/отключено: (Адрес: {:?})", device_info.address());
                            // Удаляем устройство из нашего списка
                            let mut found_device_arc = None;
                            connected_devices.retain(|dev_arc| {
                                if dev_arc.address() == device_info.address() {
                                    found_device_arc = Some(dev_arc.clone());
                                    false // удалить из списка
                                } else {
                                    true // оставить в списке
                                }
                            });
                            if let Some(dev_arc) = found_device_arc {
                                let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDeviceLost(dev_arc)).await;
                            }
                        }
                        Ok(ButtplugClientEvent::ServerDisconnect) => {
                            tracing::info!("Buttplug сервер отключился.");
                            client = None;
                            connected_devices.clear();
                            let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                            let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Buttplug сервер отключился.".to_string())).await;
                        }
                        Ok(other_event) => {
                            tracing::trace!("Получено другое событие Buttplug: {:?}", other_event);
                        }
                        Err(err) => {
                            tracing::error!("Ошибка в потоке событий Buttplug: {:?}", err);
                            let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugError(format!("Ошибка потока событий: {}", err))).await;
                        }
                    }
                } else {
                    // Поток событий завершился, вероятно, клиент отключился
                    if client.is_some() && !client.as_ref().unwrap().connected() {
                         client = None;
                         connected_devices.clear();
                         let _ = gui_update_sender.send(UpdateFromAsyncTasks::ButtplugDisconnected).await;
                         let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Поток событий Buttplug завершен (клиент отключен).".to_string())).await;
                    }
                }
            },
            else => {
                // Все каналы закрыты или произошла ошибка, выходим из цикла
                tracing::info!("Цикл Buttplug сервиса завершается.");
                break;
            }
        }
    }
}