# trust — your **TRUSTy** editor 🦀

*(this is not a shell! it just **acts** like one because it’s cool.)*

***not actively maintained as of 2026, but updates will come soon! (pls make issues)***

**trust** is the Rust rewrite / spiritual successor to my C++ `tedit`: same vibe, same command-y workflow, but now with **native Rust + Cargo tools baked in**, theming, line history, tab completion, atomic saves, and multi-buffer editing — all in one fast binary.

It’s basically:

> *“What if tedit, but Rust, and it knew Cargo, and the prompt was pretty?”* :D

Inspired by the classics (*ed*, *ex*), influenced by my old `tedit`, and still shouting-out stuff like [Kokonico’s medit](https://github.com/Kokonico/medit) because that project is sick.

Also check out the original C++ editor project by me: **[tedit](https://github.com/RobertFlexx/tedit)**.

> **Note:** no syntax highlighting yet — this is the command-style, theme-aware version first.

---

## Highlights (Why trust exists)

* **Rust-first, not an afterthought**

  * `cargo run`, `cargo build`, `cargo check` **inside** the editor
  * `rs-run` → dumps current buffer to `/tmp`, `rustc`s it, and runs it
  * `rs-snip main` / `rs-snip struct Foo` → drop in Rust boilerplate instantly
  * `rs-detect` → “does this look like Rust?” (yes, we’re judging)
  * `rs-explain` → reminds you of all the Rusty commands
* **Safe AF**

  * Atomic saves (`.tmp` → `rename`)
  * Optional backups (`file~`)
  * Undo/redo stack (up to 200 ops)
  * Autosave / crash recovery to `~/.trust-recover-*`
* **Pretty CLI, like the C++ tedit but extra**

  * Smart line reader: arrows, history, tab completion (commands first, filesystem after)
  * `cd` with `~` expansion
  * **Theme-aware prompt** — **shows as `trust>`**, color matches theme
  * User input text uses the theme’s “input” color (neon = bright blue, matrix = green, etc.)
  * Help text shows with a little gradient flair so it’s not 1995 anymore
* **Tons of editor-y stuff**

  * Buffers: `new`, `bnext`, `bprev`, `lsb`
  * Printing: `p`, `print 10-30`, `r 42`
  * Editing: `append`, `insert <n>`, `delete <range>`
  * FS helpers: `ls [-l] [-a]`, `pwd`, `cd <dir>`
  * Themes: `default`, `dark`, `neon`, `matrix`, `paper`
* **Built like tedit, but milled in Rust**

  * Safe string handling
  * Works great on Linux/BSD/macOS
  * Uses `libc` on Unix just for raw-mode arrows

---

## What’s Different from OG `tedit`

| feature             | C++ `tedit`                 | Rust `trust`                                            |
| ------------------- | --------------------------- | ------------------------------------------------------- |
| Core language       | C++17                       | **Rust 2021**                                           |
| Rust awareness      | ❌ nope                      | ✅ yes: `cargo`, `rs-run`, `rs-snip`                     |
| Prompt              | `tedit>`                    | **`trust>` (theme colored)**                            |
| Install             | `make && sudo make install` | **`cargo build --release`** or `cargo install --path .` |
| Autosave dir        | `~/.tedit-recover-*`        | **`~/.trust-recover-*`**                                |
| Theming             | yes                         | yes, **plus colorized input**                           |
| Line reader         | added later                 | built-in, Rusty, raw-mode                               |
| Syntax highlighting | ✅ tedit had modes           | **❌ not yet in trust**                                  |

So yeah — **this one actually knows it’s a Rust editor**. The old one was just speedrunning C++.

---

## Install / Build

### 1. Using Cargo (recommended)

```bash
git clone https://github.com/RobertFlexx/trust.git
cd trust
cargo build --release
# binary → target/release/trust
```

Install globally (user):

```bash
cargo install --path .
```

You can also rename it to `trust` in your Cargo.toml/bin section so it shows up as `~/.cargo/bin/trust`.

### 2. Dependencies (what it actually uses)

In `Cargo.toml` you want (roughly):

```toml
[package]
name = "trust"
version = "0.1.0"
edition = "2021"

[[bin]]
name = "trust"
path = "src/main.rs"

[dependencies]
atty = "0.2"
term_size = "0.3"
libc = "0.2"     # for raw terminal mode on Unix
```

That’s it. No 40 crates. No “oops tokio.” Just tiny, simple, CLI Rust like the ancestors intended. :P

---

## Running It

```bash
trust
# or:
trust path/to/file.rs
```

You should see something like:

```text
trust — editing (unnamed) (0 lines). type 'help'
[...status line...]
trust>
```

If you open a file:

```bash
trust src/main.rs
```

it will load it right away.

---

## Themes (and the prompt colors)

trust ships with:

* `theme default` → cyan prompt, white input
* `theme dark` → cyan-ish prompt, gray input
* `theme neon` → **magenta/purple prompt, neon-ish blue input** (your “original tedit vibe”)
* `theme matrix` → green prompt, green input (hello 1999)
* `theme paper` → gray/black prompt, gray input (boring but classy)

The prompt **always** looks like:

```text
trust>
```

(or `*trust>` when buffer is dirty), but **the colors change per theme**, and **the text you type** shows in the matching input color.

---

## Rust-First Commands (the good stuff)

Inside `trust`, you can do:

```text
trust> rustfmt
trust> rustfmt 10-40
trust> cargo check
trust> cargo build
trust> cargo run
trust> rs-snip main
trust> rs-run
trust> rs-explain
```

What they do:

* **`rustfmt`** → writes your buffer to a tmp file → runs `rustfmt` → re-reads → replaces buffer
* **`cargo ...`** → just calls system `cargo` in the current directory (inherits stdin/stdout/stderr)
* **`rs-snip`** → appends Rust boilerplate to your buffer
* **`rs-run`** → temp-compile and execute (super handy for one-offs)

This **did not** exist in OG tedit. This is **why this thing is “your TRUSTy editor.”**

---

## Editor Commands (Core)

```text
help                # show commands (in pretty colors)
open <path>         # open a file
info                # buffer info
write / w [path]    # save
wq                  # save & quit
quit / q            # quit (asks if dirty)
print / p [range]   # print lines
r <n>               # print single line
append / a          # append until '.'
insert <n>          # insert before line n (until '.')
delete <range>      # delete some lines
find <text>         # search
findi <text>        # case-insensitive search
number              # toggle line numbers
theme <name>        # default/dark/neon/matrix/paper
alias <a> <real>    # make command shortcuts
new                 # new empty buffer
bnext / bprev / lsb # buffer hopping
pwd / cd / ls       # little shell helpers
clear               # clear screen
version             # prints: `trust v0.1.0 🦀`
```

Yes, it also respects `:command` style (leading colon).

---

## Autosave & Recovery

trust will periodically write crash-recovery snapshots to something like:

```text
~/.trust-recover-<hash>
```

so if your WM dies, you don’t lose everything. Mirrors the old C++ behavior, just renamed.

---

## Philosophy

**trust** follows the same idea as my C++ editor: do text editing with a command-led workflow, stay scriptable, stay portable, but because it’s Rust, it can also become your mini Rust lab. Add syntax highlighting later when you’re happy with the command core.

---

## License

MIT keep the credit, ship cool stuff. :P

---

## Related Projects

* **trust** (this repo) — Rust CLI editor with Cargo baked in
* **[tedit](https://github.com/RobertFlexx/tedit)** — original C++ editor that inspired this (mine :P)
* **[medit](https://github.com/Kokonico/medit)** — because its goated
