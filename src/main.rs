use std::collections::HashMap;
use std::fs::{self, File, OpenOptions, Metadata};
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use atty::Stream;
use term_size;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

const APP_NAME: &str = "trust";
const APP_VERSION: &str = "trust v0.1.0 🦀";

const UNDO_MAX: usize = 200;

// ===== Line reader (tedit-like) ======================================
#[cfg(unix)]
fn enable_raw_mode(fd: i32) -> io::Result<libc::termios> {
    unsafe {
        let mut orig: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut orig) != 0 {
            return Err(io::Error::last_os_error());
        }
        let mut raw = orig;
        raw.c_lflag &= !(libc::ECHO | libc::ICANON);
        raw.c_cc[libc::VMIN as usize] = 1;
        raw.c_cc[libc::VTIME as usize] = 0;
        if libc::tcsetattr(fd, libc::TCSAFLUSH, &raw) != 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(orig)
    }
}

#[cfg(unix)]
fn disable_raw_mode(fd: i32, orig: &libc::termios) {
    unsafe {
        let _ = libc::tcsetattr(fd, libc::TCSAFLUSH, orig);
    }
}

struct LineReader {
    history: Vec<String>,
    hist_max: usize,
    commands: Vec<String>,
    input_color: String,
}

impl LineReader {
    fn new() -> Self {
        Self {
            history: Vec::new(),
            hist_max: 800,
            commands: Vec::new(),
            input_color: String::new(),
        }
    }

    fn set_commands<S: AsRef<str>>(&mut self, cmds: &[S]) {
        self.commands = cmds.iter().map(|s| s.as_ref().to_string()).collect();
    }

    fn set_input_color(&mut self, c: &str) {
        self.input_color = c.to_string();
    }

    fn remember(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        if self.history.last().map(|x| x.as_str()) != Some(s) {
            if self.history.len() >= self.hist_max {
                self.history.remove(0);
            }
            self.history.push(s.to_string());
        }
    }

    fn split_words(s: &str) -> Vec<&str> {
        s.split_whitespace().collect()
    }

    fn expand_home(token: &str) -> String {
        if token == "~" {
            return home_path().to_string_lossy().to_string();
        }
        if token.starts_with("~/") {
            let mut p = home_path();
            p.push(&token[2..]);
            return p.to_string_lossy().to_string();
        }
        token.to_string()
    }

    fn complete_dirs_only(token: &str) -> Vec<String> {
        let token = Self::expand_home(token);
        let (dir, base) = match token.rfind('/') {
            Some(idx) => (&token[..idx], &token[idx + 1..]),
            None => (".", token.as_str()),
        };
        let mut out = Vec::new();
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.flatten() {
                if let Ok(md) = e.metadata() {
                    if md.is_dir() {
                        let name = e.file_name().to_string_lossy().to_string();
                        if name.starts_with(base) {
                            if dir == "." {
                                out.push(format!("{}/", name));
                            } else {
                                out.push(format!("{}/{}", dir, name));
                            }
                        }
                    }
                }
            }
        }
        out.sort();
        out
    }

    fn complete_fs(token: &str) -> Vec<String> {
        let token = Self::expand_home(token);
        let (dir, base) = match token.rfind('/') {
            Some(idx) => (&token[..idx], &token[idx + 1..]),
            None => (".", token.as_str()),
        };
        let mut out = Vec::new();
        if let Ok(rd) = fs::read_dir(dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with(base) {
                    let path = if dir == "." {
                        name.clone()
                    } else {
                        format!("{}/{}", dir, name)
                    };
                    if let Ok(md) = e.metadata() {
                        if md.is_dir() {
                            out.push(format!("{}/", path));
                        } else {
                            out.push(path);
                        }
                    } else {
                        out.push(path);
                    }
                }
            }
        }
        out.sort();
        out
    }

    fn complete(&self, buf: &str) -> Vec<String> {
        let toks = Self::split_words(buf);
        let at_start = toks.is_empty();
        let fresh = !buf.is_empty() && buf.ends_with(char::is_whitespace);
        if at_start {
            return self.commands.clone();
        }
        if toks.len() == 1 && !fresh {
            let pref = toks[0];
            return self
            .commands
            .iter()
            .filter(|c| c.starts_with(pref))
            .cloned()
            .collect();
        }
        // after first word
        let first = toks[0];
        if first == "cd" {
            let last = if fresh { "" } else { toks[toks.len() - 1] };
            return Self::complete_dirs_only(last);
        }
        let last = if fresh { "" } else { toks[toks.len() - 1] };
        Self::complete_fs(last)
    }

    fn redraw(&self, prompt: &str, buf: &str, cursor: usize) {
        print!("\r\x1b[2K{}{}{}\x1b[0m", prompt, self.input_color, buf);
        let tail = buf.len().saturating_sub(cursor);
        if tail > 0 {
            print!("\x1b[{}D", tail);
        }
        let _ = io::stdout().flush();
    }

    #[cfg(unix)]
    fn read_line(&mut self, prompt: &str) -> io::Result<String> {
        use std::os::fd::AsRawFd;

        print!("{}", prompt);
        io::stdout().flush()?;

        let stdin = io::stdin();
        let fd = stdin.as_raw_fd();
        let orig = enable_raw_mode(fd)?;

        let mut buf = String::new();
        let mut cursor: usize = 0;
        let mut hist_idx: isize = self.history.len() as isize;

        loop {
            let mut byte = [0u8; 1];
            if stdin.lock().read(&mut byte)? == 0 {
                disable_raw_mode(fd, &orig);
                return Ok(String::new());
            }
            let b = byte[0];
            match b {
                b'\r' | b'\n' => {
                    println!();
                    disable_raw_mode(fd, &orig);
                    self.remember(&buf);
                    return Ok(buf);
                }
                127 | 8 => {
                    if cursor > 0 {
                        buf.remove(cursor - 1);
                        cursor -= 1;
                        self.redraw(prompt, &buf, cursor);
                    }
                }
                b'\t' => {
                    let opts = self.complete(&buf);
                    if opts.is_empty() {
                        // nothing
                    } else if opts.len() == 1 {
                        // single completion
                        let mut toks = buf.split_whitespace().collect::<Vec<_>>();
                        if toks.is_empty() {
                            buf = opts[0].clone();
                        } else {
                            // replace last token
                            let lastsp = buf.rfind(' ');
                            if let Some(idx) = lastsp {
                                buf = format!("{}{}", &buf[..idx + 1], opts[0]);
                            } else {
                                buf = opts[0].clone();
                            }
                        }
                        cursor = buf.len();
                        self.redraw(prompt, &buf, cursor);
                    } else {
                        // show options
                        println!();
                        let mut c = 0;
                        for o in &opts {
                            print!("{}  ", o);
                            c += 1;
                            if c % 6 == 0 {
                                println!();
                            }
                        }
                        if c % 6 != 0 {
                            println!();
                        }
                        self.redraw(prompt, &buf, cursor);
                    }
                }
                27 => {
                    // escape
                    let mut seq = [0u8; 2];
                    if stdin.lock().read(&mut seq[..1]).is_ok() && seq[0] == b'[' {
                        if stdin.lock().read(&mut seq[1..2]).is_ok() {
                            match seq[1] {
                                b'A' => {
                                    // up
                                    if hist_idx > 0 {
                                        hist_idx -= 1;
                                        buf = self.history[hist_idx as usize].clone();
                                        cursor = buf.len();
                                        self.redraw(prompt, &buf, cursor);
                                    }
                                }
                                b'B' => {
                                    // down
                                    if hist_idx < self.history.len() as isize - 1 {
                                        hist_idx += 1;
                                        buf = self.history[hist_idx as usize].clone();
                                        cursor = buf.len();
                                        self.redraw(prompt, &buf, cursor);
                                    } else {
                                        hist_idx = self.history.len() as isize;
                                        buf.clear();
                                        cursor = 0;
                                        self.redraw(prompt, &buf, cursor);
                                    }
                                }
                                b'C' => {
                                    // right
                                    if cursor < buf.len() {
                                        cursor += 1;
                                        self.redraw(prompt, &buf, cursor);
                                    }
                                }
                                b'D' => {
                                    // left
                                    if cursor > 0 {
                                        cursor -= 1;
                                        self.redraw(prompt, &buf, cursor);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                _ => {
                    // printable-ish
                    let ch = b as char;
                    buf.insert(cursor, ch);
                    cursor += 1;
                    self.redraw(prompt, &buf, cursor);
                }
            }
        }
    }

    #[cfg(not(unix))]
    fn read_line(&mut self, prompt: &str) -> io::Result<String> {
        print!("{}", prompt);
        io::stdout().flush()?;
        let mut s = String::new();
        io::stdin().read_line(&mut s)?;
        let s = s.trim_end_matches(&['\r', '\n'][..]).to_string();
        self.remember(&s);
        Ok(s)
    }
}

// ===== END line reader ===============================================

#[derive(Clone)]
struct Buffer {
    path: Option<PathBuf>,
    lines: Vec<String>,
    dirty: bool,
    number: bool,
    backup: bool,
    highlight: bool,
}

impl Buffer {
    fn new() -> Self {
        Self {
            path: None,
            lines: Vec::new(),
            dirty: false,
            number: true,
            backup: true,
            highlight: false,
        }
    }

    fn name(&self) -> String {
        self.path
        .as_ref()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| "(unnamed)".to_string())
    }

    fn char_count(&self) -> usize {
        self.lines.iter().map(|l| l.len() + 1).sum()
    }
}

#[derive(Clone)]
struct Snap {
    lines: Vec<String>,
}

struct Stack {
    st: Vec<Snap>,
}

impl Stack {
    fn new() -> Self {
        Self { st: Vec::new() }
    }
    fn push(&mut self, buf: &Buffer) {
        if self.st.len() == UNDO_MAX {
            self.st.remove(0);
        }
        self.st.push(Snap {
            lines: buf.lines.clone(),
        });
    }
    fn pop(&mut self) -> Option<Snap> {
        self.st.pop()
    }
    fn clear(&mut self) {
        self.st.clear();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Theme {
    Default,
    Dark,
    Neon,
    Matrix,
    Paper,
}

struct Palette {
    accent: &'static str,
    ok: &'static str,
    warn: &'static str,
    err: &'static str,
    dim: &'static str,
    prompt: &'static str,
    input: &'static str,
    gutter: &'static str,
    title: &'static str,
    help_cmd: &'static str,
    help_arg: &'static str,
    help_text: &'static str,
}

fn use_color() -> bool {
    atty::is(Stream::Stdout)
}

fn palette_for(t: Theme) -> Palette {
    if !use_color() {
        return Palette {
            accent: "",
            ok: "",
            warn: "",
            err: "",
            dim: "",
            prompt: "",
            input: "",
            gutter: "",
            title: "",
            help_cmd: "",
            help_arg: "",
            help_text: "",
        };
    }

    const DIM: &str = "\x1b[2m";
    const GREEN: &str = "\x1b[32m";
    const RED: &str = "\x1b[31m";
    const CYAN: &str = "\x1b[36m";
    const YEL: &str = "\x1b[33m";
    const BBLACK: &str = "\x1b[90m";
    const BWHITE: &str = "\x1b[97m";
    const BCYAN: &str = "\x1b[96m";
    const BGREEN: &str = "\x1b[92m";
    const BYEL: &str = "\x1b[93m";
    const BRED: &str = "\x1b[91m";
    const BMAG: &str = "\x1b[95m";
    const BOLD_CYAN: &str = "\x1b[1;36m";
    const BOLD_MAG: &str = "\x1b[1;95m";
    const BOLD_GREEN: &str = "\x1b[1;32m";
    const BOLD_BLACK: &str = "\x1b[1;90m";

    match t {
        Theme::Dark => Palette {
            accent: CYAN,
            ok: GREEN,
            warn: YEL,
            err: RED,
            dim: BBLACK,
            prompt: BCYAN,
            input: BBLACK,  // dark theme! (sigma)
            gutter: BBLACK,
            title: BOLD_CYAN,
            help_cmd: BCYAN,
            help_arg: BBLACK,
            help_text: BBLACK,
        },
        Theme::Neon => Palette {
            accent: BMAG,
            ok: BGREEN,
            warn: BYEL,
            err: BRED,
            dim: BBLACK,
            prompt: BMAG,
            input: BCYAN, // light neon blue
            gutter: BBLACK,
            title: BOLD_MAG,
            help_cmd: BMAG,
            help_arg: BBLACK,
            help_text: BBLACK,
        },
        Theme::Matrix => Palette {
            accent: GREEN,
            ok: BGREEN,
            warn: YEL,
            err: RED,
            dim: BBLACK,
            prompt: BGREEN,
            input: BGREEN, // green
            gutter: BBLACK,
            title: BOLD_GREEN,
            help_cmd: BGREEN,
            help_arg: BBLACK,
            help_text: BBLACK,
        },
        Theme::Paper => Palette {
            accent: BBLACK,
            ok: GREEN,
            warn: YEL,
            err: RED,
            dim: BBLACK,
            prompt: BBLACK,
            input: BBLACK, // gray
            gutter: BBLACK,
            title: BOLD_BLACK,
            help_cmd: BBLACK,
            help_arg: BBLACK,
            help_text: BBLACK,
        },
        Theme::Default => Palette {
            accent: CYAN,
            ok: GREEN,
            warn: YEL,
            err: RED,
            dim: DIM,
            prompt: CYAN,
            input: BWHITE, // white
            gutter: BBLACK,
            title: BOLD_CYAN,
            help_cmd: CYAN,
            help_arg: DIM,
            help_text: DIM,
        },
    }
}

fn home_path() -> PathBuf {
    std::env::var("HOME")
    .map(PathBuf::from)
    .unwrap_or_else(|_| PathBuf::from("."))
}

fn trim(s: &str) -> String {
    s.trim().to_string()
}

fn lower(s: &str) -> String {
    s.chars().map(|c| c.to_ascii_lowercase()).collect()
}

fn digits_for(mut n: usize) -> usize {
    let mut w = 1;
    while n >= 10 {
        n /= 10;
        w += 1;
    }
    w
}

fn load_file(path: &Path, buf: &mut Buffer) -> io::Result<()> {
    buf.lines.clear();
    let f = File::open(path)?;
    let reader = io::BufReader::new(f);
    for line in reader.lines() {
        buf.lines.push(line?);
    }
    buf.dirty = false;
    Ok(())
}

fn atomic_save(path: &Path, buf: &Buffer, backup: bool) -> io::Result<()> {
    if backup && path.exists() {
        let mut backup_path = path.to_path_buf();
        backup_path.set_extension("~");
        let _ = fs::copy(path, &backup_path);
    }
    let mut tmp = path
    .parent()
    .unwrap_or_else(|| Path::new("."))
    .to_path_buf();
    tmp.push(format!(".{}.tmp.{}", APP_NAME, std::process::id()));
    {
        #[cfg(unix)]
        let mut f = OpenOptions::new()
        .write(true)
        .create(true)
        .mode(0o644)
        .open(&tmp)?;

        #[cfg(not(unix))]
        let mut f = OpenOptions::new().write(true).create(true).open(&tmp)?;

        for l in &buf.lines {
            f.write_all(l.as_bytes())?;
            f.write_all(b"\n")?;
        }
        f.flush()?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

fn detect_lang_from_path(path: Option<&PathBuf>) -> &'static str {
    if let Some(p) = path {
        if let Some(ext) = p.extension().and_then(|e| e.to_str()) {
            let ext = ext.to_ascii_lowercase();
            return match ext.as_str() {
                "rs" => "rust",
                "c" | "cc" | "cpp" | "cxx" | "h" | "hpp" => "cpp",
                "py" => "python",
                "sh" | "bash" | "zsh" => "shell",
                "js" | "ts" => "js",
                "html" | "htm" => "html",
                "css" => "css",
                "json" => "json",
                _ => "plain",
            };
        }
    }
    "plain"
}

fn term_width() -> usize {
    if let Some((w, _)) = term_size::dimensions() {
        w
    } else {
        80
    }
}

fn parse_range(s: &str, nlines: usize) -> Option<(usize, usize)> {
    let s = s.trim();
    if s.is_empty() {
        return Some((1, nlines));
    }
    if let Some(idx) = s.find('-') {
        let left = &s[..idx];
        let right = &s[idx + 1..];
        let lo = if left.is_empty() {
            1
        } else {
            left.parse::<usize>().ok()?
        };
        let hi = if right.is_empty() {
            nlines
        } else {
            right.parse::<usize>().ok()?
        };
        if lo == 0 || hi == 0 || lo > hi {
            return None;
        }
        Some((lo, hi.min(nlines)))
    } else {
        let n = s.parse::<usize>().ok()?;
        if n == 0 {
            return None;
        }
        Some((n, n.min(nlines)))
    }
}

// ls helpers
#[cfg(unix)]
fn perm_string(meta: &Metadata) -> String {
    let mode = meta.mode();
    let mut s = String::new();
    s.push(if meta.is_dir() { 'd' } else { '-' });
    let bits = [
        libc::S_IRUSR,
        libc::S_IWUSR,
        libc::S_IXUSR,
        libc::S_IRGRP,
        libc::S_IWGRP,
        libc::S_IXGRP,
        libc::S_IROTH,
        libc::S_IWOTH,
        libc::S_IXOTH,
    ];
    let chars = ['r', 'w', 'x', 'r', 'w', 'x', 'r', 'w', 'x'];
    for (b, ch) in bits.iter().zip(chars.iter()) {
        if (mode & *b as u32) != 0 {
            s.push(*ch);
        } else {
            s.push('-');
        }
    }
    s
}

#[cfg(not(unix))]
fn perm_string(_meta: &Metadata) -> String {
    // boring on non unix
    "----------".to_string()
}

fn gradient_str(s: &str, pal: &Palette) -> String {
    if !use_color() {
        return s.to_string();
    }
    let colors = [pal.title, pal.accent, pal.help_cmd, pal.help_text];
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        out.push_str(colors[i % colors.len()]);
        out.push(ch);
    }
    out.push_str("\x1b[0m");
    out
}

fn gradient_prompt_text(dirty: bool, pal: &Palette) -> String {
    let base = if dirty { "*trust>" } else { "trust>" };
    if !use_color() {
        return format!("{} ", base);
    }
    let colors = [pal.title, pal.accent, pal.help_cmd, pal.input];
    let mut out = String::new();
    for (i, ch) in base.chars().enumerate() {
        out.push_str(colors[i % colors.len()]);
        out.push(ch);
    }
    out.push(' ');
    // don't reset here; linereader will reset after printing input
    out
}

struct Editor {
    buf: Buffer,
    undo: Stack,
    redo: Stack,
    others: Vec<Buffer>,
    theme: Theme,
    pal: Palette,
    last_search: String,
    last_icase: bool,
    autosave_sec: u64,
    last_autosave: Instant,
    aliases: HashMap<String, String>,
    wrap_long: bool,
    truncate_long: bool,
    lr: LineReader,
}

impl Editor {
    fn new() -> Self {
        let theme = Theme::Default;
        let pal = palette_for(theme);
        let mut lr = LineReader::new();
        lr.set_commands(&[
            "help", "open", "info", "write", "w", "wq", "quit", "q", "print", "p", "r", "append",
            "a", "insert", "i", "delete", "d", "find", "findi", "number", "theme", "alias", "new",
            "bnext", "bprev", "lsb", "pwd", "cd", "ls", "undo", "u", "redo", "rustfmt", "cargo",
            "cargo-run", "cargo-check", "cargo-build", "rs-snip", "rs-detect", "rs-explain",
            "version", "clear", "goto", "rs-run",
        ]);
        lr.set_input_color(pal.input);
        Self {
            buf: Buffer::new(),
            undo: Stack::new(),
            redo: Stack::new(),
            others: Vec::new(),
            theme,
            pal,
            last_search: String::new(),
            last_icase: false,
            autosave_sec: 120,
            last_autosave: Instant::now(),
            aliases: HashMap::new(),
            wrap_long: true,
            truncate_long: false,
            lr,
        }
    }

    fn prompt(&self) -> String {
        gradient_prompt_text(self.buf.dirty, &self.pal)
    }

    fn status(&self) {
        let lang = detect_lang_from_path(self.buf.path.as_ref());
        println!(
            "{}[{}] lines={} chars={} lang={} theme={:?} wrap:{}{}\x1b[0m",
            self.pal.dim,
            self.buf.name(),
                 self.buf.lines.len(),
                 self.buf.char_count(),
                 lang,
                 self.theme,
                 if self.wrap_long { "on" } else { "off" },
                     ""
        );
    }

    fn load(&mut self, path: &str) {
        let path_buf = PathBuf::from(path);
        match load_file(&path_buf, &mut self.buf) {
            Ok(_) => {
                self.buf.path = Some(path_buf);
                println!("{}opened {}{}\x1b[0m", self.pal.ok, path, "");
            }
            Err(e) => {
                self.buf = Buffer::new();
                self.buf.path = Some(path_buf);
                println!("{}(new) {} ({}){}\x1b[0m", self.pal.warn, path, e, "");
            }
        }
    }

    fn print_line(&self, i: usize) {
        if i == 0 || i > self.buf.lines.len() {
            return;
        }
        let line = &self.buf.lines[i - 1];
        let gw = if self.buf.number {
            digits_for(self.buf.lines.len()) + 3
        } else {
            0
        };
        if self.buf.number {
            print!(
                "{}{:>width$} | {}\x1b[0m",
                self.pal.gutter,
                i,
                "",
                width = gw - 3
            );
        }
        if self.truncate_long {
            let tw = term_width();
            let max = if tw > gw { tw - gw } else { tw };
            if line.len() > max {
                println!("{}…", &line[..max.saturating_sub(1)]);
            } else {
                println!("{}", line);
            }
        } else {
            println!("{}", line);
        }
    }

    fn print_range(&self, lo: usize, hi: usize) {
        if self.buf.lines.is_empty() {
            println!("(empty)");
            return;
        }
        let lo = lo.max(1);
        let hi = hi.min(self.buf.lines.len());
        for i in lo..=hi {
            self.print_line(i);
        }
    }

    fn push_undo(&mut self) {
        self.undo.push(&self.buf);
        self.redo.clear();
    }

    fn save(&mut self, path_opt: Option<&str>) {
        let target = if let Some(p) = path_opt {
            PathBuf::from(p)
        } else if let Some(p) = &self.buf.path {
            p.clone()
        } else {
            println!("{}save: no filename{}\x1b[0m", self.pal.warn, "");
            return;
        };

        match atomic_save(&target, &self.buf, self.buf.backup) {
            Ok(_) => {
                self.buf.path = Some(target.clone());
                self.buf.dirty = false;
                println!("{}saved to {:?}{}\x1b[0m", self.pal.ok, target, "");
            }
            Err(e) => {
                println!("{}save: {}{}\x1b[0m", self.pal.err, e, "");
            }
        }
    }

    fn autosave_if_needed(&mut self) {
        if self.autosave_sec == 0 {
            return;
        }
        if self.buf.dirty && self.last_autosave.elapsed() >= Duration::from_secs(self.autosave_sec) {
            if let Some(p) = &self.buf.path {
                let mut rec = home_path();
                let hash = fxhash::hash64(p.to_string_lossy().as_bytes());
                rec.push(format!(".trust-recover-{:x}", hash));
                if let Ok(mut f) = File::create(&rec) {
                    for l in &self.buf.lines {
                        let _ = writeln!(f, "{}", l);
                    }
                }
            }
            self.last_autosave = Instant::now();
        }
    }

    fn list_buffers(&self) {
        println!("\x1b[1m* 0 {}\x1b[0m", self.buf.name());
        for (i, b) in self.others.iter().enumerate() {
            println!("  {} {}", i + 1, b.name());
        }
    }

    fn bnext(&mut self) {
        if self.others.is_empty() {
            println!("(only one buffer)");
            return;
        }
        self.others.insert(0, self.buf.clone());
        self.buf = self.others.pop().unwrap();
        println!("[bnext] {}", self.buf.name());
    }

    fn bprev(&mut self) {
        if self.others.is_empty() {
            println!("(only one buffer)");
            return;
        }
        let last = self.others.pop().unwrap();
        self.others.insert(0, self.buf.clone());
        self.buf = last;
        println!("[bprev] {}", self.buf.name());
    }

    fn set_theme(&mut self, name: &str) {
        let t = match lower(name).as_str() {
            "dark" => Theme::Dark,
            "neon" => Theme::Neon,
            "matrix" => Theme::Matrix,
            "paper" => Theme::Paper,
            _ => Theme::Default,
        };
        self.theme = t;
        self.pal = palette_for(t);
        // update line reader input color too
        self.lr.set_input_color(self.pal.input);
        println!("{}theme set{}\x1b[0m", self.pal.ok, "");
    }

    fn search_plain(&mut self, q: &str, icase: bool) {
        let mut hits = 0usize;
        let q_norm = if icase { lower(q) } else { q.to_string() };
        for (i, line) in self.buf.lines.iter().enumerate() {
            let cmp = if icase { lower(line) } else { line.to_string() };
            if cmp.contains(&q_norm) {
                println!("match at {}: {}", i + 1, line);
                hits += 1;
            }
        }
        if hits == 0 {
            println!("no matches");
        }
    }

    fn cargo_cmd(&self, args: &[&str]) {
        println!("{}[cargo {:?}]{}\x1b[0m", self.pal.dim, args, "");
        let mut cmd = Command::new("cargo");
        for a in args {
            cmd.arg(a);
        }
        let status = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
        match status {
            Ok(s) => println!("{}cargo exited with {}{}\x1b[0m", self.pal.dim, s, ""),
            Err(e) => println!("{}cargo error: {}{}\x1b[0m", self.pal.err, e, ""),
        }
    }

    fn rustfmt_current(&mut self, range: Option<(usize, usize)>) {
        let tmpdir = std::env::temp_dir();
        let tmpfile = tmpdir.join("trust-rustfmt.rs");
        {
            let mut f = match File::create(&tmpfile) {
                Ok(f) => f,
                Err(e) => {
                    println!(
                        "{}rustfmt: cannot create temp: {}{}\x1b[0m",
                        self.pal.err, e, ""
                    );
                    return;
                }
            };
            if let Some((lo, hi)) = range {
                let lo = lo.max(1);
                let hi = hi.min(self.buf.lines.len());
                for i in lo..=hi {
                    let _ = writeln!(f, "{}", self.buf.lines[i - 1]);
                }
            } else {
                for l in &self.buf.lines {
                    let _ = writeln!(f, "{}", l);
                }
            }
        }
        let out = Command::new("rustfmt").arg(&tmpfile).output();
        match out {
            Ok(o) if o.status.success() => {
                let mut s = String::new();
                if let Ok(mut f) = File::open(&tmpfile) {
                    let _ = f.read_to_string(&mut s);
                }
                let new_lines: Vec<String> = s.lines().map(|l| l.to_string()).collect();
                self.push_undo();
                if let Some((lo, hi)) = range {
                    let lo = lo.max(1);
                    let hi = hi.min(self.buf.lines.len());
                    self.buf.lines.splice(lo - 1..hi, new_lines);
                } else {
                    self.buf.lines = new_lines;
                }
                self.buf.dirty = true;
                println!("{}rustfmt applied{}\x1b[0m", self.pal.ok, "");
            }
            Ok(o) => {
                println!(
                    "{}rustfmt failed ({}): {}{}\x1b[0m",
                         self.pal.err,
                         o.status,
                         String::from_utf8_lossy(&o.stderr),
                         ""
                );
            }
            Err(e) => {
                println!("{}rustfmt: {}{}\x1b[0m", self.pal.err, e, "");
            }
        }
    }

    fn insert_snip(&mut self, kind: &str) {
        self.push_undo();
        match kind {
            "main" => {
                self.buf.lines.push("fn main() {".to_string());
                self.buf
                .lines
                .push("    println!(\"hello from trust 🦀\");".to_string());
                self.buf.lines.push("}".to_string());
            }
            "mod" => {
                self.buf.lines.push("pub mod my_mod {".to_string());
                self.buf.lines.push("    pub fn hi() {".to_string());
                self.buf
                .lines
                .push("        println!(\"hi from module\");".to_string());
                self.buf.lines.push("    }".to_string());
                self.buf.lines.push("}".to_string());
            }
            x if x.starts_with("struct ") => {
                let name = x.trim_start_matches("struct ").trim();
                self.buf.lines.push(format!("pub struct {} {{", name));
                self.buf.lines.push("    pub id: u32,".to_string());
                self.buf.lines.push("}".to_string());
                self.buf.lines.push(format!("impl {} {{", name));
                self.buf
                .lines
                .push("    pub fn new(id: u32) -> Self {".to_string());
                self.buf
                .lines
                .push("        Self { id }".to_string());
                self.buf.lines.push("    }".to_string());
                self.buf.lines.push("}".to_string());
            }
            _ => {
                println!(
                    "{}rs-snip: unknown snippet (try: main, mod, struct Foo){}\x1b[0m",
                         self.pal.warn, ""
                );
                return;
            }
        }
        self.buf.dirty = true;
        println!("{}snippet inserted{}\x1b[0m", self.pal.ok, "");
    }

    fn expand_path(&self, s: &str) -> PathBuf {
        if s == "~" {
            return home_path();
        }
        if s.starts_with("~/") {
            let mut p = home_path();
            p.push(&s[2..]);
            return p;
        }
        PathBuf::from(s)
    }

    fn cmd_ls(&self, args: &str) {
        let mut all = false;
        let mut longfmt = false;
        let mut target = ".".to_string();

        for tok in args.split_whitespace() {
            match tok {
                "-a" => all = true,
                "-l" => longfmt = true,
                "-la" | "-al" => {
                    all = true;
                    longfmt = true;
                }
                other => {
                    target = other.to_string();
                }
            }
        }

        // tiny safeguard like C++: don't ls /etc/shadow if non-root, huihfguwioeghew lol
        if target == "/etc/shadow" && unsafe { libc::geteuid() } != 0 {
            println!("ls: permission denied");
            return;
        }

        let path = self.expand_path(&target);
        let md = match fs::metadata(&path) {
            Ok(m) => m,
            Err(e) => {
                println!("{}ls: {}{}\x1b[0m", self.pal.err, e, "");
                return;
            }
        };
        if md.is_dir() {
            let mut entries = Vec::new();
            if let Ok(rd) = fs::read_dir(&path) {
                for e in rd.flatten() {
                    entries.push(e);
                }
            }
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let name = e.file_name().to_string_lossy().to_string();
                if !all && name.starts_with('.') {
                    continue;
                }
                let mut shown = name.clone();
                let emd = e.metadata().ok();
                let is_dir = emd.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                if is_dir {
                    shown.push('/');
                }
                if longfmt {
                    if let Some(m) = emd {
                        let perms = perm_string(&m);
                        let size = m.len();
                        println!("{:10} {:8}  {}", perms, size, shown);
                    } else {
                        println!("??????????        ?  {}", shown);
                    }
                } else {
                    println!("{}", shown);
                }
            }
        } else {
            if longfmt {
                let perms = perm_string(&md);
                let size = md.len();
                println!(
                    "{:10} {:8}  {}",
                    perms,
                    size,
                    path.file_name().unwrap().to_string_lossy()
                );
            } else {
                println!("{}", path.file_name().unwrap().to_string_lossy());
            }
        }
    }

    fn clear_screen(&self) {
        print!("\x1b[3J\x1b[H\x1b[2J");
        let _ = io::stdout().flush();
    }

    fn rs_run(&self) {
        // write current buffer to /tmp and run with `rustc /tmp/tmp.rs && /tmp/tmp-bin`(if u read this u kewl)
        let tmpdir = std::env::temp_dir();
        let src = tmpdir.join("trust-run.rs");
        let bin = tmpdir.join("trust-run-bin");
        if let Ok(mut f) = File::create(&src) {
            for l in &self.buf.lines {
                let _ = writeln!(f, "{}", l);
            }
        } else {
            println!("{}rs-run: cannot write tmp source{}\x1b[0m", self.pal.err, "");
            return;
        }
        println!("{}[rs-run] compiling...{}\x1b[0m", self.pal.dim, "");
        let st = Command::new("rustc")
        .arg(&src)
        .arg("-o")
        .arg(&bin)
        .status();
        match st {
            Ok(s) if s.success() => {
                println!("{}[rs-run] running...{}\x1b[0m", self.pal.dim, "");
                let _ = Command::new(&bin)
                .stdin(Stdio::inherit())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .status();
            }
            Ok(s) => {
                println!("{}rs-run: rustc exited with {}{}\x1b[0m", self.pal.err, s, "");
            }
            Err(e) => {
                println!("{}rs-run: {}{}\x1b[0m", self.pal.err, e, "");
            }
        }
    }

    fn show_help(&self) {
        println!("{}", gradient_str("Commands (trust)", &self.pal));
        let rows = [
            ("open <path>", "open file"),
            ("info", "buffer info"),
            ("w|write [path]", "save"),
            ("wq", "save & quit"),
            ("q|quit", "quit"),
            ("p|print [range]", "print lines"),
            ("r <n>", "print line"),
            ("a|append", "append lines"),
            ("i|insert <n>", "insert before n"),
            ("d|delete <range>", "delete lines"),
            ("find <text>", "search"),
            ("findi <text>", "search (icase)"),
            ("goto <n>", "jump to line"),
            ("number", "toggle line nums"),
            ("theme <name>", "set theme"),
            ("alias <from> <to...>", "make alias"),
            ("new", "new buffer"),
            ("bnext|bprev|lsb", "buffer mgmt"),
            ("pwd|cd <dir>", "filesystem"),
            ("ls [-l] [-a] [path]", "list dir (like C++)"),
            ("undo|redo", "undo/redo"),
            ("clear", "clear screen"),
            // rust bits
            ("version", "show version (🦀)"),
            ("rustfmt [range]", "format Rust with rustfmt"),
            ("cargo run/check/build", "run cargo"),
            ("rs-snip main", "insert Rust snippet"),
            ("rs-detect", "is this Rust?"),
            ("rs-explain", "describe Rust specials"),
            ("rs-run", "compile+run current buffer"),
        ];
        for (c, d) in rows {
            println!("  {}{:<26}\x1b[0m  {}", self.pal.help_cmd, c, d);
        }
        println!(
            "{}themes:{} default, dark, neon, matrix, paper{}\x1b[0m",
            self.pal.help_arg, self.pal.help_text, ""
        );
    }

    fn handle(&mut self, line: &str) -> bool {
        self.autosave_if_needed();

        let mut line = trim(line);
        if line.is_empty() {
            return true;
        }
        if line.starts_with(':') {
            line = line[1..].to_string();
        }

        {
            // alias
            let mut parts = line.splitn(2, ' ');
            let first = parts.next().unwrap_or("");
            if let Some(exp) = self.aliases.get(&lower(first)) {
                let rest = parts.next().unwrap_or("");
                line = if rest.is_empty() {
                    exp.clone()
                } else {
                    format!("{} {}", exp, rest)
                };
            }
        }

        let mut parts = line.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        let rest = line[cmd.len()..].trim();
        let lc = lower(cmd);

        if lc == "version" || lc == "ver" {
            if use_color() {
                println!("{}{}{}\x1b[0m", self.pal.title, APP_VERSION, "");
            } else {
                println!("{}", APP_VERSION);
            }
            return true;
        }

        if lc == "help" || lc == "h" || lc == "?" {
            self.show_help();
            return true;
        }

        if lc == "open" {
            if rest.is_empty() {
                println!("{}usage: open <path>\x1b[0m", self.pal.warn);
            } else if self.buf.dirty {
                println!("{}unsaved changes, save first\x1b[0m", self.pal.warn);
            } else {
                self.load(rest);
            }
            return true;
        }

        if lc == "info" {
            println!(
                "file: {}{}",
                self.buf.name(),
                     if self.buf.dirty { " *" } else { "" }
            );
            println!("  lines: {}", self.buf.lines.len());
            println!("  chars: {}", self.buf.char_count());
            return true;
        }

        if lc == "write" || lc == "w" {
            if rest.is_empty() {
                self.save(None);
            } else {
                self.save(Some(rest));
            }
            return true;
        }

        if lc == "wq" {
            self.save(None);
            println!("{}bye!{}\n", self.pal.dim, "\x1b[0m");
            return false;
        }

        if lc == "quit" || lc == "q" {
            if self.buf.dirty {
                println!(
                    "{}Unsaved changes. Quit anyway? [y/N]{}\n",
                    self.pal.warn, "\x1b[0m"
                );
                let mut s = String::new();
                let _ = io::stdin().read_line(&mut s);
                if s.trim().eq_ignore_ascii_case("y") {
                    println!("{}bye!{}\n", self.pal.dim, "\x1b[0m");
                    return false;
                } else {
                    return true;
                }
            } else {
                println!("{}bye!{}\n", self.pal.dim, "\x1b[0m");
                return false;
            }
        }

        if lc == "print" || lc == "p" {
            if rest.is_empty() {
                self.print_range(1, self.buf.lines.len());
            } else if let Some((lo, hi)) = parse_range(rest, self.buf.lines.len()) {
                self.print_range(lo, hi);
            } else {
                println!("{}bad range{}\x1b[0m", self.pal.warn, "");
            }
            return true;
        }

        if lc == "r" {
            if let Ok(n) = rest.parse::<usize>() {
                self.print_line(n);
            } else {
                println!("{}usage: r <n>{}\x1b[0m", self.pal.warn, "");
            }
            return true;
        }

        if lc == "goto" {
            if let Ok(n) = rest.parse::<usize>() {
                self.print_line(n);
            } else {
                println!("{}usage: goto <n>{}\x1b[0m", self.pal.warn, "");
            }
            return true;
        }

        if lc == "append" || lc == "a" {
            self.push_undo();
            println!("enter text; '.' on a line ends");
            loop {
                print!("> ");
                let _ = io::stdout().flush();
                let mut s = String::new();
                if io::stdin().read_line(&mut s).is_err() {
                    break;
                }
                let s = s.trim_end_matches(&['\r', '\n'][..]).to_string();
                if s == "." {
                    break;
                }
                self.buf.lines.push(s);
            }
            self.buf.dirty = true;
            return true;
        }

        if lc == "insert" || lc == "i" {
            if rest.is_empty() {
                println!("{}usage: insert <n>{}\x1b[0m", self.pal.warn, "");
            } else if let Ok(n) = rest.parse::<usize>() {
                self.push_undo();
                println!("enter text; '.' on a line ends");
                let mut added = Vec::new();
                loop {
                    print!("> ");
                    let _ = io::stdout().flush();
                    let mut s = String::new();
                    if io::stdin().read_line(&mut s).is_err() {
                        break;
                    }
                    let s = s.trim_end_matches(&['\r', '\n'][..]).to_string();
                    if s == "." {
                        break;
                    }
                    added.push(s);
                }
                let idx = n.saturating_sub(1).min(self.buf.lines.len());
                for (i, l) in added.into_iter().enumerate() {
                    self.buf.lines.insert(idx + i, l);
                }
                self.buf.dirty = true;
            }
            return true;
        }

        if lc == "delete" || lc == "d" {
            if self.buf.lines.is_empty() {
                println!("(empty)");
                return true;
            }
            if rest.is_empty() {
                println!("{}usage: delete <range>{}\x1b[0m", self.pal.warn, "");
                return true;
            }
            if let Some((lo, hi)) = parse_range(rest, self.buf.lines.len()) {
                self.push_undo();
                let loi = lo - 1;
                let hii = hi;
                self.buf.lines.drain(loi..hii);
                self.buf.dirty = true;
                println!("deleted {} line(s)", hi - lo + 1);
            } else {
                println!("{}bad range{}\x1b[0m", self.pal.warn, "");
            }
            return true;
        }

        if lc == "find" {
            if rest.is_empty() {
                println!("{}usage: find <text>{}\x1b[0m", self.pal.warn, "");
            } else {
                self.last_search = rest.to_string();
                self.last_icase = false;
                self.search_plain(rest, false);
            }
            return true;
        }

        if lc == "findi" {
            if rest.is_empty() {
                println!("{}usage: findi <text>{}\x1b[0m", self.pal.warn, "");
            } else {
                self.last_search = rest.to_string();
                self.last_icase = true;
                self.search_plain(rest, true);
            }
            return true;
        }

        if lc == "number" {
            self.buf.number = !self.buf.number;
            println!("number: {}", if self.buf.number { "on" } else { "off" });
            return true;
        }

        if lc == "theme" {
            if rest.is_empty() {
                println!("{}usage: theme <name>{}\x1b[0m", self.pal.warn, "");
            } else {
                self.set_theme(rest);
            }
            return true;
        }

        if lc == "alias" {
            let mut p = rest.splitn(2, ' ');
            let from = p.next().unwrap_or("");
            let to = p.next().unwrap_or("");
            if from.is_empty() || to.is_empty() {
                println!("{}usage: alias <from> <to...>{}\x1b[0m", self.pal.warn, "");
            } else {
                self.aliases.insert(lower(from), to.to_string());
                println!("alias: {} -> {}", from, to);
            }
            return true;
        }

        if lc == "new" {
            self.others.push(self.buf.clone());
            self.buf = Buffer::new();
            println!("{}(new buffer){}\x1b[0m", self.pal.ok, "");
            return true;
        }
        if lc == "bnext" {
            self.bnext();
            return true;
        }
        if lc == "bprev" {
            self.bprev();
            return true;
        }
        if lc == "lsb" {
            self.list_buffers();
            return true;
        }

        if lc == "pwd" {
            match std::env::current_dir() {
                Ok(d) => println!("{}", d.display()),
                Err(e) => println!("{}pwd: {}{}\x1b[0m", self.pal.err, e, ""),
            }
            return true;
        }

        if lc == "cd" {
            if rest.is_empty() {
                println!("{}cd: missing path{}\x1b[0m", self.pal.warn, "");
            } else {
                let target = self.expand_path(rest);
                if let Err(e) = std::env::set_current_dir(&target) {
                    println!("{}cd: {}{}\x1b[0m", self.pal.err, e, "");
                } else {
                    println!("{}cd: {}{}\x1b[0m", self.pal.ok, target.display(), "");
                }
            }
            return true;
        }

        if lc == "ls" {
            self.cmd_ls(rest);
            return true;
        }

        if lc == "clear" {
            self.clear_screen();
            return true;
        }

        if lc == "undo" || lc == "u" {
            if let Some(s) = self.undo.pop() {
                self.redo.push(&self.buf);
                self.buf.lines = s.lines;
                self.buf.dirty = true;
                println!("undo");
            } else {
                println!("nothing to undo");
            }
            return true;
        }

        if lc == "redo" {
            if let Some(s) = self.redo.pop() {
                self.undo.push(&self.buf);
                self.buf.lines = s.lines;
                self.buf.dirty = true;
                println!("redo");
            } else {
                println!("nothing to redo");
            }
            return true;
        }

        // rustfmt
        if lc == "rustfmt" {
            if rest.is_empty() {
                self.rustfmt_current(None);
            } else if let Some((lo, hi)) = parse_range(rest, self.buf.lines.len()) {
                self.rustfmt_current(Some((lo, hi)));
            } else {
                println!("{}rustfmt: bad range{}\x1b[0m", self.pal.err, "");
            }
            return true;
        }

        // cargo
        if lc == "cargo" {
            if rest.is_empty() {
                self.cargo_cmd(&["check"]);
            } else {
                let args: Vec<&str> = rest.split_whitespace().collect();
                self.cargo_cmd(&args);
            }
            return true;
        }
        if lc == "cargo-run" {
            self.cargo_cmd(&["run"]);
            return true;
        }
        if lc == "cargo-check" {
            self.cargo_cmd(&["check"]);
            return true;
        }
        if lc == "cargo-build" {
            self.cargo_cmd(&["build"]);
            return true;
        }

        if lc == "rs-snip" {
            if rest.is_empty() {
                println!(
                    "{}usage: rs-snip <main|mod|struct Foo>{}\x1b[0m",
                    self.pal.warn, ""
                );
            } else {
                self.insert_snip(rest);
            }
            return true;
        }

        if lc == "rs-detect" {
            let lang = detect_lang_from_path(self.buf.path.as_ref());
            if lang == "rust" {
                println!("{}this buffer looks like Rust{}\x1b[0m", self.pal.ok, "");
            } else {
                println!(
                    "{}this buffer does NOT look like Rust{}\x1b[0m",
                    self.pal.warn, ""
                );
            }
            return true;
        }

        if lc == "rs-explain" {
            println!("Rust helpers in {}:", APP_NAME);
            println!("  version            -> show {} 🦀", APP_VERSION);
            println!("  rustfmt [range]    -> run rustfmt on buffer or range");
            println!("  cargo run/check    -> run cargo in current dir");
            println!("  rs-snip main       -> insert Rust main");
            println!("  rs-snip struct Foo -> insert struct");
            println!("  rs-run             -> quick tmp compile+run");
            return true;
        }

        if lc == "rs-run" {
            self.rs_run();
            return true;
        }

        println!(
            "{}unknown command — type 'help'{}\n\x1b[0m",
            self.pal.warn, ""
        );
        true
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() == 2 && (args[1] == "--version" || args[1] == "-V") {
        println!("{}", APP_VERSION);
        return;
    }

    let mut ed = Editor::new();

    if args.len() >= 2 {
        ed.load(&args[1]);
    }

    println!(
        "{}{} — editing {} ({} lines). type 'help'{}\n\x1b[0m",
             ed.pal.accent,
             APP_NAME,
             ed.buf.name(),
             ed.buf.lines.len(),
             ""
    );

    loop {
        ed.status();
        let line = match ed.lr.read_line(&ed.prompt()) {
            Ok(s) => s,
            Err(_) => break,
        };
        if !ed.handle(&line) {
            break;
        }
    }
}

// tiny hash for recover naming
mod fxhash {
    pub fn hash64(data: &[u8]) -> u64 {
        let mut hash: u64 = 0xcbf29ce484222325;
        for &b in data {
            hash ^= b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        hash
    }
}
// uh.. hi
