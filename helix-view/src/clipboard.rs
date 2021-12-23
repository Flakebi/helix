// Implementation reference: https://github.com/neovim/neovim/blob/f2906a4669a2eef6d7bf86a29648793d63c98949/runtime/autoload/provider/clipboard.vim#L68-L152

use anyhow::Result;
use futures_util::future::BoxFuture;
use std::borrow::Cow;

#[derive(Clone, Copy, Debug)]
pub enum ClipboardType {
    Clipboard,
    Selection,
}

pub trait ClipboardProvider: std::fmt::Debug + Send + Sync {
    fn name(&self) -> Cow<str>;
    fn get_contents(&self, clipboard_type: ClipboardType) -> BoxFuture<Result<String>>;
    fn set_contents(
        &mut self,
        contents: String,
        clipboard_type: ClipboardType,
    ) -> BoxFuture<Result<()>>;
}

macro_rules! command_provider {
    (paste => $get_prg:literal $( , $get_arg:literal )* ; copy => $set_prg:literal $( , $set_arg:literal )* ; ) => {{
        Box::new(provider::CommandProvider {
            get_cmd: provider::CommandConfig {
                prg: $get_prg,
                args: &[ $( $get_arg ),* ],
            },
            set_cmd: provider::CommandConfig {
                prg: $set_prg,
                args: &[ $( $set_arg ),* ],
            },
            get_primary_cmd: None,
            set_primary_cmd: None,
        })
    }};

    (paste => $get_prg:literal $( , $get_arg:literal )* ;
     copy => $set_prg:literal $( , $set_arg:literal )* ;
     primary_paste => $pr_get_prg:literal $( , $pr_get_arg:literal )* ;
     primary_copy => $pr_set_prg:literal $( , $pr_set_arg:literal )* ;
    ) => {{
        Box::new(provider::CommandProvider {
            get_cmd: provider::CommandConfig {
                prg: $get_prg,
                args: &[ $( $get_arg ),* ],
            },
            set_cmd: provider::CommandConfig {
                prg: $set_prg,
                args: &[ $( $set_arg ),* ],
            },
            get_primary_cmd: Some(provider::CommandConfig {
                prg: $pr_get_prg,
                args: &[ $( $pr_get_arg ),* ],
            }),
            set_primary_cmd: Some(provider::CommandConfig {
                prg: $pr_set_prg,
                args: &[ $( $pr_set_arg ),* ],
            }),
        })
    }};
}

pub fn get_clipboard_provider() -> Box<dyn ClipboardProvider> {
    // TODO: support for user-defined provider, probably when we have plugin support by setting a
    // variable?

    if exists("pbcopy") && exists("pbpaste") {
        command_provider! {
            paste => "pbpaste";
            copy => "pbcopy";
        }
    } else if env_var_is_set("WAYLAND_DISPLAY") && exists("wl-copy") && exists("wl-paste") {
        command_provider! {
            paste => "wl-paste", "--no-newline";
            copy => "wl-copy", "--type", "text/plain";
            primary_paste => "wl-paste", "-p", "--no-newline";
            primary_copy => "wl-copy", "-p", "--type", "text/plain";
        }
    } else if env_var_is_set("DISPLAY") && exists("xclip") {
        command_provider! {
            paste => "xclip", "-o", "-selection", "clipboard";
            copy => "xclip", "-i", "-selection", "clipboard";
            primary_paste => "xclip", "-o";
            primary_copy => "xclip", "-i";
        }
    } else if env_var_is_set("DISPLAY") && exists("xsel") && is_exit_success("xsel", &["-o", "-b"])
    {
        // FIXME: check performance of is_exit_success
        command_provider! {
            paste => "xsel", "-o", "-b";
            copy => "xsel", "-i", "-b";
            primary_paste => "xsel", "-o";
            primary_copy => "xsel", "-i";
        }
    } else if exists("lemonade") {
        command_provider! {
            paste => "lemonade", "paste";
            copy => "lemonade", "copy";
        }
    } else if exists("doitclient") {
        command_provider! {
            paste => "doitclient", "wclip", "-r";
            copy => "doitclient", "wclip";
        }
    } else if exists("win32yank.exe") {
        // FIXME: does it work within WSL?
        command_provider! {
            paste => "win32yank.exe", "-o", "--lf";
            copy => "win32yank.exe", "-i", "--crlf";
        }
    } else if exists("termux-clipboard-set") && exists("termux-clipboard-get") {
        command_provider! {
            paste => "termux-clipboard-get";
            copy => "termux-clipboard-set";
        }
    } else if env_var_is_set("TMUX") && exists("tmux") {
        command_provider! {
            // Refresh tmux clipboard, wait a bit for it to be updated and paste it
            paste => "sh", "-c", "tmux refresh-client -l; sleep 0.1; tmux save-buffer -";
            copy => "tmux", "load-buffer", "-w", "-";
        }
    } else {
        #[cfg(target_os = "windows")]
        return Box::new(provider::WindowsProvider::default());

        #[cfg(not(target_os = "windows"))]
        Box::new(provider::TermProvider::default())
    }
}

fn exists(executable_name: &str) -> bool {
    which::which(executable_name).is_ok()
}

fn env_var_is_set(env_var_name: &str) -> bool {
    std::env::var_os(env_var_name).is_some()
}

fn is_exit_success(program: &str, args: &[&str]) -> bool {
    std::process::Command::new(program)
        .args(args)
        .output()
        .ok()
        .and_then(|out| out.status.success().then(|| ())) // TODO: use then_some when stabilized
        .is_some()
}

mod provider {
    use super::{ClipboardProvider, ClipboardType};
    use anyhow::{bail, Context as _, Result};
    use futures_util::future::{self, BoxFuture};
    use std::borrow::Cow;

    #[cfg(not(target_os = "windows"))]
    #[derive(Debug, Default)]
    pub struct NopProvider {
        buf: String,
        primary_buf: String,
    }

    #[cfg(not(target_os = "windows"))]
    impl ClipboardProvider for NopProvider {
        fn name(&self) -> Cow<str> {
            Cow::Borrowed("none")
        }

        fn get_contents(&self, clipboard_type: ClipboardType) -> BoxFuture<Result<String>> {
            let value = match clipboard_type {
                ClipboardType::Clipboard => self.buf.clone(),
                ClipboardType::Selection => self.primary_buf.clone(),
            };

            Box::pin(future::ok(value))
        }

        fn set_contents(
            &mut self,
            content: String,
            clipboard_type: ClipboardType,
        ) -> BoxFuture<Result<()>> {
            match clipboard_type {
                ClipboardType::Clipboard => self.buf = content,
                ClipboardType::Selection => self.primary_buf = content,
            }
            Box::pin(future::ok(()))
        }
    }

    /// Clipboard provider using ANSI escape sequences.
    ///
    /// The clipboard sequences are described at https://invisible-island.net/xterm/ctlseqs/ctlseqs.html
    #[cfg(not(target_os = "windows"))]
    #[derive(Debug, Default)]
    pub struct TermProvider(NopProvider);

    #[cfg(not(target_os = "windows"))]
    impl TermProvider {
        fn get_clip_char(clipboard_type: ClipboardType) -> &'static str {
            match clipboard_type {
                ClipboardType::Clipboard => "",
                ClipboardType::Selection => "p",
            }
        }

        async fn term_command(cmd: &str) -> Result<String> {
            use std::time::Duration;
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            use tokio::time::timeout;

            let mut file = tokio::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/tty")
                .await?;
            file.write_all(cmd.as_bytes()).await?;

            let mut res = String::new();
            loop {
                let b = timeout(Duration::from_millis(100), file.read_u8())
                    .await
                    .context("Reading escape code response")??;
                res.push(b as char);
                if b == b'\\' {
                    break;
                }
            }
            Ok(res)
        }
    }

    #[cfg(not(target_os = "windows"))]
    impl ClipboardProvider for TermProvider {
        fn name(&self) -> Cow<str> {
            Cow::Borrowed("ansi-escape-codes")
        }

        fn get_contents(&self, clipboard_type: ClipboardType) -> BoxFuture<Result<String>> {
            Box::pin(async move {
                if let Ok(value) = Self::term_command(&format!(
                    "\x1b]52;{};?\x1b\\",
                    Self::get_clip_char(clipboard_type),
                ))
                .await
                {
                    // Format is \b]52;c;<base64>\b\\
                    if let Some(rest) = value
                        .strip_prefix("\x1b]52;")
                        .and_then(|s| s.strip_suffix("\x1b\\"))
                    {
                        if let Some(start) = rest.find(';') {
                            log::debug!("Got clipboard response from terminal");
                            return Ok(String::from_utf8(base64::decode(&rest[start + 1..])?)?);
                        }
                    }

                    log::debug!("unexpected clipboard escape sequence: {:?}", value);
                    bail!("The clipboard escape sequence does not have the expected format");
                } else {
                    // Fallback
                    log::debug!("Use fallback clipboard");
                    self.0.get_contents(clipboard_type).await
                }
            })
        }

        fn set_contents(
            &mut self,
            content: String,
            clipboard_type: ClipboardType,
        ) -> BoxFuture<Result<()>> {
            let _ = self.0.set_contents(content.clone(), clipboard_type);
            Box::pin(future::ready(
                crossterm::execute!(
                    std::io::stdout(),
                    crossterm::style::Print(format!(
                        "\x1b]52;{};{}\x1b\\",
                        Self::get_clip_char(clipboard_type),
                        base64::encode(content)
                    ))
                )
                .map_err(|e| e.into()),
            ))
        }
    }

    #[cfg(target_os = "windows")]
    #[derive(Default, Debug)]
    pub struct WindowsProvider;

    #[cfg(target_os = "windows")]
    impl ClipboardProvider for WindowsProvider {
        fn name(&self) -> Cow<str> {
            Cow::Borrowed("clipboard-win")
        }

        fn get_contents(&self, clipboard_type: ClipboardType) -> Result<String> {
            match clipboard_type {
                ClipboardType::Clipboard => {
                    let contents = clipboard_win::get_clipboard(clipboard_win::formats::Unicode)?;
                    Ok(contents)
                }
                ClipboardType::Selection => Ok(String::new()),
            }
        }

        fn set_contents(&mut self, contents: String, clipboard_type: ClipboardType) -> Result<()> {
            match clipboard_type {
                ClipboardType::Clipboard => {
                    clipboard_win::set_clipboard(clipboard_win::formats::Unicode, contents)?;
                }
                ClipboardType::Selection => {}
            };
            Ok(())
        }
    }

    #[derive(Clone, Debug)]
    pub struct CommandConfig {
        pub prg: &'static str,
        pub args: &'static [&'static str],
    }

    impl CommandConfig {
        async fn execute(&self, input: Option<&str>, pipe_output: bool) -> Result<Option<String>> {
            use std::process::Stdio;
            use tokio::io::AsyncWriteExt;
            use tokio::process::Command;

            let stdin = input.map(|_| Stdio::piped()).unwrap_or_else(Stdio::null);
            let stdout = pipe_output.then(Stdio::piped).unwrap_or_else(Stdio::null);

            let mut child = Command::new(self.prg)
                .args(self.args)
                .stdin(stdin)
                .stdout(stdout)
                .stderr(Stdio::null())
                .spawn()?;

            if let Some(input) = input {
                let mut stdin = child.stdin.take().context("stdin is missing")?;
                stdin
                    .write_all(input.as_bytes())
                    .await
                    .context("couldn't write in stdin")?;
            }

            // TODO: add timer?
            let output = child.wait_with_output().await?;

            if !output.status.success() {
                bail!("clipboard provider {} failed", self.prg);
            }

            if pipe_output {
                Ok(Some(String::from_utf8(output.stdout)?))
            } else {
                Ok(None)
            }
        }
    }

    #[derive(Debug)]
    pub struct CommandProvider {
        pub get_cmd: CommandConfig,
        pub set_cmd: CommandConfig,
        pub get_primary_cmd: Option<CommandConfig>,
        pub set_primary_cmd: Option<CommandConfig>,
    }

    impl ClipboardProvider for CommandProvider {
        fn name(&self) -> Cow<str> {
            if self.get_cmd.prg != self.set_cmd.prg {
                Cow::Owned(format!("{}+{}", self.get_cmd.prg, self.set_cmd.prg))
            } else {
                Cow::Borrowed(self.get_cmd.prg)
            }
        }

        fn get_contents(&self, clipboard_type: ClipboardType) -> BoxFuture<Result<String>> {
            let cmd = match clipboard_type {
                ClipboardType::Clipboard => &self.get_cmd,
                ClipboardType::Selection => {
                    if let Some(cmd) = &self.get_primary_cmd {
                        cmd
                    } else {
                        return Box::pin(future::ok(String::new()));
                    }
                }
            }
            .clone();

            Box::pin(async move { cmd.execute(None, true).await?.context("output is missing") })
        }

        fn set_contents(
            &mut self,
            value: String,
            clipboard_type: ClipboardType,
        ) -> BoxFuture<Result<()>> {
            let cmd = match clipboard_type {
                ClipboardType::Clipboard => &self.set_cmd,
                ClipboardType::Selection => {
                    if let Some(cmd) = &self.set_primary_cmd {
                        cmd
                    } else {
                        return Box::pin(future::ok(()));
                    }
                }
            }
            .clone();

            Box::pin(async move { cmd.execute(Some(&value), false).await.map(|_| ()) })
        }
    }
}
