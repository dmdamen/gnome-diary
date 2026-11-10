# Diary

A distraction-free, encrypted diary for GNOME.

- **Plain files you can sync.** Each day is one Markdown file in a folder you
  choose, so your online storage client (Nextcloud, pCloud, Dropbox, …) can
  sync it.
- **Encrypted with your password**, in the standard
  [age](https://age-encryption.org) format, so you can read your diary
  without this app.
- **Dates are kept for you.** The app opens on today, and every entry records
  when it was created and last changed.
- **Nothing in the way.** A clean page of text. The header bar slides away
  while you type and comes back when you move the mouse.

## Using it

On first start, Diary asks for a folder and a password.

- If you choose a folder that already holds a diary (for example, one synced
  from another computer), Diary asks for that diary's password instead.
- There is no password recovery. If you forget the password, the entries
  cannot be read.

| Keys | Action |
|---|---|
| Alt+← / Alt+→ | Previous / next day with an entry |
| Alt+Home | Today |
| Ctrl+O | Calendar (days with entries are marked) |
| Ctrl+F | Search all entries |
| Ctrl+L | Lock |
| Ctrl+, | Preferences (text size, folder, password) |
| F11 | Fullscreen |

Saving is automatic, one second after you stop typing, and again when the
window closes. If the text cannot be saved, Diary refuses to close and tells
you why.

## The diary folder

```
Diary/
  .diary/identity.age        your key, encrypted with your password
  .diary/recipient.txt       the matching public key
  .diary/README.txt          how to read the diary without the app
  2026/11/2026-11-10.md.age  one encrypted file per day
```

Decrypted, a day looks like this:

```
created: 2026-11-10T20:55:03+01:00
modified: 2026-11-10T21:04:41+01:00

What I wrote today…
```

To read a day without the app, run this in the diary folder. `age` asks for
your password:

```sh
age -d -i .diary/identity.age 2026/11/2026-11-10.md.age
```

Things to know:
- **The dates are visible** in the file names. Only the contents are encrypted.
- **Sync conflicts.** If another computer changed a day while you were editing
  it, your version is saved and theirs is kept next to it as
  `2026-11-10.conflict-<computer>-<time>.md.age`. Diary tells you when this
  happens.
- **Changing the password** re-encrypts only `.diary/identity.age`. Other
  computers need the new password once that file has synced.

## Development

Everything runs in podman; nothing needs to be installed on your machine.

```sh
./dev.sh test          # fmt, clippy, all tests, and the ≥95% line-coverage gate
./dev.sh quick [args]  # cargo test without coverage (fast red/green loop)
./dev.sh run           # run the app on your desktop (throwaway home folder)
./dev.sh screenshots   # render the main pages to target/screenshots/
./dev.sh install       # build and install natively in ~/.local
./dev.sh flatpak       # optional: build build/diary.flatpak (install with ./dev.sh install-flatpak)
```

The code has three layers:

| Crate | What | Tests |
|---|---|---|
| `crates/diary-core` | Entry format, age encryption, folder layout, atomic saves, conflicts, search | Unit tests, including interop with the real `age` CLI |
| `crates/diary-app` | Session (open day, autosave timing, navigation), settings, password rules | Unit tests with a manual clock |
| `crates/diary` | GTK 4 / libadwaita widgets, kept thin | Headless UI tests (`tests/ui.rs`) under Weston |

New behaviour is written test-first. The coverage report is in
`target/llvm-cov/html/index.html`.
