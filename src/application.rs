// src/application.rs

use crate::configuration_manager::{self, ApplicationSettings, EventActionSetting, DeviceAction, DeviceActionType};
use crate::game_event_processor::{self, GameStateSnapshot};
use crate::message_passing::{CommandToAsyncTasks, UpdateFromAsyncTasks, ClonableButtplugClientDevice}; // Добавили ClonableButtplugClientDevice
use crate::war_thunder_connector::WarThunderIndicators;
use eframe::egui;
use tokio::sync::mpsc;
use buttplug::client::ButtplugClientDevice; 

pub struct WarThunderHapticsApplication {
    command_sender: mpsc::Sender<CommandToAsyncTasks>,
    update_receiver: mpsc::Receiver<UpdateFromAsyncTasks>,
    settings: ApplicationSettings,
    current_wt_indicators: Option<WarThunderIndicators>,
    game_state_snapshot: GameStateSnapshot,
    // Храним ClonableButtplugClientDevice, чтобы соответствовать сообщениям
    // Или конвертируем при получении, но для простоты UI будем хранить его.
    // Либо храним ButtplugClientDevice и конвертируем при отправке/получении, если Clone для enum не нужен.
    // Пока оставим Vec<ButtplugClientDevice>, а Clonable используется только в канале.
    // Это значит, что при получении ClonableButtplugClientDevice мы будем извлекать .0
    buttplug_devices: Vec<ButtplugClientDevice>, 
    selected_device_index_in_vec: Option<usize>,
    is_buttplug_connected: bool,
    is_war_thunder_connected: bool,
    log_messages: Vec<String>,
    is_processing_enabled: bool,
    config_editor_new_event_name: String,
    config_editor_new_event_intensity: f64,
    config_editor_new_event_duration: u64,
}

impl WarThunderHapticsApplication {
    pub fn new(
        _creation_context: &eframe::CreationContext<'_>,
        command_sender: mpsc::Sender<CommandToAsyncTasks>,
        update_receiver: mpsc::Receiver<UpdateFromAsyncTasks>,
    ) -> Self {
        let initial_settings = match configuration_manager::load_configuration() {
            Ok(settings) => settings,
            Err(err_msg) => {
                tracing::error!("Ошибка загрузки конфигурации: {}. Используются настройки по умолчанию.", err_msg);
                ApplicationSettings::default()
            }
        };
        let _ = command_sender.try_send(CommandToAsyncTasks::UpdateApplicationSettings(initial_settings.clone()));

        Self {
            command_sender,
            update_receiver,
            settings: initial_settings,
            current_wt_indicators: None,
            game_state_snapshot: GameStateSnapshot::default(),
            buttplug_devices: Vec::new(), // Здесь храним оригинальный ButtplugClientDevice
            selected_device_index_in_vec: None,
            is_buttplug_connected: false,
            is_war_thunder_connected: false,
            log_messages: vec!["Приложение запущено.".to_string()],
            is_processing_enabled: false,
            config_editor_new_event_name: "Новое событие".to_string(),
            config_editor_new_event_intensity: 0.5,
            config_editor_new_event_duration: 500,
        }
    }

    fn add_log_message(&mut self, message: String) {
        tracing::info!("{}", message);
        self.log_messages.insert(0, message);
        if self.log_messages.len() > 100 {
            self.log_messages.pop();
        }
    }

    fn handle_incoming_updates(&mut self) {
        while let Ok(update) = self.update_receiver.try_recv() {
            match update {
                UpdateFromAsyncTasks::LogMessage(msg) => self.add_log_message(msg),
                UpdateFromAsyncTasks::WarThunderIndicatorsUpdate(indicators) => {
                    self.current_wt_indicators = Some(indicators.clone());
                    if self.is_processing_enabled {
                        let actions_to_take = game_event_processor::process_war_thunder_data(
                            &indicators,
                            &self.settings,
                            &mut self.game_state_snapshot,
                        );
                        for device_action in actions_to_take {
                            if let Some(device_idx_in_vec) = self.selected_device_index_in_vec.or_else(|| if !self.buttplug_devices.is_empty() { Some(0)} else {None} ) {
                                if let Some(device) = self.buttplug_devices.get(device_idx_in_vec) { 
                                    match device_action.action_type {
                                        DeviceActionType::Vibrate => {
                                            self.add_log_message(format!(
                                                "Игровое событие: вибрация устр-ва '{}' (индекс {}) инт. {} на {} мс",
                                                device.name(), 
                                                device.index(),
                                                device_action.intensity,
                                                device_action.duration_milliseconds
                                            ));
                                            let _ = self.command_sender.try_send(CommandToAsyncTasks::VibrateDevice {
                                                device_index: device_idx_in_vec, 
                                                speed: device_action.intensity,
                                            });
                                        }
                                        DeviceActionType::Stop => {
                                            let _ = self.command_sender.try_send(CommandToAsyncTasks::StopDevice(device_idx_in_vec));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                UpdateFromAsyncTasks::WarThunderConnectionStatus(is_connected) => {
                    self.is_war_thunder_connected = is_connected;
                    if !is_connected { self.current_wt_indicators = None; }
                }
                UpdateFromAsyncTasks::ButtplugConnected => {
                    self.is_buttplug_connected = true;
                    self.add_log_message("Успешно подключено к Buttplug серверу.".to_string());
                }
                UpdateFromAsyncTasks::ButtplugDisconnected => {
                    self.is_buttplug_connected = false;
                    self.buttplug_devices.clear();
                    self.selected_device_index_in_vec = None;
                    self.add_log_message("Отключено от Buttplug сервера.".to_string());
                }
                UpdateFromAsyncTasks::ButtplugDeviceFound(clonable_device) => { 
                    let device = clonable_device.0; // Извлекаем внутренний ButtplugClientDevice
                    if !self.buttplug_devices.iter().any(|d_arc| d_arc.index() == device.index()) {
                        self.add_log_message(format!(
                            "Найдено устройство Buttplug: {} (Индекс: {}, Атрибуты: {:?})",
                            device.name(),
                            device.index(),
                            device.message_attributes()
                        ));
                        self.buttplug_devices.push(device); // Храним оригинальный ButtplugClientDevice
                        if self.selected_device_index_in_vec.is_none() && !self.buttplug_devices.is_empty() {
                            self.selected_device_index_in_vec = Some(0);
                        }
                    }
                }
                UpdateFromAsyncTasks::ButtplugDeviceLost(clonable_device) => { 
                    let device = clonable_device.0; // Извлекаем внутренний ButtplugClientDevice
                    self.add_log_message(format!("Устройство Buttplug потеряно: {} (Индекс: {})", device.name(), device.index()));
                    self.buttplug_devices.retain(|d_arc| d_arc.index() != device.index());
                    if let Some(selected_idx) = self.selected_device_index_in_vec {
                        if selected_idx >= self.buttplug_devices.len() {
                            self.selected_device_index_in_vec = if self.buttplug_devices.is_empty() { None } else { Some(0) };
                        }
                    }
                }
                UpdateFromAsyncTasks::ButtplugError(err_msg) => {
                    self.add_log_message(format!("Ошибка Buttplug: {}", err_msg));
                }
                 UpdateFromAsyncTasks::ApplicationSettingsLoaded(loaded_settings) => {
                    self.settings = loaded_settings;
                    self.add_log_message("Настройки успешно загружены.".to_string());
                }
            }
        }
    }
}

impl eframe::App for WarThunderHapticsApplication {
    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_incoming_updates();

        egui::TopBottomPanel::top("top_panel").show(context, |ui| {
            egui::menu::bar(ui, |ui| {
                // ... (меню Файл и Управление без изменений) ...
                 ui.menu_button("Файл", |ui| {
                    if ui.button("Сохранить конфигурацию").clicked() {
                        match configuration_manager::save_configuration(&self.settings) {
                            Ok(_) => self.add_log_message("Конфигурация успешно сохранена.".to_string()),
                            Err(e) => self.add_log_message(format!("Ошибка сохранения конфигурации: {}", e)),
                        }
                        ui.close_menu();
                    }
                    if ui.button("Загрузить конфигурацию").clicked() {
                         match configuration_manager::load_configuration() {
                            Ok(loaded_settings) => {
                                self.settings = loaded_settings.clone();
                                let _ = self.command_sender.try_send(CommandToAsyncTasks::UpdateApplicationSettings(loaded_settings));
                                self.add_log_message("Конфигурация успешно загружена.".to_string());
                            },
                            Err(e) => self.add_log_message(format!("Ошибка загрузки конфигурации: {}", e)),
                        }
                        ui.close_menu();
                    }
                    if ui.button("Выход").clicked() {
                        context.send_viewport_cmd(egui::ViewportCommand::Close);
                        ui.close_menu();
                    }
                });
                 ui.menu_button("Управление", |ui| {
                    if ui.checkbox(&mut self.is_processing_enabled, "Включить обработку событий WT").changed() {
                        if self.is_processing_enabled {
                            self.add_log_message("Обработка событий War Thunder включена.".to_string());
                            let _ = self.command_sender.try_send(CommandToAsyncTasks::StartProcessing);
                        } else {
                            self.add_log_message("Обработка событий War Thunder выключена.".to_string());
                            let _ = self.command_sender.try_send(CommandToAsyncTasks::StopProcessing);
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                     if ui.button("Подключиться/Сканировать Buttplug").clicked() {
                        let _ = self.command_sender.try_send(CommandToAsyncTasks::ScanForButtplugDevices);
                        ui.close_menu();
                    }
                    if ui.button("Отключиться от Buttplug").clicked() {
                        let _ = self.command_sender.try_send(CommandToAsyncTasks::DisconnectButtplug);
                        ui.close_menu();
                    }
                });
            });
        });

        egui::CentralPanel::default().show(context, |ui| {
            ui.heading(&self.settings.application_name);
            ui.separator();

            ui.collapsing("Статус", |ui| {
                // ... (статус WT и Buttplug сервера без изменений) ...
                ui.horizontal(|ui| {
                    ui.label("War Thunder API:");
                    ui.label(egui::RichText::new(if self.is_war_thunder_connected { "ПОДКЛЮЧЕНО" } else { "ОТКЛЮЧЕНО" })
                        .color(if self.is_war_thunder_connected { egui::Color32::GREEN } else { egui::Color32::RED }));
                });
                 ui.horizontal(|ui| {
                    ui.label("Buttplug сервер:");
                    ui.label(egui::RichText::new(if self.is_buttplug_connected { "ПОДКЛЮЧЕНО" } else { "ОТКЛЮЧЕНО" })
                        .color(if self.is_buttplug_connected { egui::Color32::GREEN } else { egui::Color32::RED }));
                });

                if self.is_buttplug_connected && !self.buttplug_devices.is_empty() {
                    ui.label("Подключенные устройства Buttplug:");
                    egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
                        for (idx_in_vec, device) in self.buttplug_devices.iter().enumerate() { 
                            ui.selectable_value(
                                &mut self.selected_device_index_in_vec,
                                Some(idx_in_vec),
                                format!("{}: {} (Индекс: {})", idx_in_vec, device.name(), device.index())
                            );
                        }
                    });

                    if let Some(selected_idx_in_vec) = self.selected_device_index_in_vec {
                         if ui.button("Тест вибрации выбранного").clicked() {
                             let _ = self.command_sender.try_send(CommandToAsyncTasks::VibrateDevice{device_index: selected_idx_in_vec, speed: 0.5});
                         }
                         if ui.button("Стоп выбранного").clicked() {
                             let _ = self.command_sender.try_send(CommandToAsyncTasks::StopDevice(selected_idx_in_vec));
                         }
                    }
                } else if self.is_buttplug_connected {
                     ui.label("Устройства Buttplug не найдены. Попробуйте сканировать.");
                }
            });
            ui.separator();
            // ... (остальные секции UI без изменений: Данные WT, Конфигурация, Логи) ...
            ui.collapsing("Данные War Thunder (Live)", |ui| {
                if let Some(indicators) = &self.current_wt_indicators {
                    egui::Grid::new("wt_indicators_grid")
                        .num_columns(2)
                        .spacing([40.0, 4.0])
                        .striped(true)
                        .show(ui, |ui| {
                            ui.label("Тип техники:"); ui.label(format!("{:?}", indicators.vehicle_type.as_deref().unwrap_or("N/A"))); ui.end_row();
                            ui.label("Скорость:"); ui.label(format!("{:.2}", indicators.speed.unwrap_or(0.0))); ui.end_row();
                            ui.label("Здоровье:"); ui.label(format!("{:.2}%", indicators.health_percentage.unwrap_or(0.0))); ui.end_row();
                        });
                } else {
                    ui.label("Нет данных от War Thunder.");
                }
            });
            ui.separator();

            ui.collapsing("Конфигурация действий", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Интервал опроса WT (мс):");
                    ui.label(self.settings.polling_interval_milliseconds.to_string());
                });
                ui.horizontal(|ui| {
                    ui.label("Адрес сервера Buttplug (для WebSocket):");
                    ui.text_edit_singleline(&mut self.settings.buttplug_server_address);
                });

                ui.separator();
                ui.label("Действия на события:");
                egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                    let mut action_to_delete_index: Option<usize> = None;
                    for (index, event_action) in self.settings.event_actions.iter_mut().enumerate() {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut event_action.enabled, "");
                                ui.text_edit_singleline(&mut event_action.name);
                            });
                            ui.label(format!("  Действие: {:?}, Интенсивность: {:.2}, Длительность: {} мс",
                                event_action.device_action.action_type,
                                event_action.device_action.intensity,
                                event_action.device_action.duration_milliseconds
                            ));
                            if ui.add(egui::Button::new("Удалить").small()).clicked() {
                                action_to_delete_index = Some(index);
                            }
                        });
                    }
                    if let Some(index) = action_to_delete_index {
                        self.settings.event_actions.remove(index);
                        self.add_log_message(format!("Действие #{} удалено. Не забудьте сохранить конфигурацию.", index));
                        let _ = self.command_sender.try_send(CommandToAsyncTasks::UpdateApplicationSettings(self.settings.clone()));
                    }
                });

                ui.separator();
                ui.label("Добавить новое действие (очень упрощенно):");
                 ui.horizontal(|ui| {
                    ui.label("Имя:");
                    ui.text_edit_singleline(&mut self.config_editor_new_event_name);
                });
                 ui.horizontal(|ui| {
                    ui.label("Интенсивность (0-1):");
                    ui.add(egui::Slider::new(&mut self.config_editor_new_event_intensity, 0.0..=1.0));
                });
                ui.horizontal(|ui| {
                    ui.label("Длительность (мс):");
                    ui.add(egui::DragValue::new(&mut self.config_editor_new_event_duration).speed(10.0).range(0..=60000));
                });

                if ui.button("Добавить действие вибрации").clicked() {
                    let new_action = EventActionSetting {
                        name: self.config_editor_new_event_name.trim().to_string(),
                        enabled: true,
                        device_action: DeviceAction {
                            action_type: DeviceActionType::Vibrate,
                            intensity: self.config_editor_new_event_intensity,
                            duration_milliseconds: self.config_editor_new_event_duration,
                        }
                    };
                    if !new_action.name.is_empty() {
                        self.settings.event_actions.push(new_action);
                        self.add_log_message("Новое действие добавлено. Не забудьте сохранить конфигурацию.".to_string());
                        let _ = self.command_sender.try_send(CommandToAsyncTasks::UpdateApplicationSettings(self.settings.clone()));
                        self.config_editor_new_event_name = "Новое событие".to_string();
                        self.config_editor_new_event_intensity = 0.5;
                        self.config_editor_new_event_duration = 500;
                    } else {
                        self.add_log_message("Имя нового события не может быть пустым.".to_string());
                    }
                }
            });
            ui.separator();

            ui.collapsing("Логи", |ui| {
                egui::ScrollArea::vertical().max_height(200.0).auto_shrink([false, false]).show(ui, |ui| {
                    for msg in self.log_messages.iter() {
                        ui.label(msg);
                    }
                });
            });
        });

        context.request_repaint_after(std::time::Duration::from_millis(100));
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        match configuration_manager::save_configuration(&self.settings) {
            Ok(_) => self.add_log_message("Конфигурация автоматически сохранена при выходе.".to_string()),
            Err(e) => self.add_log_message(format!("Ошибка автосохранения конфигурации: {}", e)),
        }
        let _ = self.command_sender.try_send(CommandToAsyncTasks::StopProcessing);
        let _ = self.command_sender.try_send(CommandToAsyncTasks::DisconnectButtplug);
    }
}