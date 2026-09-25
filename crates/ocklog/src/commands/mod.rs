use crate::cli::Args;
use crate::config::UiTheme;
use crate::docker::{DockerClient, DockerLogMsg};
use crate::query::Query;
use crate::state::{AppState, ContainerFilterOption, LogItem, PromptMode, VisualMode};
use crate::terminal_colors::TerminalColorMode;
use crate::ui;
use crate::worker::{context::AppWorkerContext, EventRegistry, EventSender};

use crossterm::{
    cursor::SetCursorStyle,
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers, MouseButton,
        MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use parking_lot::RwLock;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{stdout, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::task::JoinSet;

pub struct RunOptions {
    pub args: Args,
    pub color_mode: TerminalColorMode,
}

struct TerminalGuard;
impl TerminalGuard {
    fn enter() -> eyre::Result<Self> {
        enable_raw_mode()?;
        execute!(
            stdout(),
            EnterAlternateScreen,
            EnableMouseCapture,
            SetCursorStyle::SteadyBlock
        )?;
        Ok(TerminalGuard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            stdout(),
            SetCursorStyle::DefaultUserShape,
            DisableMouseCapture,
            LeaveAlternateScreen
        );
        let _ = disable_raw_mode();
    }
}

pub async fn run(opts: RunOptions) -> eyre::Result<()> {
    let theme = UiTheme::default();
    let state = Arc::new(RwLock::new(AppState::new(100_000)));

    let docker_client = Arc::new(DockerClient::new());
    let (log_tx, mut log_rx) = mpsc::unbounded_channel::<DockerLogMsg>();
    let (container_status_tx, mut container_status_rx) = mpsc::unbounded_channel();

    // 1. Enter Terminal Guard FIRST so the user sees the interface immediately (< 5ms)
    let _term_guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // 2. Spawn concurrent background ingestion
    if docker_client.is_available() {
        let client = Arc::clone(&docker_client);
        let s_arc = Arc::clone(&state);
        let tx = log_tx.clone();
        let status_tx = container_status_tx.clone();

        tokio::spawn(async move {
            match client.list_containers().await {
                Ok(containers) => {
                    // Populate services options immediately
                    {
                        let mut s = s_arc.write();
                        for c in &containers {
                            s.service_options.push(ContainerFilterOption {
                                id: c.id.clone(),
                                name: c.name.clone(),
                                enabled: true,
                                is_running: c.is_running,
                                has_error: c.has_error,
                            });
                        }
                    }

                    // Parallel initial log fetch across all containers simultaneously
                    let mut set = JoinSet::new();
                    for c in &containers {
                        let client = Arc::clone(&client);
                        let cid = c.id.clone();
                        let name = c.name.clone();
                        let is_running = c.is_running;
                        let has_error = c.has_error;

                        set.spawn(async move {
                            let tail = if is_running || has_error { 1000 } else { 100 };
                            client.fetch_initial_logs_tail(&cid, &name, tail).await
                        });
                    }

                    let mut initial_logs = Vec::new();
                    while let Some(res) = set.join_next().await {
                        if let Ok(logs) = res {
                            initial_logs.extend(logs);
                        }
                    }

                    // Sort initial logs chronologically across all containers
                    initial_logs.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

                    // Bulk insert initial logs into state
                    {
                        let mut s = s_arc.write();
                        for msg in initial_logs {
                            let lvl = infer_log_level(&msg.message, msg.is_system_event);
                            s.push_log(LogItem::new(
                                msg.service_name,
                                msg.message,
                                lvl,
                                msg.timestamp,
                                msg.is_system_event,
                            ));
                        }
                        s.recompute_visible_indices();
                    }

                    // Start live stream re-using the container list without duplicate round-trips
                    let _ = client
                        .start_live_ingestion_with_containers(&containers, tx, status_tx)
                        .await;
                }
                Err(e) => {
                    let mut s = s_arc.write();
                    s.push_log(LogItem::new(
                        "system".into(),
                        format!("Docker daemon error: {}. Check socket permissions.", e),
                        Some("ERROR".into()),
                        None,
                        true,
                    ));
                }
            }
        });
    } else {
        let mut s = state.write();
        s.push_log(LogItem::new(
            "system".into(),
            "Docker socket /var/run/docker.sock not found.".into(),
            Some("WARN".into()),
            None,
            true,
        ));
    }

    let ctx = AppWorkerContext::builder()
        .state(Arc::clone(&state))
        .build();

    let registry = EventRegistry::spawn(ctx);
    let (tx, mut rx, _worker_handle) = registry.into_split();

    run_event_loop(
        &mut terminal,
        state,
        tx,
        &mut rx,
        &mut log_rx,
        &mut container_status_rx,
        &theme,
        &opts.color_mode,
    )
    .await?;

    Ok(())
}

async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    state: Arc<RwLock<AppState>>,
    _tx: EventSender,
    rx: &mut crate::worker::EventReceiver,
    log_rx: &mut mpsc::UnboundedReceiver<DockerLogMsg>,
    container_status_rx: &mut mpsc::UnboundedReceiver<(String, bool, bool)>,
    theme: &UiTheme,
    color_mode: &TerminalColorMode,
) -> eyre::Result<()> {
    let mut pending_g = false;
    let mut last_auto_scroll = Instant::now();
    let mut current_cursor_style: Option<SetCursorStyle> = None;

    loop {
        // 1. Ingest live Docker logs
        while let Ok(msg) = log_rx.try_recv() {
            let mut s = state.write();
            let lvl = infer_log_level(&msg.message, msg.is_system_event);
            s.push_log(LogItem::new(
                msg.service_name,
                msg.message,
                lvl,
                msg.timestamp,
                msg.is_system_event,
            ));
        }

        // 2. Ingest container status updates
        while let Ok((id, is_running, has_error)) = container_status_rx.try_recv() {
            let mut s = state.write();
            if let Some(opt) = s.service_options.iter_mut().find(|o| o.id == id) {
                opt.is_running = is_running;
                opt.has_error = has_error;
            }
        }

        // 3. Ingest worker events
        while let Ok(event) = rx.try_recv() {
            tracing::debug!("Worker event: {:?}", event);
        }

        // 4. Edge auto-scroll on mouse drag (debounced to 60ms intervals)
        if last_auto_scroll.elapsed() >= Duration::from_millis(60) {
            let mut s = state.write();
            if s.trigger_mouse_auto_scroll() {
                last_auto_scroll = Instant::now();
            }
        }

        // 5. Render Frame & Update Dynamic Cursor Shape
        {
            let mut current_state = state.write();
            terminal.draw(|f| ui::render(f, &mut current_state, theme, color_mode))?;

            // Switch to line cursor (SteadyBar) while typing, block cursor in normal navigation mode
            let desired_style = if current_state.prompt_mode == PromptMode::Filter
                || current_state.show_service_search
            {
                SetCursorStyle::SteadyBar
            } else {
                SetCursorStyle::SteadyBlock
            };

            if current_cursor_style != Some(desired_style) {
                let _ = execute!(stdout(), desired_style);
                current_cursor_style = Some(desired_style);
            }
        }

        if event::poll(Duration::from_millis(20))? {
            let ev = event::read()?;

            // Mouse handling
            if let Event::Mouse(mouse) = ev {
                let mut s = state.write();

                if s.show_service_menu {
                    let menu_width = 46u16.min(s.viewport_width as u16);
                    let menu_height = 14u16.min(s.viewport_height as u16);
                    let menu_x = 0;
                    let menu_y = (s.viewport_height as u16).saturating_sub(menu_height);

                    let inside_popup = mouse.column >= menu_x
                        && mouse.column < menu_x + menu_width
                        && mouse.row >= menu_y
                        && mouse.row < menu_y + menu_height;

                    if !inside_popup {
                        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                            s.show_service_menu = false;
                            s.show_service_search = false;
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }

                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        let (r, c) = s.screen_coords_to_char_pos(mouse.column as usize, mouse.row as usize);
                        s.cursor_row = r;
                        s.cursor_col = c;

                        // Clicking or starting a drag on a tag initiates line visual selection ('V')
                        if s.is_screen_coords_on_tag(mouse.column as usize, mouse.row as usize) {
                            s.visual_mode = VisualMode::Line;
                            s.visual_anchor = Some((r, 0));
                        } else {
                            s.visual_mode = VisualMode::Character;
                            s.visual_anchor = Some((r, c));
                        }

                        s.is_mouse_dragging = false;
                        s.last_mouse_pos = Some((mouse.column, mouse.row));
                        s.sticky = false;
                        if s.line_fold {
                            s.adjust_h_scroll();
                        }
                    }
                    MouseEventKind::Drag(MouseButton::Left) => {
                        let (r, c) =
                            s.screen_coords_to_char_pos(mouse.column as usize, mouse.row as usize);
                        s.cursor_row = r;
                        s.cursor_col = c;
                        s.is_mouse_dragging = true;
                        s.last_mouse_pos = Some((mouse.column, mouse.row));
                        s.adjust_h_scroll();
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        if s.is_mouse_dragging && s.visual_mode != VisualMode::None {
                            if let Some(text) = s.get_selected_text() {
                                if !text.is_empty() {
                                    copy_to_clipboard(&text);
                                    s.set_copied_notification();
                                }
                            }
                        }
                        s.exit_visual_mode();
                    }
                    MouseEventKind::ScrollDown => {
                        s.move_down(3);
                        s.sticky = false;
                    }
                    MouseEventKind::ScrollUp => {
                        s.move_up(3);
                        s.sticky = false;
                    }
                    _ => {}
                }
                continue;
            }

            // Keyboard handling
            if let Event::Key(key) = ev {
                let mut s = state.write();

                let is_esc_or_ctrl_c = matches!(
                    (key.code, key.modifiers),
                    (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL)
                );

                if is_esc_or_ctrl_c {
                    pending_g = false;
                    if s.prompt_mode != PromptMode::None {
                        s.prompt_mode = PromptMode::None;
                        s.input_buffer.clear();
                        s.filter_cursor_col = 0;
                        s.live_filter_ast = None;
                    } else if s.show_service_menu {
                        if s.show_service_search {
                            s.show_service_search = false;
                            s.service_search_query.clear();
                            s.update_service_search();
                        } else {
                            s.show_service_menu = false;
                        }
                    } else if s.visual_mode != VisualMode::None {
                        s.exit_visual_mode();
                    }
                    continue;
                }

                // 1. Service Menu Active Input (<C-s>)
                if s.show_service_menu {
                    let filtered = s.filtered_service_indices();

                    if s.show_service_search {
                        match (key.code, key.modifiers) {
                            (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                                s.show_service_menu = false;
                                s.show_service_search = false;
                            }
                            (KeyCode::Char('f'), KeyModifiers::CONTROL) => {
                                s.show_service_search = false;
                                s.service_search_query.clear();
                                s.update_service_search();
                            }
                            (KeyCode::Char('j'), KeyModifiers::CONTROL) | (KeyCode::Down, _) => {
                                if !filtered.is_empty() {
                                    s.service_menu_selected_idx =
                                        (s.service_menu_selected_idx + 1) % filtered.len();
                                }
                            }
                            (KeyCode::Char('k'), KeyModifiers::CONTROL) | (KeyCode::Up, _) => {
                                if !filtered.is_empty() {
                                    s.service_menu_selected_idx =
                                        (s.service_menu_selected_idx + filtered.len() - 1)
                                            % filtered.len();
                                }
                            }
                            (KeyCode::Enter, KeyModifiers::NONE)
                            | (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                                if let Some(&actual_idx) = filtered.get(s.service_menu_selected_idx)
                                {
                                    if let Some(opt) = s.service_options.get_mut(actual_idx) {
                                        opt.enabled = !opt.enabled;
                                    }
                                }
                                s.recompute_visible_indices();
                            }
                            (KeyCode::Enter, KeyModifiers::CONTROL) => {
                                if let Some(&selected_actual_idx) =
                                    filtered.get(s.service_menu_selected_idx)
                                {
                                    for (i, opt) in s.service_options.iter_mut().enumerate() {
                                        opt.enabled = i == selected_actual_idx;
                                    }
                                }
                                s.recompute_visible_indices();
                            }
                            (KeyCode::Char('x'), KeyModifiers::CONTROL) => {
                                for opt in &mut s.service_options {
                                    opt.enabled = false;
                                }
                                s.recompute_visible_indices();
                            }
                            (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                                for opt in &mut s.service_options {
                                    opt.enabled = true;
                                }
                                s.recompute_visible_indices();
                            }
                            (KeyCode::Backspace, _) => {
                                s.service_search_query.pop();
                                s.update_service_search();
                            }
                            (KeyCode::Char(c), KeyModifiers::NONE) => {
                                s.service_search_query.push(c);
                                s.update_service_search();
                            }
                            _ => {}
                        }
                    } else {
                        match (key.code, key.modifiers) {
                            (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                                s.show_service_menu = false
                            }
                            (KeyCode::Char('f'), KeyModifiers::CONTROL) => {
                                s.show_service_search = true;
                                s.service_search_query.clear();
                            }
                            (KeyCode::Char('j'), KeyModifiers::NONE) | (KeyCode::Down, _) => {
                                if !filtered.is_empty() {
                                    s.service_menu_selected_idx =
                                        (s.service_menu_selected_idx + 1) % filtered.len();
                                }
                            }
                            (KeyCode::Char('k'), KeyModifiers::NONE) | (KeyCode::Up, _) => {
                                if !filtered.is_empty() {
                                    s.service_menu_selected_idx =
                                        (s.service_menu_selected_idx + filtered.len() - 1)
                                            % filtered.len();
                                }
                            }
                            (KeyCode::Char(' '), _)
                            | (KeyCode::Char('l'), KeyModifiers::NONE)
                            | (KeyCode::Enter, KeyModifiers::NONE) => {
                                if let Some(&actual_idx) = filtered.get(s.service_menu_selected_idx)
                                {
                                    if let Some(opt) = s.service_options.get_mut(actual_idx) {
                                        opt.enabled = !opt.enabled;
                                    }
                                }
                                s.recompute_visible_indices();
                            }
                            (KeyCode::Enter, KeyModifiers::CONTROL) => {
                                if let Some(&selected_actual_idx) =
                                    filtered.get(s.service_menu_selected_idx)
                                {
                                    for (i, opt) in s.service_options.iter_mut().enumerate() {
                                        opt.enabled = i == selected_actual_idx;
                                    }
                                }
                                s.recompute_visible_indices();
                            }
                            (KeyCode::Char('x'), KeyModifiers::CONTROL) => {
                                for opt in &mut s.service_options {
                                    opt.enabled = false;
                                }
                                s.recompute_visible_indices();
                            }
                            (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                                for opt in &mut s.service_options {
                                    opt.enabled = true;
                                }
                                s.recompute_visible_indices();
                            }
                            _ => {}
                        }
                    }
                    continue;
                }

                // 2. Filter Command Line Input (<C-f>)
                if s.prompt_mode == PromptMode::Filter {
                    if matches!(
                        (key.code, key.modifiers),
                        (KeyCode::Char('f'), KeyModifiers::CONTROL)
                    ) {
                        s.prompt_mode = PromptMode::None;
                        s.input_buffer.clear();
                        s.filter_cursor_col = 0;
                        s.live_filter_ast = None;
                        continue;
                    }

                    match (key.code, key.modifiers) {
                        (KeyCode::Enter, _) => {
                            let trimmed = s.input_buffer.trim().to_string();
                            if trimmed.is_empty() {
                                s.filter_ast = None;
                                s.active_filter_str = None;
                            } else {
                                s.active_filter_str = Some(trimmed.clone());
                                match Query::parse(&trimmed) {
                                    Ok(q) => {
                                        s.filter_ast = Some(q);
                                    }
                                    Err(_) => {
                                        s.filter_ast = None;
                                    }
                                }
                            }
                            s.live_filter_ast = None;
                            s.recompute_visible_indices();
                            s.prompt_mode = PromptMode::None;
                            s.input_buffer.clear();
                            s.filter_cursor_col = 0;
                        }
                        (KeyCode::Left, _) => {
                            s.filter_cursor_col = s.filter_cursor_col.saturating_sub(1);
                        }
                        (KeyCode::Right, _) => {
                            let len = s.input_buffer.chars().count();
                            if s.filter_cursor_col < len {
                                s.filter_cursor_col += 1;
                            }
                        }
                        (KeyCode::Home, _) | (KeyCode::Char('a'), KeyModifiers::CONTROL) => {
                            s.filter_cursor_col = 0;
                        }
                        (KeyCode::End, _) | (KeyCode::Char('e'), KeyModifiers::CONTROL) => {
                            s.filter_cursor_col = s.input_buffer.chars().count();
                        }
                        (KeyCode::Delete, _) => {
                            let mut chars: Vec<char> = s.input_buffer.chars().collect();
                            if s.filter_cursor_col < chars.len() {
                                chars.remove(s.filter_cursor_col);
                                s.input_buffer = chars.into_iter().collect();
                                s.update_live_filter();
                            }
                        }
                        (KeyCode::Backspace, _) => {
                            if s.filter_cursor_col > 0 {
                                let mut chars: Vec<char> = s.input_buffer.chars().collect();
                                if s.filter_cursor_col <= chars.len() {
                                    chars.remove(s.filter_cursor_col - 1);
                                    s.input_buffer = chars.into_iter().collect();
                                    s.filter_cursor_col -= 1;
                                    s.update_live_filter();
                                }
                            }
                        }
                        (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                            s.input_buffer.clear();
                            s.filter_cursor_col = 0;
                            s.update_live_filter();
                        }
                        (KeyCode::Char('w'), KeyModifiers::CONTROL) => {
                            let chars: Vec<char> = s.input_buffer.chars().collect();
                            let mut i = s.filter_cursor_col;
                            while i > 0 && chars[i - 1].is_whitespace() {
                                i -= 1;
                            }
                            while i > 0 && !chars[i - 1].is_whitespace() {
                                i -= 1;
                            }
                            let mut new_chars = chars[..i].to_vec();
                            new_chars.extend_from_slice(&chars[s.filter_cursor_col..]);
                            s.filter_cursor_col = i;
                            s.input_buffer = new_chars.into_iter().collect();
                            s.update_live_filter();
                        }
                        (KeyCode::Char(c), KeyModifiers::NONE)
                        | (KeyCode::Char(c), KeyModifiers::SHIFT) => {
                            let mut chars: Vec<char> = s.input_buffer.chars().collect();
                            let col = s.filter_cursor_col.min(chars.len());
                            chars.insert(col, c);
                            s.input_buffer = chars.into_iter().collect();
                            s.filter_cursor_col += 1;
                            s.update_live_filter();
                        }
                        _ => {}
                    }
                    continue;
                }

                // 3. Normal / Visual Mode Keys
                match (key.code, key.modifiers) {
                    (KeyCode::Char('q'), KeyModifiers::NONE) => {
                        break Ok(());
                    }
                    (KeyCode::Char('z'), KeyModifiers::NONE) | (KeyCode::Char('Z'), _) => {
                        pending_g = false;
                        s.toggle_line_fold();
                    }

                    (KeyCode::Char('w'), KeyModifiers::NONE) => {
                        pending_g = false;
                        s.move_word_forward();
                    }
                    (KeyCode::Char('b'), KeyModifiers::NONE) => {
                        pending_g = false;
                        s.move_word_backward();
                    }
                    (KeyCode::Char('e'), KeyModifiers::NONE) => {
                        pending_g = false;
                        s.move_word_end();
                    }

                    (KeyCode::Char('j'), KeyModifiers::CONTROL) => {
                        pending_g = false;
                        s.move_down(5);
                    }
                    (KeyCode::Char('k'), KeyModifiers::CONTROL) => {
                        pending_g = false;
                        s.move_up(5);
                    }
                    (KeyCode::Char('h'), KeyModifiers::CONTROL) => {
                        pending_g = false;
                        s.move_left(5);
                    }
                    (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                        pending_g = false;
                        s.move_right(5);
                    }

                    (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                        pending_g = false;
                        s.show_service_menu = true;
                    }

                    (KeyCode::Char('f'), KeyModifiers::CONTROL) => {
                        pending_g = false;
                        if s.prompt_mode == PromptMode::Filter {
                            s.prompt_mode = PromptMode::None;
                            s.input_buffer.clear();
                            s.filter_cursor_col = 0;
                            s.live_filter_ast = None;
                        } else {
                            s.prompt_mode = PromptMode::Filter;
                            if let Some(ref active) = s.active_filter_str {
                                s.input_buffer = active.clone();
                            } else {
                                s.input_buffer.clear();
                            }
                            s.filter_cursor_col = s.input_buffer.chars().count();
                            s.update_live_filter();
                        }
                    }

                    (KeyCode::Char('j'), KeyModifiers::NONE)
                    | (KeyCode::Down, KeyModifiers::NONE) => {
                        pending_g = false;
                        s.move_down(1);
                    }
                    (KeyCode::Char('k'), KeyModifiers::NONE)
                    | (KeyCode::Up, KeyModifiers::NONE) => {
                        pending_g = false;
                        s.move_up(1);
                    }
                    (KeyCode::Char('h'), KeyModifiers::NONE)
                    | (KeyCode::Left, KeyModifiers::NONE) => {
                        pending_g = false;
                        s.move_left(1);
                    }
                    (KeyCode::Char('l'), KeyModifiers::NONE)
                    | (KeyCode::Right, KeyModifiers::NONE) => {
                        pending_g = false;
                        s.move_right(1);
                    }
                    (KeyCode::Char('0'), KeyModifiers::NONE) => {
                        pending_g = false;
                        s.cursor_col = 0;
                        s.adjust_h_scroll();
                    }
                    (KeyCode::Char('$'), KeyModifiers::NONE) => {
                        pending_g = false;
                        let line_len = s.max_line_len();
                        s.cursor_col = line_len.saturating_sub(1);
                        s.adjust_h_scroll();
                    }

                    (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                        pending_g = false;
                        let half = (s.viewport_height / 2).max(1);
                        s.move_down(half);
                    }
                    (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                        pending_g = false;
                        let half = (s.viewport_height / 2).max(1);
                        s.move_up(half);
                    }

                    (KeyCode::Char('G'), _) => {
                        pending_g = false;
                        s.move_to_bottom();
                        s.sticky = true;
                    }
                    (KeyCode::Char('g'), KeyModifiers::NONE) => {
                        if pending_g {
                            s.move_to_top();
                            pending_g = false;
                            s.sticky = false;
                        } else {
                            pending_g = true;
                        }
                    }

                    (KeyCode::Char('v'), KeyModifiers::NONE) => {
                        pending_g = false;
                        s.toggle_visual_mode(VisualMode::Character);
                    }
                    (KeyCode::Char('V'), _) | (KeyCode::Char('v'), KeyModifiers::SHIFT) => {
                        pending_g = false;
                        s.toggle_visual_mode(VisualMode::Line);
                    }
                    (KeyCode::Char('v'), KeyModifiers::CONTROL) => {
                        pending_g = false;
                        s.toggle_visual_mode(VisualMode::Block);
                    }

                    (KeyCode::Char('y'), KeyModifiers::NONE) => {
                        pending_g = false;
                        if s.visual_mode != VisualMode::None || !s.toggled_lines.is_empty() {
                            if let Some(text) = s.get_selected_text() {
                                copy_to_clipboard(&text);
                                s.set_copied_notification();
                            }
                            s.toggled_lines.clear();
                            s.exit_visual_mode();
                        }
                    }

                    _ => {
                        pending_g = false;
                    }
                }
            }
        }
    }
}

fn infer_log_level(msg: &str, is_system: bool) -> Option<String> {
    if is_system {
        return Some("SYS".into());
    }
    let upper = msg.to_uppercase();
    if upper.contains("ERROR")
        || upper.contains("FATAL")
        || upper.contains("CRITICAL")
        || upper.contains("🚨")
    {
        Some("ERROR".into())
    } else if upper.contains("WARN") {
        Some("WARN".into())
    } else if upper.contains("INFO") {
        Some("INFO".into())
    } else if upper.contains("DEBUG") {
        Some("DEBUG".into())
    } else {
        None
    }
}

fn copy_to_clipboard(text: &str) {
    let encoded = base64_encode(text);
    let osc52 = format!("\x1b]52;c;{}\x07", encoded);
    let _ = stdout().write_all(osc52.as_bytes());
    let _ = stdout().flush();

    if cfg!(target_os = "macos") {
        if let Ok(mut child) = Command::new("pbcopy").stdin(Stdio::piped()).spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
        }
    } else if cfg!(target_os = "linux") {
        let spawned = Command::new("wl-copy")
            .stdin(Stdio::piped())
            .spawn()
            .or_else(|_| {
                Command::new("xclip")
                    .arg("-selection")
                    .arg("clipboard")
                    .stdin(Stdio::piped())
                    .spawn()
            });

        if let Ok(mut child) = spawned {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
        }
    }
}

fn base64_encode(input: &str) -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = input.as_bytes();
    let mut out = String::with_capacity((bytes.len() + 2) / 3 * 4);

    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);

        out.push(CHARSET[(b0 >> 2) as usize] as char);
        out.push(CHARSET[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);

        if chunk.len() > 1 {
            out.push(CHARSET[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }

        if chunk.len() > 2 {
            out.push(CHARSET[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }

    out
}
