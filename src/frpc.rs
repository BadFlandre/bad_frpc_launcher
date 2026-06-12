use std::io::BufRead;
use std::sync::mpsc;

#[derive(Clone)]
pub(crate) enum LogEvent {
    Stdout(String),
    Stderr(String),
    Error(String),
}

pub(crate) fn spawn_reader_thread<R: BufRead + Send + 'static>(
    mut reader: R,
    tx: mpsc::Sender<LogEvent>,
    is_err: bool,
) {
    std::thread::spawn(move || {
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    let s = strip_ansi_and_control(line.trim_end_matches(['\r', '\n']));
                    let _ = tx.send(if is_err {
                        LogEvent::Stderr(s)
                    } else {
                        LogEvent::Stdout(s)
                    });
                }
                Err(err) => {
                    let _ = tx.send(LogEvent::Error(err.to_string()));
                    break;
                }
            }
        }
    });
}

fn strip_ansi_and_control(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if let Some('[') = chars.peek().copied() {
                chars.next();
                while let Some(next) = chars.next() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
                continue;
            }
            continue;
        }

        if ch.is_control() && ch != '\t' {
            continue;
        }

        out.push(ch);
    }

    out
}
