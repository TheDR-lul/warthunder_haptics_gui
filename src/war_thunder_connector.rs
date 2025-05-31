// src/war_thunder_connector.rs

use serde::Deserialize;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use reqwest::Client;
use crate::message_passing::{UpdateFromAsyncTasks, CommandToAsyncTasks}; // CommandToAsyncTasks может понадобиться для сигнала остановки или изменения интервала опроса

// Пример структуры для данных из /indicators. Тебе нужно будет ее дополнить на основе реального JSON.
// Используй https://app.quicktype.io/ чтобы сгенерировать структуры из примера JSON.
#[derive(Deserialize, Debug, Clone, Default)]
pub struct WarThunderIndicators {
    #[serde(rename = "type")]
    pub vehicle_type: Option<String>,
    pub speed: Option<f32>,
    pub altitude_10k: Option<f32>, // Пример, если есть такое поле
    #[serde(rename = "RPM throttle")] // Пример с переименованием
    pub rpm_throttle: Option<f32>,
    #[serde(rename = "H, %")]
    pub health_percentage: Option<f32>, // Здоровье в процентах
    // ... добавь сюда все интересующие тебя поля из /indicators
    // Например:
    // pub Gx: Option<f32>,
    // pub Gy: Option<f32>,
    // pub Gz: Option<f32>,
    // pub weapon_active: Option<bool>, // Если есть флаг активного оружия
    // pub shells_count: Option<u32>, // Количество снарядов
}

const WAR_THUNDER_STATE_URL: &str = "http://localhost:8111/state";
const WAR_THUNDER_INDICATORS_URL: &str = "http://localhost:8111/indicators";

pub async fn run_war_thunder_polling_loop(
    gui_update_sender: mpsc::Sender<UpdateFromAsyncTasks>,
    mut command_receiver: mpsc::Receiver<CommandToAsyncTasks>, // Пока не используется, но для будущего
    http_client: Client,
    mut polling_interval_milliseconds: u64,
) {
    let mut last_known_health: Option<f32> = None; // Пример для отслеживания изменений

    loop {
        // Проверяем, не пришла ли команда на изменение интервала или остановку
        // Это пример, как можно было бы обрабатывать команды
        match command_receiver.try_recv() {
            Ok(CommandToAsyncTasks::UpdateApplicationSettings(settings)) => {
                polling_interval_milliseconds = settings.polling_interval_milliseconds;
                let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage(format!("Интервал опроса War Thunder изменен на {} мс", polling_interval_milliseconds))).await;
            }
            Ok(CommandToAsyncTasks::StopProcessing) => {
                 let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage("Остановлен опрос War Thunder.".to_string())).await;
                 let _ = gui_update_sender.send(UpdateFromAsyncTasks::WarThunderConnectionStatus(false)).await;
                break;
            }
            Err(mpsc::error::TryRecvError::Empty) => { /* нет команд, продолжаем */ }
            Err(mpsc::error::TryRecvError::Disconnected) => {
                tracing::warn!("Канал команд для War Thunder коннектора закрыт.");
                break;
            }
            _ => { /* другие команды пока игнорируем */ }
        }


        match http_client.get(WAR_THUNDER_INDICATORS_URL).send().await {
            Ok(response) => {
                if response.status().is_success() {
                    match response.json::<WarThunderIndicators>().await {
                        Ok(indicators) => {
                            // Пример простой логики: если здоровье изменилось
                            if let Some(current_health) = indicators.health_percentage {
                                if let Some(last_health) = last_known_health {
                                    if (current_health - last_health).abs() > 0.01 && current_health < last_health { // Небольшой порог, и здоровье уменьшилось
                                        let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage(format!("Обнаружен урон! Здоровье: {:.2}%", current_health))).await;
                                        // Здесь можно было бы генерировать более специфичное событие,
                                        // но пока просто отправляем все индикаторы
                                    }
                                }
                                last_known_health = Some(current_health);
                            }

                            // Отправляем полные данные в GUI для отображения или дальнейшей обработки
                            if gui_update_sender.send(UpdateFromAsyncTasks::WarThunderIndicatorsUpdate(indicators)).await.is_err() {
                                tracing::error!("Не удалось отправить обновление индикаторов WT в GUI: канал закрыт.");
                                break;
                            }
                             if gui_update_sender.send(UpdateFromAsyncTasks::WarThunderConnectionStatus(true)).await.is_err() {
                                break; // Канал закрыт
                            }
                        }
                        Err(parse_error) => {
                            tracing::error!("Ошибка парсинга JSON от War Thunder Indicators: {}", parse_error);
                            let _ = gui_update_sender.send(UpdateFromAsyncTasks::LogMessage(format!("Ошибка парсинга JSON от WT: {}", parse_error))).await;
                             if gui_update_sender.send(UpdateFromAsyncTasks::WarThunderConnectionStatus(false)).await.is_err() {
                                break;
                            }
                        }
                    }
                } else {
                    // War Thunder API может возвращать 404 или 503 если не в ангаре/бою или API выключено
                    // tracing::warn!("War Thunder API (Indicators) вернул статус: {}", response.status());
                    if gui_update_sender.send(UpdateFromAsyncTasks::WarThunderConnectionStatus(false)).await.is_err() {
                        break; // Канал закрыт
                    }
                }
            }
            Err(request_error) => {
                // Это обычно означает, что игра не запущена или API выключено
                // tracing::debug!("Ошибка подключения к War Thunder Indicators API: {}. Возможно, игра не запущена.", request_error);
                 if gui_update_sender.send(UpdateFromAsyncTasks::WarThunderConnectionStatus(false)).await.is_err() {
                    break; // Канал закрыт
                }
            }
        }
        sleep(Duration::from_millis(polling_interval_milliseconds)).await;
    }
}