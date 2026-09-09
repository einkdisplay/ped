mod config;
mod control;
mod device;
mod entropy;
mod display;
mod kindle_lifecycle;
mod runtime;
mod static_server;

use dpi::PhysicalSize;
use embedder_traits::user_contents::UserScript;
use servo::{
    CpuRenderingContext, KindleApiResult, KindleRefreshRequest, KindleWaveform, RenderingContext,
    RgbaImage, ServoBuilder, UserContentManager, WebView, WebViewBuilder, WebViewDelegate,
};
use servo_base::generic_channel::GenericCallback;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::mem::zeroed;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use config::Config;
use control::{Command, ControlServer};
use display::{rotate_to_native, GrayFrame, Orientation, Rect, Scheduler, Waveform};
use kindle_lifecycle::{LifecycleConfig as RuntimeLifecycleConfig, LifecycleGuard};
use runtime::{RefreshStats, State};
use static_server::StaticFileServer;

#[derive(Default)]
struct Delegate {
    frame_ready: RefCell<Option<WebView>>,
    shutdown_requested: Cell<bool>,
    refresh_requested: Cell<bool>,
    kindle_commands: RefCell<VecDeque<KindleCommand>>,
}

enum KindleCommand {
    SetAutoRefresh {
        enabled: bool,
        interval_ms: u32,
    },
    Refresh {
        request: KindleRefreshRequest,
        callback: GenericCallback<KindleApiResult>,
    },
    Battery {
        callback: GenericCallback<KindleApiResult>,
    },
    Network {
        callback: GenericCallback<KindleApiResult>,
    },
}

struct PendingApiRefresh {
    region: Option<Rect>,
    waveform: Waveform,
    flashing: bool,
    full: bool,
    callback: GenericCallback<KindleApiResult>,
}

#[derive(Default)]
struct RuntimeFlags {
    wait_for_complete: bool,
    periodic_full_refresh: bool,
    full_refresh_interval: u32,
}

impl WebViewDelegate for Delegate {
    fn notify_new_frame_ready(&self, webview: WebView) {
        webview.paint();
        self.frame_ready.borrow_mut().replace(webview);
    }

    fn kindle_set_auto_refresh(&self, _webview: WebView, enabled: bool, interval_ms: u32) {
        self.kindle_commands
            .borrow_mut()
            .push_back(KindleCommand::SetAutoRefresh {
                enabled,
                interval_ms,
            });
    }

    fn kindle_refresh_now(
        &self,
        _webview: WebView,
        request: KindleRefreshRequest,
        callback: GenericCallback<KindleApiResult>,
    ) {
        self.kindle_commands
            .borrow_mut()
            .push_back(KindleCommand::Refresh { request, callback });
    }

    fn kindle_query_battery(
        &self,
        _webview: WebView,
        callback: GenericCallback<KindleApiResult>,
    ) {
        self.kindle_commands
            .borrow_mut()
            .push_back(KindleCommand::Battery { callback });
    }

    fn kindle_query_network(
        &self,
        _webview: WebView,
        callback: GenericCallback<KindleApiResult>,
    ) {
        self.kindle_commands
            .borrow_mut()
            .push_back(KindleCommand::Network { callback });
    }
}

fn main() {
    // Before Servo/AWS-LC: credit kernel entropy and keep topping it up on Kindle.
    let _entropy_guard = entropy::EntropyGuard::start();
    let shutdown = install_signal_handlers();

    let config_path = std::env::args().skip(1).find_map(|argument| {
        argument
            .strip_prefix("--config=")
            .map(std::path::PathBuf::from)
    });
    let config_path = config_path.or_else(|| {
        let candidate = std::path::PathBuf::from("ped.toml");
        candidate.exists().then_some(candidate)
    });
    let mut config = Config::load(config_path.as_deref()).unwrap_or_else(|error| {
        eprintln!("ped: {error}");
        std::process::exit(2);
    });
    let page_url = config.page_url().unwrap_or_else(|error| {
        eprintln!("ped: {error}");
        std::process::exit(2);
    });
    let orientation = config.orientation().unwrap_or_else(|error| {
        eprintln!("ped: {error}");
        std::process::exit(2);
    });
    let (logical_width, logical_height) = orientation.logical_size();
    println!(
        "ped: orientation {} (logical {}x{}, native panel {}x{})",
        orientation.as_str(),
        logical_width,
        logical_height,
        display::NATIVE_WIDTH,
        display::NATIVE_HEIGHT
    );
    let lifecycle_config = RuntimeLifecycleConfig {
        enabled: config.lifecycle.enabled,
        marker: config.lifecycle.marker.clone(),
        service: config.lifecycle.service.clone(),
        stop_timeout: Duration::from_millis(config.lifecycle.stop_timeout_ms),
        watchdog_timeout: Duration::from_secs(config.lifecycle.watchdog_timeout_s.max(1)),
    };
    if let Err(error) = LifecycleGuard::recover_stale_marker(&lifecycle_config) {
        eprintln!("ped: stale lifecycle recovery failed: {error}");
        std::process::exit(2);
    }
    let _lifecycle = LifecycleGuard::acquire(lifecycle_config).unwrap_or_else(|error| {
        eprintln!("ped: {error}");
        std::process::exit(2);
    });
    let (static_server, page_url) = StaticFileServer::for_page(
        &page_url,
        config.static_server.enabled,
        &config.static_server.bind,
        config.static_server.port,
        &config.static_server.prefix,
    )
    .unwrap_or_else(|error| {
        eprintln!("ped: {error}");
        std::process::exit(2);
    });
    if let Some(server) = &static_server {
        println!("ped: serving local page at {}", server.base_url());
    }

    println!("ped: initializing libservo");
    let servo = ServoBuilder::default().build();
    let context = Rc::new(
        CpuRenderingContext::new(PhysicalSize::new(logical_width, logical_height)).expect("CPU rendering context"),
    );
    context
        .make_current()
        .expect("make software context current");

    let user_content_manager = Rc::new(UserContentManager::new(&servo));
    for script_path in &config.page.script {
        let script = std::fs::read_to_string(script_path).unwrap_or_else(|error| {
            eprintln!(
                "ped: cannot read injected script {}: {error}",
                script_path.display()
            );
            std::process::exit(2);
        });
        user_content_manager
            .add_script(Rc::new(UserScript::new(script, Some(script_path.clone()))));
    }

    let delegate = Rc::new(Delegate::default());
    let webview = WebViewBuilder::new(&servo, context.clone())
        .url(page_url.clone())
        .delegate(delegate.clone())
        .user_content_manager(user_content_manager)
        .build();
    println!("ped: embedded webpage requested");
    let control = ControlServer::bind(
        &config.control.socket,
        config.control.max_request_bytes,
        config.control.queue_capacity,
    )
    .unwrap_or_else(|error| {
        eprintln!("ped: {error}");
        std::process::exit(2);
    });
    println!("ped: control socket {}", config.control.socket.display());

    let screenshot = Rc::new(RefCell::new(None::<RgbaImage>));
    let screenshot_result = screenshot.clone();
    webview.take_screenshot(None, move |result| {
        if let Ok(image) = result {
            screenshot_result.borrow_mut().replace(image);
        }
    });

    let mut scheduler = Scheduler::new(
        config.display.auto_refresh,
        config.display.refresh_interval_ms,
    );
    let mut runtime_state = State::Starting;
    let mut refresh_stats = RefreshStats::default();
    let mut runtime_flags = RuntimeFlags {
        wait_for_complete: config.display.wait_for_complete,
        periodic_full_refresh: config.display.periodic_full_refresh,
        full_refresh_interval: config.display.full_refresh_interval,
    };
    let mut waveform = Waveform::parse(&config.display.waveform).unwrap_or_else(|error| {
        eprintln!("ped: {error}");
        std::process::exit(2);
    });
    let mut pending_api_refresh: Option<PendingApiRefresh> = None;

    let deadline = Instant::now() + Duration::from_secs(30);
    while screenshot.borrow().is_none() && Instant::now() < deadline {
        if shutdown.load(Ordering::Acquire) {
            delegate.request_shutdown();
            break;
        }
        handle_commands(
            &control,
            &webview,
            &delegate,
            &page_url,
            &mut config,
            &mut scheduler,
            &mut runtime_state,
            &refresh_stats,
            &mut runtime_flags,
            &mut waveform,
        );
        drain_kindle_commands(
            &delegate,
            &webview,
            &config,
            &mut scheduler,
            &mut pending_api_refresh,
            &mut waveform,
            logical_width,
            logical_height,
        );
        servo.spin_event_loop();
        thread::sleep(Duration::from_millis(10));
    }
    let image = screenshot
        .borrow_mut()
        .take()
        .expect("timed out waiting for webpage screenshot");
    println!(
        "ped: webpage rendered as {}x{}",
        image.width(),
        image.height()
    );

    let frame = grayscale_frame(&image, logical_width, logical_height);
    if let Err(error) = commit_frame(
        &frame,
        Rect::full(logical_width, logical_height),
        orientation,
        waveform,
        false,
        runtime_flags.wait_for_complete,
    ) {
        eprintln!("ped: initial framebuffer commit failed: {error}");
    }
    runtime_state = State::Running;
    let mut refresh_count = 1_u32;
    let initial_refresh_time = Instant::now();
    let initial_sequence = scheduler.commit(initial_refresh_time);
    refresh_stats.record_success(
        initial_sequence,
        (0, 0, logical_width, logical_height),
        initial_refresh_time,
    );
    let last_frame = Rc::new(RefCell::new(Some(frame)));
    let pending_frame = Rc::new(RefCell::new(None::<GrayFrame>));
    let pending_region = Rc::new(RefCell::new(None::<Rect>));
    let pending_screenshot = Rc::new(RefCell::new(None::<RgbaImage>));

    while !delegate.shutdown_requested.get() {
        if shutdown.load(Ordering::Acquire) {
            runtime_state = State::Stopping;
            delegate.request_shutdown();
        }
        handle_commands(
            &control,
            &webview,
            &delegate,
            &page_url,
            &mut config,
            &mut scheduler,
            &mut runtime_state,
            &refresh_stats,
            &mut runtime_flags,
            &mut waveform,
        );
        drain_kindle_commands(
            &delegate,
            &webview,
            &config,
            &mut scheduler,
            &mut pending_api_refresh,
            &mut waveform,
            logical_width,
            logical_height,
        );
        servo.spin_event_loop();
        if let Some(frame_view) = delegate.frame_ready.borrow_mut().take() {
            let pending_screenshot_result = pending_screenshot.clone();
            frame_view.take_screenshot(None, move |result| {
                if let Ok(image) = result {
                    pending_screenshot_result.borrow_mut().replace(image);
                }
            });
        }
        if delegate.refresh_requested.replace(false) {
            let pending_screenshot_result = pending_screenshot.clone();
            webview.take_screenshot(None, move |result| {
                if let Ok(image) = result {
                    pending_screenshot_result.borrow_mut().replace(image);
                }
            });
        }
        if let Some(image) = pending_screenshot.borrow_mut().take() {
            let frame = grayscale_frame(&image, logical_width, logical_height);
            if let Some(api) = pending_api_refresh.as_ref() {
                let region = if api.full {
                    Rect::full(logical_width, logical_height)
                } else {
                    api.region
                        .unwrap_or_else(|| Rect::full(logical_width, logical_height))
                };
                *pending_region.borrow_mut() = Some(region);
                *pending_frame.borrow_mut() = Some(frame);
                scheduler.request(api.full || api.flashing);
            } else {
                let region = frame.changed_region(last_frame.borrow().as_ref());
                if let Some(region) = region {
                    *pending_region.borrow_mut() = Some(region);
                    *pending_frame.borrow_mut() = Some(frame);
                    scheduler.request(false);
                } else if scheduler.full_requested() {
                    *pending_region.borrow_mut() = Some(Rect::full(logical_width, logical_height));
                    *pending_frame.borrow_mut() = Some(frame);
                }
            }
        }
        if scheduler.due(Instant::now()) {
            if let Some(frame) = pending_frame.borrow_mut().take() {
                let api = pending_api_refresh.take();
                let mut region = pending_region
                    .borrow_mut()
                    .take()
                    .unwrap_or(Rect::full(logical_width, logical_height));
                let mut commit_waveform = waveform;
                let mut flashing = false;
                if let Some(api) = api.as_ref() {
                    commit_waveform = api.waveform;
                    flashing = api.flashing;
                    if api.full || api.flashing {
                        region = Rect::full(logical_width, logical_height);
                    } else if let Some(api_region) = api.region {
                        region = api_region;
                    }
                } else if scheduler.take_full_request() {
                    region = Rect::full(logical_width, logical_height);
                }
                runtime_state = State::Refreshing;
                let commit_result = commit_frame(
                    &frame,
                    region,
                    orientation,
                    commit_waveform,
                    flashing,
                    runtime_flags.wait_for_complete,
                );
                if commit_result.is_ok() {
                    last_frame.borrow_mut().replace(frame);
                    let now = Instant::now();
                    let sequence = scheduler.commit(now);
                    refresh_count += 1;
                    refresh_stats.record_success(
                        sequence,
                        (region.left, region.top, region.width, region.height),
                        now,
                    );
                    if let Some(api) = api {
                        let _ = api.callback.send(KindleApiResult::Ok);
                    }
                    runtime_state = State::Running;
                    if runtime_flags.periodic_full_refresh
                        && refresh_count >= runtime_flags.full_refresh_interval
                    {
                        refresh_count = 0;
                        scheduler.request(true);
                        delegate.request_refresh();
                    }
                } else {
                    refresh_stats.record_failure();
                    if let Some(api) = api {
                        let message = commit_result.err().unwrap_or_else(|| "commit failed".into());
                        let _ = api.callback.send(KindleApiResult::Error {
                            code: "commit_failed".into(),
                            message,
                        });
                    }
                    runtime_state = State::Running;
                }
            }
        }
        thread::sleep(Duration::from_millis(10));
    }
    runtime_state = State::Stopped;
    drop(webview);
    drop(context);
    drop(servo);
    drop(static_server);
    println!("ped: clean shutdown");
}

impl Delegate {
    fn request_shutdown(&self) {
        self.shutdown_requested.set(true);
    }

    fn request_refresh(&self) {
        self.refresh_requested.set(true);
    }
}

fn install_signal_handlers() -> Arc<AtomicBool> {
    let flag = Arc::new(AtomicBool::new(false));
    let handler_flag = flag.clone();
    ctrlc_install(move || {
        handler_flag.store(true, Ordering::Release);
    });
    flag
}

fn ctrlc_install(handler: impl Fn() + Send + Sync + 'static) {
    use std::sync::OnceLock;
    static HANDLER: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
    let _ = HANDLER.set(Box::new(handler));
    unsafe extern "C" fn on_signal(_: libc::c_int) {
        if let Some(handler) = HANDLER.get() {
            handler();
        }
    }
    unsafe {
        libc::signal(libc::SIGINT, on_signal as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, on_signal as *const () as libc::sighandler_t);
    }
}

fn drain_kindle_commands(
    delegate: &Delegate,
    webview: &WebView,
    config: &Config,
    scheduler: &mut Scheduler,
    pending_api_refresh: &mut Option<PendingApiRefresh>,
    default_waveform: &mut Waveform,
    logical_width: u32,
    logical_height: u32,
) {
    while let Some(command) = delegate.kindle_commands.borrow_mut().pop_front() {
        match command {
            KindleCommand::SetAutoRefresh {
                enabled,
                interval_ms,
            } => {
                if !origin_is_trusted(webview, config) {
                    continue;
                }
                scheduler.set_enabled(enabled);
                scheduler.set_interval_ms(u64::from(interval_ms.max(1)));
            }
            KindleCommand::Refresh { request, callback } => {
                if !origin_is_trusted(webview, config) {
                    let _ = callback.send(KindleApiResult::Error {
                        code: "origin_not_trusted".into(),
                        message: "page origin is not trusted for Kindle refresh".into(),
                    });
                    continue;
                }
                if let Some(previous) = pending_api_refresh.take() {
                    let _ = previous.callback.send(KindleApiResult::Error {
                        code: "superseded".into(),
                        message: "a newer Kindle refresh replaced this request".into(),
                    });
                }
                let region = request
                    .regions
                    .iter()
                    .filter_map(|rect| {
                        Rect::from_css(
                            rect.x,
                            rect.y,
                            rect.width,
                            rect.height,
                            logical_width,
                            logical_height,
                        )
                    })
                    .reduce(Rect::union);
                let waveform = match request.waveform {
                    KindleWaveform::Auto => Waveform::Auto,
                    KindleWaveform::Fast => Waveform::Fast,
                    KindleWaveform::Quality => Waveform::Quality,
                };
                *pending_api_refresh = Some(PendingApiRefresh {
                    region,
                    waveform,
                    flashing: request.flashing,
                    full: request.full || region.is_none(),
                    callback,
                });
                scheduler.request(request.full || request.flashing);
                delegate.request_refresh();
                let _ = default_waveform;
            }
            KindleCommand::Battery { callback } => {
                if !origin_is_trusted(webview, config) {
                    let _ = callback.send(KindleApiResult::Error {
                        code: "origin_not_trusted".into(),
                        message: "page origin is not trusted for battery telemetry".into(),
                    });
                    continue;
                }
                let response = match device::read_battery() {
                    Ok(info) => KindleApiResult::Battery {
                        percentage: info.percentage,
                        charging: info.charging,
                    },
                    Err(message) => KindleApiResult::Error {
                        code: "unavailable".into(),
                        message,
                    },
                };
                let _ = callback.send(response);
            }
            KindleCommand::Network { callback } => {
                if !origin_is_trusted(webview, config) {
                    let _ = callback.send(KindleApiResult::Error {
                        code: "origin_not_trusted".into(),
                        message: "page origin is not trusted for network telemetry".into(),
                    });
                    continue;
                }
                let response = match device::read_network() {
                    Ok(info) => KindleApiResult::Network {
                        connected: info.connected,
                        ssid: info.ssid,
                        airplane_mode: info.airplane_mode,
                        ip_address: info.ip_address,
                    },
                    Err(message) => KindleApiResult::Error {
                        code: "unavailable".into(),
                        message,
                    },
                };
                let _ = callback.send(response);
            }
        }
    }
}

fn origin_is_trusted(webview: &WebView, config: &Config) -> bool {
    match webview.url() {
        Some(url) => config.is_origin_trusted(&url),
        None => false,
    }
}

fn handle_commands(
    control: &ControlServer,
    webview: &WebView,
    delegate: &Delegate,
    initial_url: &url::Url,
    config: &mut Config,
    scheduler: &mut Scheduler,
    runtime_state: &mut State,
    refresh_stats: &RefreshStats,
    runtime_flags: &mut RuntimeFlags,
    waveform: &mut Waveform,
) {
    while let Some(command) = control.try_recv() {
        handle_command(
            command,
            webview,
            delegate,
            initial_url,
            config,
            scheduler,
            runtime_state,
            refresh_stats,
            runtime_flags,
            waveform,
        );
    }
}

fn handle_command(
    command: Command,
    webview: &WebView,
    delegate: &Delegate,
    initial_url: &url::Url,
    config: &mut Config,
    scheduler: &mut Scheduler,
    runtime_state: &mut State,
    refresh_stats: &RefreshStats,
    runtime_flags: &mut RuntimeFlags,
    waveform: &mut Waveform,
) {
    let id = command.request.id;
    let max_script_bytes = config.control.max_script_bytes;
    let max_event_detail_bytes = config.control.max_event_detail_bytes;
    let response = match command.request.method.as_str() {
        "status" => {
            let last_refresh_ms = refresh_stats.last_refresh.map(|at| {
                // Best-effort conversion using process uptime approximation.
                let elapsed = at.elapsed().as_millis() as i64;
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|value| value.as_millis() as i64)
                    .unwrap_or(0);
                now.saturating_sub(elapsed)
            });
            control::ok(
                id,
                serde_json::json!({
                    "url": webview.url().map(|url| url.to_string()),
                    "loadStatus": format!("{:?}", webview.load_status()),
                    "state": runtime_state.as_str(),
                    "autoRefresh": scheduler.enabled(),
                    "refreshIntervalMs": scheduler.interval_ms(),
                    "waveform": match *waveform {
                        Waveform::Auto => "auto",
                        Waveform::Fast => "fast",
                        Waveform::Quality => "quality",
                    },
                    "orientation": config.display.orientation,
                    "refresh": {
                        "sequence": refresh_stats.sequence,
                        "successful": refresh_stats.successful,
                        "failed": refresh_stats.failed,
                        "lastRegion": refresh_stats.last_region,
                        "lastRefreshEpochMs": last_refresh_ms,
                    }
                }),
            )
        }
        "open" => match command
            .request
            .url
            .as_deref()
            .and_then(|url| url::Url::parse(url).ok())
        {
            Some(url) if config.is_url_allowed(&url, initial_url) => {
                webview.load(url);
                control::ok(id, serde_json::json!({"accepted": true}))
            }
            Some(_) => control::error(
                id,
                "url_not_allowed",
                "URL is not in the configured allowlist",
            ),
            None => control::error(id, "invalid_url", "open requires a valid URL"),
        },
        "refresh" => {
            delegate.request_refresh();
            scheduler.request(command.request.force);
            *runtime_state = State::Running;
            control::ok(
                id,
                serde_json::json!({"accepted": true, "sequence": scheduler.sequence(), "force": command.request.force}),
            )
        }
        "reload-config" => match reload_config(config, scheduler, runtime_flags, waveform) {
            Ok(report) if report.restart_required => control::error(
                id,
                "restart_required",
                report
                    .messages
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "restart required".to_owned()),
            ),
            Ok(report) => control::ok(
                id,
                serde_json::json!({
                    "applied": report.applied,
                    "restartRequired": report.restart_required,
                    "messages": report.messages,
                    "autoRefresh": scheduler.enabled(),
                    "refreshIntervalMs": scheduler.interval_ms(),
                }),
            ),
            Err(message) => control::error(id, "reload_failed", message),
        },
        "eval" => {
            let Some(script) = command.request.script.clone() else {
                return send_error(command, "missing_script", "eval requires script");
            };
            if script.len() > max_script_bytes {
                return send_error(
                    command,
                    "script_too_large",
                    "script exceeds configured limit",
                );
            }
            let reply = command.reply;
            webview.evaluate_javascript(script, move |result| {
                let response = match result {
                    Ok(value) => control::ok(
                        id,
                        serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
                    ),
                    Err(error) => control::error(id, "javascript_error", format!("{error:?}")),
                };
                let _ = reply.send(response);
            });
            return;
        }
        "emit" => {
            let Some(event_type) = command.request.event_type.clone() else {
                return send_error(command, "missing_event_type", "emit requires type");
            };
            let event_type = match serde_json::to_string(&event_type) {
                Ok(value) => value,
                Err(error) => return send_error(command, "invalid_event_type", error.to_string()),
            };
            let detail = serde_json::to_string(
                &command
                    .request
                    .detail
                    .clone()
                    .unwrap_or(serde_json::Value::Null),
            )
            .unwrap_or_else(|_| "null".to_owned());
            if detail.len() > max_event_detail_bytes {
                return send_error(
                    command,
                    "event_detail_too_large",
                    "event detail exceeds configured limit",
                );
            }
            let script = format!(
                "window.dispatchEvent(new CustomEvent({}, {{detail: {}}}));",
                event_type, detail
            );
            let reply = command.reply;
            webview.evaluate_javascript(script, move |result| {
                let response = match result {
                    Ok(_) => control::ok(id, serde_json::json!({"accepted": true})),
                    Err(error) => control::error(id, "javascript_error", format!("{error:?}")),
                };
                let _ = reply.send(response);
            });
            return;
        }
        "stop" => {
            *runtime_state = State::Stopping;
            delegate.request_shutdown();
            control::ok(id, serde_json::json!({"accepted": true}))
        }
        _ => control::error(id, "unknown_method", "unknown control method"),
    };
    let _ = command.reply.send(response);
}

fn reload_config(
    config: &mut Config,
    scheduler: &mut Scheduler,
    runtime_flags: &mut RuntimeFlags,
    waveform: &mut Waveform,
) -> Result<config::ReloadReport, String> {
    let Some(path) = config.source_path.clone() else {
        return Err("PED was started without a config file path".to_owned());
    };
    let next = Config::load(Some(&path))?;
    let report = config.apply_runtime_reload(next)?;
    if report.applied {
        scheduler.set_enabled(config.display.auto_refresh);
        scheduler.set_interval_ms(config.display.refresh_interval_ms);
        *waveform = Waveform::parse(&config.display.waveform)?;
        runtime_flags.wait_for_complete = config.display.wait_for_complete;
        runtime_flags.periodic_full_refresh = config.display.periodic_full_refresh;
        runtime_flags.full_refresh_interval = config.display.full_refresh_interval;
    }
    Ok(report)
}

fn send_error(command: Command, code: &'static str, message: impl Into<String>) {
    let id = command.request.id;
    let _ = command.reply.send(control::error(id, code, message));
}

fn grayscale_frame(image: &RgbaImage, logical_width: u32, logical_height: u32) -> GrayFrame {
    assert_eq!(
        (image.width(), image.height()),
        (logical_width, logical_height)
    );
    let pixels = image
        .pixels()
        .map(|pixel| {
            let alpha = u16::from(pixel[3]);
            let red = (u16::from(pixel[0]) * alpha + 255 * (255 - alpha)) / 255;
            let green = (u16::from(pixel[1]) * alpha + 255 * (255 - alpha)) / 255;
            let blue = (u16::from(pixel[2]) * alpha + 255 * (255 - alpha)) / 255;
            ((77 * red + 150 * green + 29 * blue) / 256) as u8
        })
        .collect();
    GrayFrame::new(logical_width, logical_height, pixels).expect("valid grayscale frame")
}

fn commit_frame(
    frame: &GrayFrame,
    region: Rect,
    orientation: Orientation,
    waveform: Waveform,
    flashing: bool,
    wait_for_complete: bool,
) -> Result<(), String> {
    // Rotate logical page pixels into native panel coordinates when needed.
    let native_frame = rotate_to_native(frame, orientation)?;
    let native_region = orientation.transform_rect(region);
    let pixels = &native_frame.pixels;

    let mut config: fbink_sys::FBInkConfig = unsafe { zeroed() };
    config.is_quiet = true;
    config.ignore_alpha = true;
    config.no_refresh = true;
    config.is_flashing = flashing;
    config.wfm_mode = match waveform {
        Waveform::Auto => 0,
        Waveform::Fast => 1,
        Waveform::Quality => 2,
    };

    let fbfd = unsafe { fbink_sys::fbink_open() };
    if fbfd < 0 {
        return Err(format!("fbink_open failed: {fbfd}"));
    }
    let init = unsafe { fbink_sys::fbink_init(fbfd, &config) };
    if init != 0 {
        unsafe { fbink_sys::fbink_close(fbfd) };
        return Err(format!("fbink_init failed: {init}"));
    }
    println!(
        "ped: refreshing region {}x{}+{}+{} (logical {}x{}+{}+{}) orientation={} waveform={:?} flashing={flashing}",
        native_region.width,
        native_region.height,
        native_region.left,
        native_region.top,
        region.width,
        region.height,
        region.left,
        region.top,
        orientation.as_str(),
        waveform
    );
    // For flashing commits, allow FBInk to refresh with the configured flash flag.
    if flashing {
        config.no_refresh = false;
    }
    let printed = unsafe {
        fbink_sys::fbink_print_raw_data(
            fbfd,
            pixels.as_ptr(),
            native_frame.width as i32,
            native_frame.height as i32,
            pixels.len(),
            0,
            0,
            &config,
        )
    };
    if printed != 0 {
        unsafe { fbink_sys::fbink_close(fbfd) };
        return Err(format!("fbink_print_raw_data failed: {printed}"));
    }
    if !flashing {
        let rect = fbink_sys::FBInkRect {
            left: native_region.left as u16,
            top: native_region.top as u16,
            width: native_region.width as u16,
            height: native_region.height as u16,
        };
        let refreshed = unsafe { fbink_sys::fbink_refresh_rect(fbfd, &rect, &config) };
        if refreshed != 0 {
            unsafe { fbink_sys::fbink_close(fbfd) };
            return Err(format!("fbink_refresh_rect failed: {refreshed}"));
        }
    }
    if wait_for_complete {
        let waited = unsafe { fbink_sys::fbink_wait_for_complete(fbfd, 0) };
        if waited != 0 {
            unsafe { fbink_sys::fbink_close(fbfd) };
            return Err(format!("fbink_wait_for_complete failed: {waited}"));
        }
    }
    let close = unsafe { fbink_sys::fbink_close(fbfd) };
    if close != 0 {
        return Err(format!("fbink_close failed: {close}"));
    }
    Ok(())
}
