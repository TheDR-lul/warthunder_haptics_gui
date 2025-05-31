// src/game_event_processor.rs

use crate::configuration_manager::{ApplicationSettings, EventActionSetting, DeviceAction, DeviceActionType};
use crate::war_thunder_connector::WarThunderIndicators;
use crate::message_passing::CommandToAsyncTasks; // Если мы решим генерировать команды напрямую

// Эта структура будет хранить предыдущее состояние для сравнения
#[derive(Default, Clone)]
pub struct GameStateSnapshot {
    // Добавь сюда поля, которые нужно отслеживать для определения событий "изменение"
    // Например:
    pub last_health_percentage: Option<f32>,
    // pub last_shells_count: Option<u32>,
    // pub was_weapon_active: Option<bool>,
}

// Эта функция будет вызываться при получении новых данных от War Thunder.
// Она сравнивает текущее состояние с предыдущим (если нужно) и с настройками,
// чтобы определить, какие действия нужно выполнить.
// Возвращает вектор команд для Buttplug устройств (пока упрощенно, только одно действие).
pub fn process_war_thunder_data(
    current_indicators: &WarThunderIndicators,
    settings: &ApplicationSettings,
    previous_state: &mut GameStateSnapshot, // mutable для обновления состояния
) -> Vec<DeviceAction> { // Возвращаем список действий, а не команд напрямую
    let mut actions_to_perform: Vec<DeviceAction> = Vec::new();

    for event_action_config in &settings.event_actions {
        if !event_action_config.enabled {
            continue;
        }

        // Здесь должна быть логика определения, сработало ли событие из конфига
        // Это самая сложная часть, требующая внимательного проектирования
        // Как пример, очень упрощенная проверка на "урон"
        // TODO: Реализовать более гибкую систему проверки условий из event_action_config
        // (например, сопоставление полей, порогов, типов сравнения)

        if event_action_config.name.contains("урона") || event_action_config.name.contains("damage") { // Очень грубая проверка по имени
            if let Some(current_health) = current_indicators.health_percentage {
                if let Some(last_health) = previous_state.last_health_percentage {
                    if current_health < last_health && (last_health - current_health) > 0.01 { // Если здоровье уменьшилось
                        tracing::info!("Сработало событие (по здоровью): {}", event_action_config.name);
                        actions_to_perform.push(event_action_config.device_action.clone());
                    }
                }
            }
        } else if event_action_config.name.contains("Выстрел") { // Еще один грубый пример
            // Здесь тебе нужно будет придумать, как определять "выстрел".
            // Например, если у тебя есть поле `shells_fired_this_tick` или
            // если количество снарядов уменьшилось по сравнению с previous_state.
            // Допустим, для примера, мы просто активируем его, если активно какое-то оружие
            // (это неверно, но для иллюстрации)
            // if current_indicators.weapon_active.unwrap_or(false) {
            //    tracing::info!("Сработало событие (по оружию): {}", event_action_config.name);
            //    actions_to_perform.push(event_action_config.device_action.clone());
            // }
        }
        // Добавь другие проверки для других типов событий из твоего конфига
    }

    // Обновляем предыдущее состояние
    previous_state.last_health_percentage = current_indicators.health_percentage;
    // previous_state.last_shells_count = current_indicators.shells_count;
    // ... и так далее для других отслеживаемых полей

    actions_to_perform
}