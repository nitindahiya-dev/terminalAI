use clap::Parser;
use eframe::egui::{self, Color32, FontId, RichText, Rounding, Stroke, TextStyle, Vec2};
use anyhow::Result;
use std::env;
use vte::{Parser as VteParser, Perform, Params};
use shellexpand::tilde;
use std::sync::mpsc::{channel, Sender, Receiver};
use std::thread;
use log::{debug, error, info};

mod command;
use command::{execute_command, change_directory, process_ai_command};

#[derive(Parser, Debug)]
#[command(name = "terminalAI", about = "An AI-powered terminal assistant")]
struct Args {
    #[arg(short, long, help = "Run in interactive GUI mode")]
    interactive: bool,
}

struct TerminalAIApp {
    input: String,
    output: Vec<(String, bool)>, // (text, is_error)
    history: Vec<String>,
    history_index: Option<usize>,
    cmd_tx: Sender<(String, Result<String>)>,
    cmd_rx: Receiver<(String, Result<String>)>,
    #[allow(dead_code)]
    ansi_parser: VteParser,
    output_buffer: String,
    current_color: Color32,
}

impl Perform for TerminalAIApp {
    fn print(&mut self, c: char) {
        self.output_buffer.push(c);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.output_buffer.push('\n'),
            b'\r' => {},
            _ => self.output_buffer.push(byte as char),
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _c: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
    fn csi_dispatch(&mut self, params: &Params, _intermediates: &[u8], _ignore: bool, c: char) {
        if c == 'm' {
            let mut color = self.current_color;
            for param in params.iter() {
                if let Some(p) = param.get(0) {
                    match p {
                        0 => color = Color32::from_rgb(200, 200, 200),
                        31 => color = Color32::from_rgb(255, 100, 100),
                        32 => color = Color32::from_rgb(150, 255, 150),
                        36 => color = Color32::from_rgb(100, 200, 255),
                        _ => {},
                    }
                }
            }
            self.current_color = color;
        }
    }
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
}

impl TerminalAIApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (cmd_tx, cmd_rx) = channel();
        TerminalAIApp {
            input: String::new(),
            output: vec![(format!("Welcome to terminalAI! Type commands below."), false)],
            history: Vec::new(),
            history_index: None,
            cmd_tx,
            cmd_rx,
            ansi_parser: VteParser::new(),
            output_buffer: String::new(),
            current_color: Color32::from_rgb(200, 200, 200),
        }
    }

    fn process_command(&mut self, cmd: &str) -> Result<()> {
        let cmd = cmd.trim();
        if cmd.is_empty() {
            return Ok(());
        }

        info!("Processing command: {}", cmd);
        self.history.push(cmd.to_string());
        self.history_index = None;

        if cmd == "exit" || cmd == "quit" {
            info!("Exit command received");
            std::process::exit(0);
        }

        // List of known bash commands (extend as needed)
        let known_commands = [
            "ls", "pwd", "clear", "cd", "cat", "echo", "grep", "find", "rm", "mkdir", "touch",
            "mv", "cp", "chmod", "chown", "whoami", "df", "du", "top", "htop", "ps", "kill",
        ];

        let command = if cmd.starts_with("cd ") || cmd == "clear" || {
            // Check if the command starts with a known bash command or looks like a bash command
            let first_word = cmd.split_whitespace().next().unwrap_or("");
            known_commands.contains(&first_word) || cmd.contains("=") || cmd.contains("|") || cmd.contains(">")
        } {
            // Treat as direct bash command
            cmd.to_string()
        } else {
            // Send to AI for natural language processing
            match process_ai_command(cmd) {
                Ok(ai_cmd) => ai_cmd,
                Err(e) => {
                    error!("AI processing error: {}", e);
                    self.output.push((format!("Error processing command: {}", e), true));
                    return Ok(());
                }
            }
        };

        if command.starts_with("cd ") {
            let dir = command.trim_start_matches("cd ").trim();
            debug!("Executing cd: {}", dir);
            match change_directory(dir) {
                Ok(_) => {
                    let cwd = std::env::current_dir()?.display().to_string();
                    self.output.push((format!("Changed directory to {}", cwd), false));
                }
                Err(e) => {
                    error!("cd error: {}", e);
                    self.output.push((format!("Error changing directory: {}", e), true));
                }
            }
        } else {
            let cmd_tx = self.cmd_tx.clone();
            let command_clone = command.clone();
            thread::spawn(move || {
                debug!("Running command in thread: {}", command_clone);
                let result = execute_command(&command_clone, &mut || {});
                cmd_tx.send((command_clone, result)).expect("Failed to send command result");
            });
        }
        Ok(())
    }

    fn complete_input(&mut self) {
        let input = self.input.trim();
        if input.is_empty() {
            return;
        }

        debug!("Attempting tab completion for: {}", input);
        let expanded = tilde(input).to_string();
        if let Ok(entries) = std::fs::read_dir(".") {
            let mut matches = Vec::new();
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    if name.starts_with(&expanded) {
                        matches.push(name);
                    }
                }
            }
            if matches.len() == 1 {
                debug!("Tab completion: single match {}", matches[0]);
                self.input = matches[0].clone();
            } else if !matches.is_empty() {
                debug!("Tab completion: multiple matches {:?}", matches);
                self.output.push((matches.join("\t"), false));
            }
        }
    }
}

impl eframe::App for TerminalAIApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for command results
        while let Ok((cmd, result)) = self.cmd_rx.try_recv() {
            debug!("Received command result for: {}", cmd);
            self.output_buffer.clear();
            self.current_color = Color32::from_rgb(200, 200, 200);
            match result {
                Ok(output) => {
                    let mut parser = VteParser::new();
                    for byte in output.bytes() {
                        parser.advance(self, byte);
                    }
                    if !self.output_buffer.is_empty() {
                        self.output.push((self.output_buffer.clone(), false));
                        self.output_buffer.clear();
                    } else {
                        self.output.push(("Command executed successfully.".to_string(), false));
                    }
                }
                Err(e) => {
                    error!("Command error: {}", e);
                    self.output.push((format!("Error executing command: {}", e), true));
                }
            }
        }

        let mut style = (*ctx.style()).clone();
        style.visuals = egui::Visuals {
            dark_mode: true,
            override_text_color: Some(Color32::from_rgb(200, 200, 200)),
            panel_fill: Color32::from_rgb(20, 20, 25),
            window_fill: Color32::from_rgb(30, 30, 35),
            window_stroke: Stroke::new(1.0, Color32::from_rgb(50, 50, 60)),
            window_rounding: Rounding::same(8.0),
            ..Default::default()
        };
        style.text_styles.insert(
            TextStyle::Body,
            FontId::new(14.0, egui::FontFamily::Monospace),
        );
        style.text_styles.insert(
            TextStyle::Button,
            FontId::new(14.0, egui::FontFamily::Monospace),
        );
        ctx.set_style(style);

        egui::CentralPanel::default()
            .frame(egui::Frame {
                fill: Color32::from_rgb(20, 20, 25),
                inner_margin: egui::Margin::same(10.0),
                ..Default::default()
            })
            .show(ctx, |ui| {
                let cwd = std::env::current_dir().unwrap_or_default().display().to_string();
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("📍 {}", cwd))
                            .color(Color32::from_rgb(100, 200, 255))
                            .size(16.0),
                    );
                });
                ui.add_space(10.0);

                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        ui.set_min_height(ui.available_height() - 30.0);
                        for (line, is_error) in &self.output {
                            let color = if *is_error {
                                Color32::from_rgb(255, 100, 100)
                            } else {
                                self.current_color
                            };
                            ui.label(
                                RichText::new(line)
                                    .color(color)
                                    .monospace()
                                    .size(13.0),
                            );
                            ui.add_space(4.0);
                        }

                        ui.horizontal(|ui| {
                            let user = env::var("USER").unwrap_or("user".to_string());
                            let host = "kali";
                            ui.label(
                                RichText::new(format!("{}@{} $ ", user, host))
                                    .color(Color32::from_rgb(100, 200, 255))
                                    .size(14.0)
                                    .monospace(),
                            );

                            let text_edit = egui::TextEdit::singleline(&mut self.input)
                                .hint_text("Enter command or instruction (or 'exit' to quit)")
                                .font(TextStyle::Body)
                                .desired_width(ui.available_width())
                                .margin(Vec2::new(4.0, 4.0))
                                .desired_rows(1);
                            let response = ui.add(text_edit);

                            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                let cmd = self.input.clone();
                                self.input.clear();
                                self.output.push((format!("{}@{} $ {}", user, host, cmd), false));
                                if let Err(e) = self.process_command(&cmd) {
                                    error!("Process command error: {}", e);
                                    self.output.push((format!("Error: {}", e), true));
                                }
                                response.request_focus();
                            }

                            if self.output.len() == 1 {
                                response.request_focus();
                            }

                            if ui.input(|i| i.key_pressed(egui::Key::Tab)) {
                                self.complete_input();
                                response.request_focus();
                            }

                            if ui.input(|i| i.key_pressed(egui::Key::ArrowUp)) && !self.history.is_empty() {
                                let index = self.history_index.map_or(self.history.len() - 1, |i| i.saturating_sub(1));
                                self.history_index = Some(index);
                                self.input = self.history[index].clone();
                                response.request_focus();
                            }
                            if ui.input(|i| i.key_pressed(egui::Key::ArrowDown)) && !self.history.is_empty() {
                                if let Some(index) = self.history_index {
                                    if index + 1 < self.history.len() {
                                        self.history_index = Some(index + 1);
                                        self.input = self.history[index + 1].clone();
                                    } else {
                                        self.history_index = None;
                                        self.input.clear();
                                    }
                                    response.request_focus();
                                }
                            }
                        });
                    });
            });

        // Request repaint to check for command results
        ctx.request_repaint();
    }
}

fn main() {
    env_logger::init();
    info!("Starting terminalAI");
    let args = Args::parse();

    if args.interactive {
        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([800.0, 600.0]),
            ..Default::default()
        };
        info!("Launching GUI with eframe");
        let _ = eframe::run_native(
            "terminalAI",
            native_options,
            Box::new(|cc| Ok(Box::new(TerminalAIApp::new(cc)))),
        );
    } else {
        println!("Please run with --interactive for now.");
    }
}