pub mod input;
pub mod terminal_guard;

use crate::cli::Args;
use crate::config::UiTheme;
use crate::libs::docker::DockerEngine;
use crate::services::ServiceRegistry;
use crate::terminal_colors::TerminalColorMode;
use crate::ui::state::service_picker::ServicePickerOption;
use crate::ui::state::AppUiState;
use crate::ui::MasterUiRenderer;
use crossterm::cursor::SetCursorStyle;
use crossterm::event::Event;
use crossterm::execute;
use input::{InputDispatcher, KeyDispatchContext};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::collections::HashMap;
use std::io::stdout;
use std::sync::Arc;
use std::time::{Duration, Instant};
use terminal_guard::TerminalGuard;
use tokio::sync::mpsc::unbounded_channel;

#[allow(unused)]
pub struct RunOptions {
    pub args: Args,
    pub color_mode: TerminalColorMode,
}

pub async fn run(opts: RunOptions) -> eyre::Result<()> {
    let _term_guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    let docker = Arc::new(DockerEngine::default_local());
    let mut services = ServiceRegistry::new(Arc::clone(&docker), 100_000);
    let mut ui_state = AppUiState::new();
    let theme = UiTheme::default();

    let (status_tx, mut status_rx) = unbounded_channel::<(String, bool, bool)>();
    let (container_list_tx, mut container_list_rx) = unbounded_channel::<Vec<ServicePickerOption>>();

    spawn_background_ingestion(Arc::clone(&docker), services.logging.sender(), status_tx, container_list_tx);

    let mut pending_g = false;
    let mut last_scroll = Instant::now();

    loop {
        while let Ok(options) = container_list_rx.try_recv() {
            merge_service_options(&mut ui_state.picker.options, options);
        }

        while let Ok((id, running, err)) = status_rx.try_recv() {
            if let Some(opt) = ui_state.picker.options.iter_mut().find(|o| o.id == id) {
                opt.is_running = running;
                opt.has_error = err;
            }
        }

        let ref_now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
        let svc_map: HashMap<String, bool> = ui_state.picker.options.iter().map(|o| (o.name.clone(), o.enabled)).collect();

        let new_logs_count = services.sync(&svc_map, ref_now);
        if new_logs_count > 0 && ui_state.sticky {
            let total = services.logging.buffer().visible_count();
            if total > 0 {
                ui_state.cursor_row = total - 1;
                ui_state.adjust_scroll(total);
            }
        }

        let is_input = services.filter.prompt().mode == crate::services::filter::prompt_state::PromptMode::Active || ui_state.picker.is_searching;
        let cursor_shape = if is_input { SetCursorStyle::SteadyBar } else { SetCursorStyle::SteadyBlock };
        let _ = execute!(stdout(), cursor_shape);

        terminal.draw(|f| {
            MasterUiRenderer::render(f, &mut ui_state, &services, &theme, &opts.color_mode);
        })?;

        handle_edge_drag(&mut ui_state, &services, &mut last_scroll);

        if crossterm::event::poll(Duration::from_millis(20))? {
            match crossterm::event::read()? {
                Event::Key(key) => {
                    let ctx = KeyDispatchContext {
                        ui: &mut ui_state,
                        services: &mut services,
                        docker: &docker,
                        pending_g: &mut pending_g,
                    };
                    if InputDispatcher::dispatch_key(key, ctx) {
                        break;
                    }
                }
                Event::Mouse(mouse) => {
                    InputDispatcher::dispatch_mouse(mouse, &mut ui_state, &mut services);
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn merge_service_options(current: &mut Vec<ServicePickerOption>, incoming: Vec<ServicePickerOption>) {
    let existing_states: HashMap<String, bool> = current.iter().map(|o| (o.id.clone(), o.enabled)).collect();
    let mut updated = Vec::with_capacity(incoming.len());

    for mut inc in incoming {
        if let Some(&prev_enabled) = existing_states.get(&inc.id) {
            inc.enabled = prev_enabled;
        }
        updated.push(inc);
    }
    *current = updated;
}

fn handle_edge_drag(ui: &mut AppUiState, services: &ServiceRegistry, last_scroll: &mut Instant) {
    if !ui.is_mouse_dragging || last_scroll.elapsed() < Duration::from_millis(60) {
        return;
    }

    let (_, y) = match ui.last_mouse_pos {
        Some(pos) => pos,
        None => return,
    };

    let total = services.logging.buffer().visible_count();
    if total == 0 {
        return;
    }

    if y < 3 {
        let prev_scroll = ui.scroll_offset;
        ui.drag_scroll_viewport_up(1);
        if ui.scroll_offset != prev_scroll {
            if let Some((_, c)) = ui.mouse_head {
                ui.mouse_head = Some((ui.scroll_offset, c));
            }
        }
        *last_scroll = Instant::now();
    } else if y >= (ui.viewport_height as u16).saturating_sub(3) {
        let prev_scroll = ui.scroll_offset;
        ui.drag_scroll_viewport_down(1, total);
        if ui.scroll_offset != prev_scroll {
            let visible_bottom_row = (ui.scroll_offset + ui.viewport_height).saturating_sub(1).min(total.saturating_sub(1));
            if let Some((_, c)) = ui.mouse_head {
                ui.mouse_head = Some((visible_bottom_row, c));
            }
        }
        *last_scroll = Instant::now();
    }
}

fn spawn_background_ingestion(
    docker: Arc<DockerEngine>,
    log_tx: tokio::sync::mpsc::UnboundedSender<crate::services::container_logging::transformer::ProcessedLogRecord>,
    status_tx: tokio::sync::mpsc::UnboundedSender<(String, bool, bool)>,
    picker_tx: tokio::sync::mpsc::UnboundedSender<Vec<ServicePickerOption>>,
) {
    tokio::spawn(async move {
        if !docker.is_available() {
            let _ = log_tx.send(crate::services::container_logging::transformer::LogTransformer::system_notice("Docker socket /var/run/docker.sock not found."));
            return;
        }

        match docker.containers().list(true).await {
            Ok(containers) => {
                dispatch_picker_options(&containers, &picker_tx);

                for c in &containers {
                    let docker_c = Arc::clone(&docker);
                    let cid = c.id.clone();
                    let name = c.primary_name.clone();
                    let tail = if c.is_running || c.has_error { 1000 } else { 100 };
                    let tx = log_tx.clone();

                    tokio::spawn(async move {
                        let params = crate::services::container_logging::pipeline::LoggingPipelineParams {
                            container_id: &cid,
                            service_name: &name,
                            follow: false,
                            tail: Some(tail),
                        };
                        let _ = crate::services::container_logging::pipeline::LoggingPipeline::run_stream(&docker_c, params, tx).await;
                    });
                }

                for c in &containers {
                    if c.is_running {
                        spawn_container_stream(Arc::clone(&docker), c.id.clone(), c.primary_name.clone(), log_tx.clone());
                    }
                }

                if let Ok(mut sub) = docker.subscribe_events().await {
                    while let Some(evt) = sub.next_event().await {
                        let _ = status_tx.send((evt.container_id.clone(), evt.is_running, evt.has_error));
                        let processed = crate::services::container_logging::transformer::LogTransformer::from_system_event(&evt);
                        let _ = log_tx.send(processed);

                        if evt.is_running {
                            spawn_container_stream(Arc::clone(&docker), evt.container_id.clone(), evt.container_name.clone(), log_tx.clone());
                        }

                        if let Ok(fresh_containers) = docker.containers().list(true).await {
                            dispatch_picker_options(&fresh_containers, &picker_tx);
                        }
                    }
                }
            }
            Err(e) => {
                let _ = log_tx.send(crate::services::container_logging::transformer::LogTransformer::system_notice(&format!("Docker daemon error: {}. Check socket permissions.", e)));
            }
        }
    });
}

fn dispatch_picker_options(
    containers: &[crate::libs::docker::ContainerSummary],
    tx: &tokio::sync::mpsc::UnboundedSender<Vec<ServicePickerOption>>,
) {
    let options: Vec<_> = containers.iter().map(|c| ServicePickerOption {
        id: c.id.clone(),
        name: c.primary_name.clone(),
        is_running: c.is_running,
        has_error: c.has_error,
        enabled: true,
    }).collect();
    let _ = tx.send(options);
}

fn spawn_container_stream(
    docker: Arc<DockerEngine>,
    id: String,
    name: String,
    tx: tokio::sync::mpsc::UnboundedSender<crate::services::container_logging::transformer::ProcessedLogRecord>,
) {
    tokio::spawn(async move {
        let params = crate::services::container_logging::pipeline::LoggingPipelineParams {
            container_id: &id,
            service_name: &name,
            follow: true,
            tail: Some(0),
        };
        let _ = crate::services::container_logging::pipeline::LoggingPipeline::run_stream(&docker, params, tx).await;
    });
}
