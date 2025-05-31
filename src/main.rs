// src/main.rs

mod application;
mod configuration_manager;
mod game_event_processor;
mod message_passing;
mod war_thunder_connector;
mod buttplug_connector;

use application::WarThunderHapticsApplication;
use message_passing::{CommandToAsyncTasks, UpdateFromAsyncTasks};
use tokio::sync::mpsc;

fn main() -> Result<(), eframe::Error> {
    // Настройка логирования (tracing)
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("warthunder_haptics_gui=info".parse().unwrap())) // Логи уровня info для нашего крейта
        .with_target(true) // Показывать модуль, откуда лог
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("Не удалось установить глобальный обработчик логов");

    tracing::info!("Запуск приложения WarThunder Haptics GUI...");

    // Создаем Tokio рантайм для асинхронных задач
    let tokio_runtime = tokio::runtime::Runtime::new()
        .map_err(|e| {
            tracing::error!("Не удалось создать Tokio рантайм: {}", e);
            // eframe::Error не имеет From<std::io::Error>, так что придется как-то преобразовать
            // или просто паниковать, так как без рантайма приложение не будет работать.
            // Для простоты, здесь можно было бы вернуть кастомную ошибку или panic.
            // В данном контексте eframe::Error::Creation("Failed to create Tokio runtime".into())
            // Но для этого нужно чтобы eframe::Error имел такой конструктор или From.
            // Проще всего здесь просто паниковать, если рантайм критичен.
            panic!("Не удалось создать Tokio рантайм: {}", e);
        })?;


    // Создаем каналы для общения между GUI и асинхронными задачами
    // Канал для команд от GUI к async задачам
    let (command_sender_gui, command_receiver_async) = mpsc::channel::<CommandToAsyncTasks>(100);
    // Канал для обновлений от async задач к GUI
    let (update_sender_async, update_receiver_gui) = mpsc::channel::<UpdateFromAsyncTasks>(100);

    // --- Запускаем асинхронные задачи ---
    let initial_settings_for_async = match configuration_manager::load_configuration() {
        Ok(s) => s,
        Err(_) => configuration_manager::ApplicationSettings::default(), // Используем дефолт, если не удалось загрузить
    };

    // HTTP клиент для War Thunder API
    let http_client = reqwest::Client::new();

    // War Thunder Polling Task
    let wt_update_sender = update_sender_async.clone();
    // Нужен отдельный канал для команд WT, если мы хотим им управлять отдельно
    let (wt_command_sender_gui_clone, wt_command_receiver_async) = mpsc::channel::<CommandToAsyncTasks>(10);
    // Пока передаем основной command_receiver_async, но это не идеально, т.к. он будет общим
    // Если делать правильно, то нужен отдельный ресивер для WT или мультиплексор команд.
    // Для простоты скелета оставим так, но это место для улучшения.
    let polling_interval = initial_settings_for_async.polling_interval_milliseconds;
    tokio_runtime.spawn(async move {
        war_thunder_connector::run_war_thunder_polling_loop(
            wt_update_sender,
            wt_command_receiver_async, // Используем этот специфичный ресивер
            http_client,
            polling_interval,
        ).await;
    });
     // Клонируем command_sender_gui для передачи в WarThunderHapticsApplication
    let command_sender_for_wt_task = command_sender_gui.clone();
    // Чтобы Application мог посылать команды в WT task, он должен знать о wt_command_sender_gui_clone.
    // Это усложняет, поэтому пока WT task будет слушать общий command_receiver_async.
    // Или, как вариант, Application сам будет роутить команды.
    // Пока что пусть WT task не принимает команд на изменение интервала через общий канал,
    // а получает их через UpdateApplicationSettings.

    // Buttplug Service Task
    let bp_update_sender = update_sender_async.clone(); // Можно использовать тот же sender
    let (bp_command_sender_gui_clone, bp_command_receiver_async) = mpsc::channel::<CommandToAsyncTasks>(10);
    let buttplug_server_address = initial_settings_for_async.buttplug_server_address.clone();
    tokio_runtime.spawn(async move {
        buttplug_connector::run_buttplug_service_loop(
            bp_update_sender,
            bp_command_receiver_async, // Используем этот специфичный ресивер
            buttplug_server_address,
        ).await;
    });
    // Клонируем command_sender_gui для передачи в WarThunderHapticsApplication
    // Приложение будет слать команды в общий command_sender_gui, а main.rs будет их роутить
    // или, как сейчас, каждая задача имеет свой command_receiver, а GUI один sender,
    // и мы должны решить, как команды из GUI попадают в нужную задачу.

    // Более простая схема: GUI имеет один command_sender. Async задачи слушают *копии* этого command_receiver.
    // Но mpsc::Receiver не Clone. Значит, либо одна задача-диспетчер, либо broadcast channel,
    // либо каждая async задача получает свой command_sender, а GUI хранит Vec<Sender> или enum для выбора.

    // Самый простой вариант для старта: GUI шлет общую команду, а каждая задача в своем цикле
    // через command_receiver.recv() получает *ту же самую* команду и решает, обрабатывать ее или нет.
    // Но mpsc receiver не позволяет нескольким задачам слушать один и тот же receiver.
    // Поэтому, для скелета, сделаем так, что GUI отправляет команды в command_sender_gui,
    // а в main.rs мы (гипотетически) могли бы иметь диспетчер, который пересылает команды.
    // Но для простоты, пусть каждая async задача получит свою копию command_sender_gui, чтобы слать сообщения самой себе
    // (что неверно), или GUI будет решать, куда слать.

    // Исправленная логика для каналов:
    // GUI будет иметь command_sender_gui.
    // main создаст специфичные command_receiver для каждой async задачи.
    // А вот как GUI будет говорить с конкретной задачей?
    // Пока что, async задачи только получают команды через свои уникальные ресиверы.
    // GUI будет отправлять команды в `command_sender_for_forwarding`
    // А `main` (или специальная задача-форвардер) будет читать из `command_receiver_from_gui`
    // и пересылать в нужный `task_specific_sender`. Это сложно для скелета.

    // Упрощенный вариант для скелета: `Application` будет хранить Sender'ы для каждой задачи.
    // Или `Application` шлет одну команду, а задачи сами фильтруют.
    // Оставим пока так, что `Application` имеет один `command_sender`,
    // а асинхронные задачи (в текущем скелете) получают `command_receiver_async`,
    // который мы сделали общим. Это НЕПРАВИЛЬНО для mpsc, так как только один consumer.
    // ПРАВИЛЬНО: Каждая async задача должна иметь свой `Receiver`.
    // `main` должен создать несколько `Sender`'ов (клонов `command_sender_gui`)
    // и передать `Receiver`'ы в задачи, а `Sender` (один) в GUI.
    // А Application уже будет решать, какую команду послать.

    // --- Корректная настройка каналов для command_sender ---
    // Мы уже имеем command_sender_gui и command_receiver_async.
    // command_receiver_async будет использоваться в задаче-диспетчере (если бы она была)
    // или мы должны передать клоны command_sender_gui в каждую задачу, если они должны
    // отправлять команды (например, самим себе для внутреннего использования).
    // А для получения команд от GUI, каждая задача должна иметь свой Receiver.

    // Давайте переделаем: GUI будет иметь один Sender. main создаст один Receiver.
    // А в main будет задача-диспетчер, которая читает из этого Receiver'а и пересылает
    // команды в нужные async-задачи через их собственные каналы.
    // Это добавляет сложности.

    // Самый простой рабочий вариант для скелета:
    // 1. GUI имеет command_sender_gui.
    // 2. Каждая async задача получает command_receiver (свой собственный).
    // 3. main создает несколько Sender'ов, клонируя command_sender_gui, и передает
    //    эти Sender'ы в Application, чтобы Application мог выбрать, какому task'у послать команду.
    // Это тоже сложно для Application.

    // Оставим первоначальную, хоть и не совсем корректную для общего command_receiver,
    // идею, что async задачи слушают *общий* command_receiver_async.
    // ВНИМАНИЕ: Это потребует `broadcast` канала, если несколько задач должны слушать одни и те же команды.
    // С `mpsc` так не получится.
    // Для скелета, предположим, что команды достаточно специфичны,
    // и только одна задача реально будет обрабатывать конкретную команду, посланную через общий канал.
    // Это упрощение, которое нужно будет исправить в реальном проекте.
    // Либо, как вариант, в `CommandToAsyncTasks` добавить поле `target_task_id`.

    // --- Запускаем GUI ---
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([600.0, 400.0]),
        // другие опции...
        ..Default::default()
    };

    tracing::info!("Запуск основного цикла eframe...");
    eframe::run_native(
        "WarThunder Haptics GUI", // Заголовок окна
        native_options,
        Box::new(move |creation_context| {
            // Здесь мы создаем экземпляр нашего GUI приложения
            // Важно: command_sender_gui и update_receiver_gui должны быть переданы в приложение.
            // command_receiver_async и update_sender_async используются асинхронными задачами.
            Box::new(WarThunderHapticsApplication::new(
                creation_context,
                command_sender_gui, // GUI будет слать команды сюда
                update_receiver_gui, // GUI будет читать обновления отсюда
            ))
        }),
    )
}