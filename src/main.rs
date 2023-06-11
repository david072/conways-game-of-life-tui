use std::collections::HashSet;
use std::time::SystemTime;
use std::{io, time::Duration};

use crossterm::{
    cursor,
    event::{self, EnableMouseCapture, Event, KeyCode, MouseButton, MouseEvent, MouseEventKind},
    execute, queue,
    style::{self, Stylize},
    terminal,
};

const KEYBIND_TEXT: &str =
    "<space> play/pause • <left-click> add • <right-click> remove • s save current cells • l restore last save • c clear • r reset generation • <arrow-up/down> change speed • q quit";
const CELL: &str = "██";
const UPDATE_MS: u128 = 200;
const MSG_DISPLAY_MS: u128 = 1000;

type Coord = (u16, u16);

fn main() -> io::Result<()> {
    let mut stdout = io::stdout();
    run(&mut stdout)
}

enum AppAction {
    NeedRedraw(bool),
    Quit,
}

struct App {
    alive_cells: HashSet<Coord>,
    save: HashSet<Coord>,

    is_playing: bool,
    speed: f32,
    last_updated: SystemTime,
    generation: usize,
    current_msg: Option<String>,
    msg_display_start: SystemTime,
}

impl App {
    fn new() -> App {
        App {
            alive_cells: HashSet::new(),
            save: HashSet::new(),
            is_playing: false,
            speed: 1.0,
            last_updated: SystemTime::now(),
            generation: 0,
            current_msg: None,
            msg_display_start: SystemTime::now(),
        }
    }

    fn draw<W: io::Write>(&self, write: &mut W) -> io::Result<()> {
        queue!(
            write,
            terminal::Clear(terminal::ClearType::All),
            style::ResetColor,
            cursor::Hide,
            cursor::MoveTo(1, 1),
        )?;

        let size = terminal::size().unwrap();

        for (col, row) in &self.alive_cells {
            let text = CELL.yellow().stylize();
            queue!(
                write,
                cursor::MoveTo(*col, *row),
                style::PrintStyledContent(text)
            )?;
        }

        let raw_info_text = if let Some(msg) = &self.current_msg {
            msg.to_string()
        } else {
            format!(
                "{} ({:.1}x) - Generation: {}, Cells: {}",
                if !self.is_playing {
                    "Paused"
                } else {
                    "Playing"
                },
                self.speed,
                self.generation,
                self.alive_cells.len(),
            )
        };
        let raw_info_text_len = raw_info_text.len();
        let info_text = raw_info_text.blue().stylize();

        let keybind_text = KEYBIND_TEXT.dark_grey().stylize();
        // Horizontal layout
        if size.0 >= raw_info_text_len as u16 + KEYBIND_TEXT.chars().count() as u16 + 5 {
            queue!(
                write,
                cursor::MoveTo(1, size.1),
                style::PrintStyledContent(info_text),
                cursor::MoveTo(size.0 - KEYBIND_TEXT.chars().count() as u16 - 1, size.1),
                style::PrintStyledContent(keybind_text)
            )?;
        }
        // Vertical layout
        else {
            queue!(
                write,
                cursor::MoveTo(1, size.1 - 2),
                style::PrintStyledContent(info_text),
                cursor::MoveTo(1, size.1),
                style::PrintStyledContent(keybind_text)
            )?;
        }

        write.flush()?;
        Ok(())
    }

    fn handle_input(&mut self) -> io::Result<AppAction> {
        let mut need_redraw = false;

        if event::poll(Duration::ZERO)? {
            match event::read()? {
                Event::Key(key) => 'blk: {
                    match key.code {
                        KeyCode::Char('q') => return Ok(AppAction::Quit),
                        KeyCode::Char(' ') => self.is_playing = !self.is_playing,
                        KeyCode::Char('r') => {
                            self.is_playing = false;
                            self.generation = 0;
                        }
                        KeyCode::Char('c') => {
                            self.is_playing = false;
                            self.alive_cells.clear();
                            self.generation = 0;
                        }
                        KeyCode::Char('s') => {
                            self.save = self.alive_cells.clone();
                            self.current_msg = Some("Current cells saved!".to_string());
                            self.msg_display_start = SystemTime::now();
                        }
                        KeyCode::Char('l') => {
                            self.alive_cells = self.save.clone();
                            self.current_msg = Some("Last save loaded!".to_string());
                            self.msg_display_start = SystemTime::now();
                        }
                        KeyCode::Up => self.speed += 0.5,
                        KeyCode::Down if self.speed > 0.5 => self.speed -= 0.5,
                        _ => break 'blk,
                    }
                    need_redraw = true;
                }
                Event::Mouse(MouseEvent {
                    kind, column, row, ..
                }) if !self.is_playing => 'blk: {
                    let coord = to_point_coord(row, column);
                    match kind {
                        MouseEventKind::Drag(MouseButton::Left)
                        | MouseEventKind::Down(MouseButton::Left) => {
                            self.alive_cells.insert(coord);
                        }
                        MouseEventKind::Drag(MouseButton::Right)
                        | MouseEventKind::Down(MouseButton::Right) => {
                            self.alive_cells.retain(|c| *c != coord);
                        }
                        _ => break 'blk,
                    }
                    need_redraw = true;
                }
                Event::Resize(..) => need_redraw = true,
                _ => {}
            }
        }

        Ok(AppAction::NeedRedraw(need_redraw))
    }

    fn update(&mut self) -> bool {
        let mut need_redraw = false;
        if self.is_playing {
            if self.last_updated.elapsed().unwrap().as_millis()
                > (UPDATE_MS as f32 / self.speed).round() as u128
            {
                self.next_generation();
                need_redraw = true;
                self.generation += 1;
                self.last_updated = SystemTime::now();
            }
        }

        if self.msg_display_start.elapsed().unwrap().as_millis() > MSG_DISPLAY_MS
            && self.current_msg.is_some()
        {
            self.current_msg = None;
            need_redraw = true;
        }

        need_redraw
    }

    fn next_generation(&mut self) {
        let size = terminal::size().unwrap();

        let mut removed_points = vec![];
        let mut new_points = vec![];
        for column in (0..size.0).step_by(2) {
            for row in 0..size.1 {
                let coord = (column, row);

                let mut neighbor_count = 0;
                for col in (column.saturating_sub(2)..=(column + 2).min(size.0)).step_by(2) {
                    for row in row.saturating_sub(1)..=(row + 1).min(size.1) {
                        if (col, row) == coord {
                            continue;
                        }

                        if self.alive_cells.contains(&(col, row)) {
                            neighbor_count += 1;
                        }
                    }
                }

                if self.alive_cells.contains(&coord) && (neighbor_count < 2 || neighbor_count > 3) {
                    removed_points.push(coord);
                } else if neighbor_count == 3 {
                    new_points.push(coord);
                }
            }
        }

        for coord in removed_points {
            self.alive_cells.retain(|c| *c != coord);
        }

        for coord in new_points {
            self.alive_cells.insert(coord);
        }
    }
}

fn to_point_coord(row: u16, column: u16) -> Coord {
    if column % 2 != 0 {
        (column - 1, row)
    } else {
        (column, row)
    }
}

fn run<W: io::Write>(write: &mut W) -> io::Result<()> {
    execute!(write, terminal::EnterAlternateScreen, EnableMouseCapture)?;
    terminal::enable_raw_mode()?;

    let mut app = App::new();
    let mut need_redraw = true;

    'outer: loop {
        if need_redraw {
            app.draw(write)?;
        }

        match app.handle_input()? {
            AppAction::NeedRedraw(b) => need_redraw = b,
            AppAction::Quit => break 'outer,
        }

        need_redraw |= app.update();
    }

    execute!(
        write,
        style::ResetColor,
        cursor::Show,
        terminal::LeaveAlternateScreen,
    )?;

    terminal::disable_raw_mode()
}
