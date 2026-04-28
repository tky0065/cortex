use std::env;
use std::io::Write;
use std::process;

fn main() -> Result<(), ExitCode> {
    let mut name = String::new();
    let mut show_help = false;

    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                show_help = true;
                break;
            }
            "-name" | "--name" => {
                let val = args.next().ok_or_else(|| process::ExitCode::FAILURE)?;
                name = val.to_string();
                if name.is_empty()
                    || name.chars().any(|c| c.is_control() || c.is_ascii_unicode())
                    || name.len() > 64
                {
                    eprintln!("error: invalid name '{}'", name);
                    return Err(process::ExitCode::FAILURE);
                }
            }
            other => {
                eprintln!("error: unknown flag '{}'", other);
                return Err(process::ExitCode::FAILURE);
            }
        }
    }

    if show_help {
        eprintln!("Usage:");
        eprintln!("  hello [flags]");
        eprintln!("");
        eprintln!("Flags:");
        eprintln!("  -h, --help      Show this help message");
        eprintln!("  -name string    Custom name for greeting (default \"world\")");
        return Ok(process::ExitCode::SUCCESS);
    }

    let msg = if name.is_empty() {
        format!("hello world\n")
    } else {
        format!("Hello, {}!\n", name)
    };

    std::io::stdout().write_all(msg.as_bytes())?;
    Ok(process::ExitCode::SUCCESS)
}