use std::ffi::OsStr;

#[cfg(windows)]
use smol::process::windows::CommandExt;

/// Create a new smol::process::Command with the given name.
///
/// If the target platform is Windows, the command will be created with the
/// CREATE_NO_WINDOW flag.
pub fn command<S>(name: S) -> smol::process::Command
where
    S: AsRef<OsStr>,
{
    let mut command = smol::process::Command::new(name);

    #[cfg(windows)]
    command.creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0);

    command
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_command() {
        let command = super::command("echo");
        assert_eq!(command.get_program(), "echo");
    }
}
