use std::io::{self, Write};

use crate::clock;

pub struct Screen {
    last: Option<String>,
    width: usize,
    open: bool,
}

impl Screen {
    pub fn new() -> Self {
        Self {
            last: None,
            width: 0,
            open: false,
        }
    }

    pub fn update(&mut self, summary: &str) {
        let line = format!("{} status {}", clock::stamp(), summary);
        self.width = self.width.max(line.len());

        if self.last.as_deref().is_some_and(|last| last != summary) {
            println!();
        }

        print!("\r{:<width$}", line, width = self.width);
        let _ = io::stdout().flush();
        self.last = Some(summary.to_string());
        self.open = true;
    }

    pub fn close(&mut self) {
        if self.open {
            println!();
            self.open = false;
        }
    }
}
