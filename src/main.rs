#![cfg_attr(windows, windows_subsystem = "windows")]

mod display;
mod registry;
mod source;
mod ui;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use registry::Location;

#[derive(Parser)]
#[command(name = "ctxmenu", about = "Manage Windows context menu entries")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// List context menu entries
    List {
        /// Filter by location
        #[arg(long, default_value = "all")]
        location: LocationFilter,
    },
    /// Disable a context menu entry
    Disable {
        /// Name of the entry to disable
        name: String,
    },
    /// Enable a previously disabled context menu entry
    Enable {
        /// Name of the entry to enable
        name: String,
    },
}

#[derive(Clone, ValueEnum)]
enum LocationFilter {
    Files,
    Folders,
    Background,
    All,
}

impl LocationFilter {
    fn to_location(&self) -> Option<Location> {
        match self {
            LocationFilter::All => None,
            LocationFilter::Files => Some(Location::Files),
            LocationFilter::Folders => Some(Location::Folders),
            LocationFilter::Background => Some(Location::Background),
        }
    }
}

/// Attach to the parent console so CLI output is visible even with windows_subsystem = "windows".
#[cfg(windows)]
fn attach_console() {
    unsafe {
        windows_sys::Win32::System::Console::AttachConsole(
            windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => ui::run(),
        Some(cmd) => {
            #[cfg(windows)]
            attach_console();

            match cmd {
                Command::List { location } => {
                    let entries = registry::scan_entries(location.to_location().as_ref())?;
                    if entries.is_empty() {
                        println!("No context menu entries found.");
                    } else {
                        display::print_table(&entries);
                    }
                }
                Command::Disable { name } => {
                    registry::require_admin()?;
                    registry::disable_entry(&name)?;
                    println!("Disabled '{name}'.");
                }
                Command::Enable { name } => {
                    registry::require_admin()?;
                    registry::enable_entry(&name)?;
                    println!("Enabled '{name}'.");
                }
            }
            Ok(())
        }
    }
}
