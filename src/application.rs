// src/application.rs

use crate::configuration_manager::{self, ApplicationSettings, EventActionSetting, DeviceAction, DeviceActionType};
use crate::game_event_processor::{self, GameStateSnapshot};
use crate::message_passing::{CommandToAsyncTasks, UpdateFromAsyncTasks};
use crate::war_thunder_connector::WarThunderIndicators; // Убедись, что эта структура определена
use eframe::egui;
use std::sync::Arc;
use tokio::sync::mpsc;
use buttplug::client::ButtplugClientDevice;


pub struct WarThunderHapticsApplication {
    // Каналы для общения с асинхронными задачами
    command_sender: mpsc::Sender<CommandToAsyncTasks>,
    update_receiver: mpsc::Receiver<UpdateFromAsyncTasks>,

    // Состояние приложения
    settings: ApplicationSettings,
    current_wt_indicators: Option<WarThunderIndicators>,
    game_state_snapshot: GameStateSnapshot, // Для отслеживания изменений
    
    // Buttplug состояние
    buttplug_devices: Vec<Arc<ButtplugClientDevice>>,
    selected_device_index: Option<usize>, // Индекс выбранного устройства для ручного управления/тестирования
    is_buttplug_connected: bool,
    is_war_thunder_connected: bool, // Статус подключения к API WT

    // UI Состояние
    log_messages: Vec<String>,
    is_processing_enabled: bool, // Запущена ли основная логика обработки
    
    // Для редактирования настроек (пример)
    config_editor_new_event_name: String,
    config_editor_new_event_intensity: f64,
    config_editor_new_event_duration: u64,
}

impl WarThunderHapticsApplication {
    pub fn new(
        _creation_context: &eframe::CreationContext<'_>, // Может понадобиться для интеграции с нативным окном или persist_native_window
        command_sender: mpsc::Sender<CommandToAsyncTasks>,
        update_receiver: mpsc::Receiver<UpdateFromAsyncTasks>,
    ) -> Self {
        // Загружаем конфигурацию при старте
        let initial_settings = match configuration_manager::load_configuration() {
            Ok(settings) => settings,
            Err(err_msg) => {
                // В GUI мы не можем паниковать, поэтому логируем и используем дефолт
                tracing::error!("Ошибка загрузки конфигурации: {}. Используются настройки по умолчанию.", err_msg);
                // TODO: Показать эту ошибку пользователю в GUI
                ApplicationSettings::default()
            }
        };

        // Отправляем начальные настройки в асинхронные задачи (если они их ожидают)
        let _ = command_sender.try_send(CommandToAsyncTasks::UpdateApplicationSettings(initial_settings.clone()));

        Self {
            command_sender,
            update_receiver,
            settings: initial_settings,
            current_wt_indicators: None,
            game_state_snapshot: GameStateSnapshot::default(),
            buttplug_devices: Vec::new(),
            selected_device_index: None,
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
        tracing::info!("{}", message); // Также логируем через tracing
        self.log_messages.insert(0, message); // Добавляем в начало списка
        if self.log_messages.len() > 100 { // Ограничиваем размер лога в GUI
            self.log_messages.pop();
        }
    }

    fn handle_incoming_updates(&mut self) {
        while let Ok(update) = self.update_receiver.try_recv() {
            match update {
                UpdateFromAsyncTasks::LogMessage(msg) => {
                    self.add_log_message(msg);
                }
                UpdateFromAsyncTasks::WarThunderIndicatorsUpdate(indicators) => {
                    self.current_wt_indicators = Some(indicators.clone());
                    if self.is_processing_enabled {
                        // Обрабатываем данные игры, если процессинг включен
                        let actions_to_take = game_event_processor::process_war_thunder_data(
                            &indicators,
                            &self.settings,
                            &mut self.game_state_snapshot,
                        );

                        for device_action in actions_to_take {
                            // TODO: Отправлять команду на конкретное выбранное устройство или на все?
                            // Пока упрощенно - на первое доступное (если выбрано) или на все.
                            // Нужен более умный выбор устройства из конфига события.
                            if let Some(device_idx) = self.selected_device_index.or_else(|| if !self.buttplug_devices.is_empty() { Some(0)} else {None} ) {
                                // TODO: Реализовать отправку разных типов команд (не только вибрация)
                                match device_action.action_type {
                                    DeviceActionType::Vibrate => {
                                        self.add_log_message(format!(
                                            "Игровое событие: вибрация устройства {} с интенсивностью {} на {} мс",
                                            self.buttplug_devices.get(device_idx).map_or("N/A", |d| d.name()),
                                            device_action.intensity,
                                            device_action.duration_milliseconds
                                        ));
                                        // Для вибрации с длительностью, Buttplug обычно требует посылать Speed, а потом через время Stop.
                                        // Это нужно будет реализовать в buttplug_connector или здесь через таймер.
                                        // Пока просто посылаем команду с интенсивностью.
                                        let _ = self.command_sender.try_send(CommandToAsyncTasks::VibrateDevice {
                                            device_index: device_idx,
                                            speed: device_action.intensity,
                                        });
                                        // TODO: Нужен механизм для остановки вибрации через device_action.duration_milliseconds
                                        // Это можно сделать, породив задачу tokio::time::sleep в buttplug_connector
                                        // или здесь, если у GUI есть доступ к tokio runtime handle.
                                    }
                                    DeviceActionType::Stop => {
                                        let _ = self.command_sender.try_send(CommandToAsyncTasks::StopDevice(device_idx));
                                    }
                                    // Добавить другие типы действий
                                }
                            }
                        }
                    }
                }
                UpdateFromAsyncTasks::WarThunderConnectionStatus(is_connected) => {
                    self.is_war_thunder_connected = is_connected;
                    if !is_connected { self.current_wt_indicators = None; } // Сбрасываем индикаторы при отключении
                }
                UpdateFromAsyncTasks::ButtplugConnected => {
                    self.is_buttplug_connected = true;
                    self.add_log_message("Успешно подключено к Buttplug серверу.".to_string());
                }
                UpdateFromAsyncTasks::ButtplugDisconnected => {
                    self.is_buttplug_connected = false;
                    self.buttplug_devices.clear();
                    self.selected_device_index = None;
                    self.add_log_message("Отключено от Buttplug сервера.".to_string());
                }
                UpdateFromAsyncTasks::ButtplugDeviceFound(device_arc) => {
                    // Проверяем, нет ли уже такого устройства по адресу (на всякий случай)
                    if !self.buttplug_devices.iter().any(|d| d.address() == device_arc.address()) {
                        self.add_log_message(format!("Найдено устройство Buttplug: {} (Функции: {:?})", device_arc.name(), device_arc.allowed_messages()));
                        self.buttplug_devices.push(device_arc);
                        if self.selected_device_index.is_none() && !self.buttplug_devices.is_empty() {
                            self.selected_device_index = Some(0); // Выбираем первое по умолчанию
                        }
                    }
                }
                UpdateFromAsyncTasks::ButtplugDeviceLost(device_arc) => {
                    self.add_log_message(format!("Устройство Buttplug потеряно: {}", device_arc.name()));
                    self.buttplug_devices.retain(|d| d.address() != device_arc.address());
                    if let Some(selected_idx) = self.selected_device_index {
                        if selected_idx >= self.buttplug_devices.len() {
                            self.selected_device_index = if self.buttplug_devices.is_empty() { None } else { Some(0) };
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
        // Сначала обрабатываем все входящие сообщения от асинхронных задач
        self.handle_incoming_updates();

        egui::TopBottomPanel::top("top_panel").show(context, |ui| {
            egui::menu::bar(ui, |ui| {
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
                        _frame.close();
                    }
                });
                ui.menu_button("Управление", |ui| {
                    if ui.checkbox(&mut self.is_processing_enabled, "Включить обработку событий WT").changed() {
                        if self.is_processing_enabled {
                            self.add_log_message("Обработка событий War Thunder включена.".to_string());
                            // Команда на старт, если нужно что-то специально запустить в бэкенде
                            let _ = self.command_sender.try_send(CommandToAsyncTasks::StartProcessing);
                        } else {
                            self.add_log_message("Обработка событий War Thunder выключена.".to_string());
                            // Команда на стоп
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

            // --- Панель статуса ---
            ui.collapsing("Статус", |ui| {
                ui.horizontal(|ui| {
                    ui.label("War Thunder API:");
                    ui.label(if self.is_war_thunder_connected { "ПОДКЛЮЧЕНО" } else { "ОТКЛЮЧЕНО" })
                        .text_color(if self.is_war_thunder_connected { egui::Color32::GREEN } else { egui::Color32::RED });
                });
                 ui.horizontal(|ui| {
                    ui.label("Buttplug сервер:");
                    ui.label(if self.is_buttplug_connected { "ПОДКЛЮЧЕНО" } else { "ОТКЛЮЧЕНО" })
                        .text_color(if self.is_buttplug_connected { egui::Color32::GREEN } else { egui::Color32::RED });
                });
                if self.is_buttplug_connected && !self.buttplug_devices.is_empty() {
                    ui.label("Подключенные устройства Buttplug:");
                    for (idx, device) in self.buttplug_devices.iter().enumerate() {
                        ui.selectable_value(&mut self.selected_device_index, Some(idx), format!("{}: {} (Адрес: {:?})", idx, device.name(), device.address()));
                    }
                    if let Some(selected_idx) = self.selected_device_index {
                         if ui.button("Тест вибрации выбранного").clicked() {
                             let _ = self.command_sender.try_send(CommandToAsyncTasks::VibrateDevice{device_index: selected_idx, speed: 0.5});
                         }
                         if ui.button("Стоп выбранного").clicked() {
                             let _ = self.command_sender.try_send(CommandToAsyncTasks::StopDevice(selected_idx));
                         }
                    }

                } else if self.is_buttplug_connected {
                     ui.label("Устройства Buttplug не найдены. Попробуйте сканировать.");
                }
            });
            ui.separator();

            // --- Данные War Thunder (простой вывод) ---
            ui.collapsing("Данные War Thunder (Live)", |ui| {
                if let Some(indicators) = &self.current_wt_indicators {
                    // TODO: Сделать красивый вывод нужных полей
                    ui.label(format!("Тип техники: {:?}", indicators.vehicle_type.as_deref().unwrap_or("N/A")));
                    ui.label(format!("Скорость: {:.2}", indicators.speed.unwrap_or(0.0)));
                    ui.label(format!("Здоровье: {:.2}%", indicators.health_percentage.unwrap_or(0.0)));
                    // ... и так далее
                } else {
                    ui.label("Нет данных от War Thunder.");
                }
            });
            ui.separator();

            // --- Редактор конфигурации (очень базовый) ---
            ui.collapsing("Конфигурация действий", |ui| {
                ui.label(format!("Интервал опроса WT (мс): {}", self.settings.polling_interval_milliseconds)); // Пока только отображение
                ui.label(format!("Адрес сервера Buttplug: {}", self.settings.buttplug_server_address));     // Пока только отображение

                ui.separator();
                ui.label("Действия на события:");
                let mut action_to_delete_index: Option<usize> = None;
                for (index, event_action) in self.settings.event_actions.iter_mut().enumerate() {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut event_action.enabled, "");
                            ui.text_edit_singleline(&mut event_action.name);
                        });
                        // TODO: Добавить редактирование условий и самого действия
                        ui.label(format!("  Действие: {:?}, Интенсивность: {:.2}, Длительность: {} мс",
                            event_action.device_action.action_type,
                            event_action.device_action.intensity,
                            event_action.device_action.duration_milliseconds
                        ));
                        if ui.button("Удалить").small().clicked() {
                            action_to_delete_index = Some(index);
                        }
                    });
                }
                if let Some(index) = action_to_delete_index {
                    self.settings.event_actions.remove(index);
                    self.add_log_message(format!("Действие #{} удалено. Не забудьте сохранить конфигурацию.", index));
                     // Отправляем обновленные настройки в бэкенд
                    let _ = self.command_sender.try_send(CommandToAsyncTasks::UpdateApplicationSettings(self.settings.clone()));
                }

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
                    ui.add(egui::DragValue::new(&mut self.config_editor_new_event_duration).speed(10.0));
                });

                if ui.button("Добавить действие вибрации").clicked() {
                    let new_action = EventActionSetting {
                        name: self.config_editor_new_event_name.clone(),
                        enabled: true,
                        device_action: DeviceAction {
                            action_type: DeviceActionType::Vibrate,
                            intensity: self.config_editor_new_event_intensity,
                            duration_milliseconds: self.config_editor_new_event_duration,
                        }
                    };
                    self.settings.event_actions.push(new_action);
                    self.add_log_message("Новое действие добавлено. Не забудьте сохранить конфигурацию.".to_string());
                    // Отправляем обновленные настройки в бэкенд
                    let _ = self.command_sender.try_send(CommandToAsyncTasks::UpdateApplicationSettings(self.settings.clone()));
                }
                // TODO: Добавить полноценный редактор событий, условий и действий.
            });
            ui.separator();

            // --- Панель логов ---
            ui.collapsing("Логи", |ui| {
                egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                    for msg in &self.log_messages {
                        ui.label(msg);
                    }
                });
            });
        });

        // Запрашиваем перерисовку для анимации или если данные пришли асинхронно
        context.request_repaint_after(std::time::Duration::from_millis(100)); // Ограничиваем FPS GUI
    }

    // Вызывается при закрытии приложения, можно использовать для сохранения состояния
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        // eframe::set_value(storage, eframe::APP_KEY, &self.settings); // Пример сохранения настроек через eframe storage
        // Но мы используем свой configuration_manager для сохранения в файл
        match configuration_manager::save_configuration(&self.settings) {
            Ok(_) => self.add_log_message("Конфигурация автоматически сохранена при выходе.".to_string()),
            Err(e) => self.add_log_message(format!("Ошибка автосохранения конфигурации: {}", e)),
        }
        // Важно: убедиться, что асинхронные задачи корректно завершаются.
        // Можно послать им сигнал StopProcessing и подождать или просто дать им завершиться
        // при закрытии каналов, если они это обрабатывают.
        let _ = self.command_sender.try_send(CommandToAsyncTasks::StopProcessing);
        let _ = self.command_sender.try_send(CommandToAsyncTasks::DisconnectButtplug);

        // Дать немного времени асинхронным задачам на завершение (не идеально, но для примера)
        // std::thread::sleep(std::time::Duration::from_millis(500));
        // В реальном приложении это лучше делать через join handles или более сложные механизмы graceful shutdown.
    }
}