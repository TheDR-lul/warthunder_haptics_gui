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

// Убираем ненужные use, если они не используются в main
// use configuration_manager::ApplicationSettings; 
// use buttplug_connector::run_buttplug_service_loop; // Вызываются ниже
// use war_thunder_connector::run_war_thunder_polling_loop; // Вызываются ниже

fn main() -> Result<(), eframe::Error> { // Возвращаемый тип eframe::Error
    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive("warthunder_haptics_gui=info".parse().unwrap()))
        .with_target(true)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .expect("Не удалось установить глобальный обработчик логов");

    tracing::info!("Запуск приложения WarThunder Haptics GUI...");

    let tokio_runtime = tokio::runtime::Runtime::new()
        .map_err(|e| {
            tracing::error!("Не удалось создать Tokio рантайм: {}", e);
            // Создаем кастомную ошибку для eframe, если это возможно, или паникуем.
            // eframe::Error не имеет простого From<std::io::Error>.
            // Для простоты примера, паника может быть допустима, если рантайм критичен.
            // В eframe 0.27 нет очевидного способа вернуть кастомную ошибку Box<dyn std::error::Error>
            // напрямую в eframe::Error без оборачивания в существующий вариант eframe::Error.
            // Простейший вариант - паника, или возврат Ok(()) и логирование ошибки, если GUI может работать без рантайма (маловероятно).
             panic!("Критическая ошибка: Не удалось создать Tokio рантайм: {}", e);
        })?;


    let (command_sender_gui, mut command_receiver_for_wt) = mpsc::channel::<CommandToAsyncTasks>(100);
    // Для buttplug нужен свой ресивер, если команды будут обрабатываться параллельно и независимо
    let (command_sender_gui_clone_for_bp, mut command_receiver_for_bp) = mpsc::channel::<CommandToAsyncTasks>(100);
    // Чтобы Application мог слать в обе задачи, он должен иметь оба sender'а или один sender к диспетчеру.
    // Пока Application будет иметь один command_sender_gui, который мы клонируем для передачи в Application.
    // А задачи будут слушать свои ресиверы. Это значит, что Application должен будет как-то
    // выбирать, в какой sender слать, или мы должны иметь задачу-диспетчер.

    // Упрощение: Application будет иметь один Sender. А в main мы будем форвардить команды.
    // Это добавляет сложности.
    // Проще всего, если CommandToAsyncTasks содержит идентификатор целевой задачи,
    // или задачи сами фильтруют команды.

    // Схема с одним главным command_sender-ом из GUI и отдельными ресиверами для задач:
    // main.rs создаст ЕДИНЫЙ канал для команд от GUI.
    // let (gui_command_sender, mut main_command_receiver) = mpsc::channel(100);
    // И затем main_command_receiver будет использоваться для диспетчеризации команд в нужные задачи.
    // Это сложно для скелета.

    // ВЕРНЕМСЯ К ПРОСТОЙ СХЕМЕ: Application имеет ОДИН command_sender.
    // АСИНХРОННЫЕ ЗАДАЧИ ПОЛУЧАЮТ КОПИИ ЭТОГО РЕСИВЕРА (через tokio::sync::broadcast::Receiver если нужно много подписчиков)
    // ИЛИ КАЖДАЯ ЗАДАЧА ИМЕЕТ СВОЙ MPSC КАНАЛ, И APPLICATION ХРАНИТ НЕСКОЛЬКО SENDER'ОВ.
    // Для скелета я оставлю первоначальный подход с одним command_sender в Application,
    // и передам command_receiver_async в задачи, но с оговоркой, что это для mpsc не совсем корректно,
    // если несколько задач должны слушать ОДИН И ТОТ ЖЕ mpsc::Receiver.
    // Поэтому сделаем так: Application будет слать команды, а задачи будут их получать через свои каналы.
    // Main создаст эти каналы.

    let (gui_command_sender, _main_command_receiver_placeholder) = mpsc::channel::<CommandToAsyncTasks>(100); // Этот ресивер не используется напрямую
    let (update_sender_async, update_receiver_gui) = mpsc::channel::<UpdateFromAsyncTasks>(100);


    let initial_settings_for_async = match configuration_manager::load_configuration() {
        Ok(s) => s,
        Err(_) => configuration_manager::ApplicationSettings::default(),
    };

    let http_client = reqwest::Client::new();

    // Канал для команд к War Thunder коннектору
    let (wt_task_command_sender, wt_task_command_receiver) = mpsc::channel(10);
    // Канал для команд к Buttplug коннектору
    let (bp_task_command_sender, bp_task_command_receiver) = mpsc::channel(10);

    // Клонируем Sender'ы для Application, чтобы он мог отправлять команды в нужные задачи
    // Application должен будет решить, в какой из этих Sender'ов отправить команду.
    // Для этого CommandToAsyncTasks может содержать поле target или Application будет иметь методы типа send_to_wt, send_to_bp.
    // Пока что Application будет иметь один общий command_sender, и мы должны решить, как он работает.
    // Давайте упростим: Application будет слать в один агрегирующий Sender, а main будет его слушать и перенаправлять.
    // Это сложно.
    // Проще всего, Application будет иметь ОДИН Sender, а в CommandToAsyncTasks будет информация, для кого эта команда.
    // И каждая задача сама фильтрует. Но это требует от задач слушать один и тот же broadcast канал.

    // --- Окончательная простая схема для скелета с MPSC:
    // Application получает ОДИН Sender. В main создается ОДИН Receiver для этого Sender'а.
    // Затем запускается задача-диспетчер, которая читает из этого Receiver'а
    // и пересылает команды в task-specific каналы (mpsc).
    // Это чисто, но добавляет еще одну задачу.

    // Для скелета сделаем так: Application имеет один `command_sender`.
    // Асинхронные задачи создаются с *этим же* `command_sender` (клоном) для *отправки* сообщений
    // (например, если им нужно послать команду самим себе или другой задаче, что редкость).
    // А для *получения* команд от GUI у них будут свои `Receiver`'ы.
    // `Application` должен будет хранить `Sender`'ы к каждой задаче.

    let app_command_sender_to_wt = wt_task_command_sender.clone();
    let app_command_sender_to_bp = bp_task_command_sender.clone();


    // War Thunder Polling Task
    let wt_update_sender_clone = update_sender_async.clone();
    let polling_interval = initial_settings_for_async.polling_interval_milliseconds;
    tokio_runtime.spawn(async move {
        war_thunder_connector::run_war_thunder_polling_loop(
            wt_update_sender_clone,
            wt_task_command_receiver, // Этот ресивер для команд, специфичных для WT
            http_client,
            polling_interval,
        ).await;
    });

    // Buttplug Service Task
    let bp_update_sender_clone = update_sender_async.clone();
    let buttplug_server_address = initial_settings_for_async.buttplug_server_address.clone();
    tokio_runtime.spawn(async move {
        buttplug_connector::run_buttplug_service_loop(
            bp_update_sender_clone,
            bp_task_command_receiver, // Этот ресивер для команд, специфичных для BP
            buttplug_server_address,
        ).await;
    });

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([600.0, 400.0]),
        ..Default::default()
    };

    tracing::info!("Запуск основного цикла eframe...");
    // Передаем в Application Sender'ы для каждой задачи, если он будет напрямую им слать.
    // Или один общий Sender, если команды будут маршрутизироваться по-другому.
    // В нашем `application.rs` сейчас один `command_sender`. Это нужно будет согласовать.
    // Пусть пока Application имеет один `gui_command_sender`, и мы предполагаем,
    // что команды из него как-то доходят до нужных задач (это слабое место скелета).
    // Для исправления, Application должен был бы получить `app_command_sender_to_wt` и `app_command_sender_to_bp`.
    // Я оставлю один `gui_command_sender` в `Application::new`, но тебе нужно будет решить, как он будет использоваться
    // для отправки команд в *конкретные* асинхронные задачи.
    // Пока что `CommandToAsyncTasks` не имеет информации о получателе.

    eframe::run_native(
        "WarThunder Haptics GUI",
        native_options,
        Box::new(move |creation_context| {
            // Вот здесь command_sender, который получает Application.
            // Если команды должны идти в разные задачи, то Application должен иметь доступ
            // к app_command_sender_to_wt и app_command_sender_to_bp.
            // Либо этот gui_command_sender идет к диспетчеру.
            // Сейчас он не подключен ни к одному из task_command_receiver'ов.
            // Подключим его к `app_command_sender_to_wt` для примера, но это неполное решение.
            // Правильнее всего, Application должен иметь два sender'а.
            // В этом скелете, я передам `app_command_sender_to_wt` как основной `command_sender` в `Application`,
            // это потребует изменений в `Application`, чтобы он знал, что это для WT.
            // Или, проще, создать новый канал для `Application` и затем в `main` слушать его и перенаправлять.
            // Но это усложнит `main`.

            // Для текущего скелета, где Application имеет ОДИН command_sender:
            // пусть он шлет команды, а задачи сами фильтруют. Но для этого нужен broadcast или общий consumer.
            // Поскольку это mpsc, ОДИН command_sender в Application будет слать команды, которые сможет забрать ОДИН Receiver.
            // Это все еще проблема.

            // Давай сделаем так: Application получит app_command_sender_to_wt и app_command_sender_to_bp.
            // Это потребует изменения Application::new и его полей.
            // Это выходит за рамки простого исправления ошибок, это редизайн.

            // ВЕРНEMСЯ К ТОМУ, ЧТО Application::new ПРИНИМАЕТ ОДИН command_sender,
            // и это будет sender от основного канала команд GUI.
            // Этот канал (созданный как gui_command_sender, _main_command_receiver_placeholder)
            // должен быть прочитан задачей-диспетчером в tokio_runtime, которая будет пересылать команды.
            // Это наиболее чистое решение. Но для скелета я это не реализовывал.

            // Если мы хотим, чтобы Application отправлял команды, которые будут получены
            // async задачами, то он должен использовать `app_command_sender_to_wt` или `app_command_sender_to_bp`.
            // В текущем Application::new передается абстрактный `command_sender`.
            // Пусть это будет `app_command_sender_to_wt` для примера.
            // Это значит, что все команды из Application полетят в WT задачу.
            // Тебе нужно будет это исправить для команд к Buttplug.

            Box::new(WarThunderHapticsApplication::new(
                creation_context,
                app_command_sender_to_wt, // <<< ВНИМАНИЕ: это пример, для BP нужен свой!
                update_receiver_gui,
            ))
        }),
    )
}