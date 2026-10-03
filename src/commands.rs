extern crate alloc;

use alloc::string::String;

#[derive(Debug, PartialEq, Eq)]
pub enum Command<'a> {
    List,
    Load(&'a str),
    Help,
    Empty,
    Invalid,
}

pub fn rom_name(name: &str) -> bool {
    if name.is_empty() || name.contains(['/', '\\', ':']) || name.chars().any(char::is_control) {
        return false;
    }
    name.rsplit_once('.').is_some_and(|(stem, extension)| {
        !stem.is_empty()
            && (extension.eq_ignore_ascii_case("gb") || extension.eq_ignore_ascii_case("gbc"))
    })
}

pub fn parse(line: &str) -> Command<'_> {
    let line = line.trim();
    let (verb, argument) = line.split_once(char::is_whitespace).unwrap_or((line, ""));
    let argument = argument.trim();
    match (verb, argument) {
        ("", "") => Command::Empty,
        ("list", "") => Command::List,
        ("help" | "?", "") => Command::Help,
        ("load", name) if rom_name(name) => Command::Load(name),
        _ => Command::Invalid,
    }
}

/// Bounded line input; an overlong command is discarded through its newline.
pub struct LineInput {
    bytes: [u8; 512],
    len: usize,
    overflow: bool,
}

impl LineInput {
    pub const fn new() -> Self {
        Self {
            bytes: [0; 512],
            len: 0,
            overflow: false,
        }
    }

    pub fn push(&mut self, byte: u8) -> Option<Result<String, &'static str>> {
        match byte {
            b'\r' | b'\n' => {
                let result = if self.overflow {
                    Err("command too long")
                } else {
                    String::from_utf8(self.bytes[..self.len].to_vec()).map_err(|_| "invalid UTF-8")
                };
                self.len = 0;
                self.overflow = false;
                Some(result)
            }
            3 => {
                self.len = 0;
                self.overflow = false;
                None
            }
            8 | 127 => {
                self.len = self.len.saturating_sub(1);
                None
            }
            0x20..=0xff => {
                if self.len < self.bytes.len() && !self.overflow {
                    self.bytes[self.len] = byte;
                    self.len += 1;
                } else {
                    self.overflow = true;
                }
                None
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_accept_only_rom_files_in_the_app_folder() {
        assert_eq!(parse("list"), Command::List);
        assert_eq!(parse(" load Mario.GBC  "), Command::Load("Mario.GBC"));
        assert_eq!(
            parse("load Super Mario.gb"),
            Command::Load("Super Mario.gb")
        );
        for line in [
            "load",
            "load ../mario.gb",
            "load /mario.gb",
            "load x\\mario.gb",
            "load trueosfs:mario.gb",
            "load mario.zip",
            "load .gb",
            "list extra",
        ] {
            assert_eq!(parse(line), Command::Invalid, "{line}");
        }
    }

    #[test]
    fn handles_backspace_crlf_and_discards_overlong_commands() {
        let mut input = LineInput::new();
        for byte in b"lisx\x08t" {
            assert!(input.push(*byte).is_none());
        }
        assert_eq!(input.push(b'\r'), Some(Ok(String::from("list"))));
        assert_eq!(input.push(b'\n'), Some(Ok(String::new())));
        for _ in 0..513 {
            input.push(b'a');
        }
        assert_eq!(input.push(b'\n'), Some(Err("command too long")));
        for byte in b"list" {
            input.push(*byte);
        }
        assert_eq!(input.push(b'\n'), Some(Ok(String::from("list"))));
    }
}
